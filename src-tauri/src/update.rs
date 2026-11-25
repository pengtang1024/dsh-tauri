//! Background update check: ask npm for the newest `@deepseek-ai/dsh`, install
//! it into the same runtime directory when it beats the local version, then
//! tell the user to restart.
//!
//! Nothing here may block a launch or kill the running server: failures are
//! logged and retried on the next start, and the installed files only matter to
//! the next launch. Only runtimes the shell owns are rewritten: a developer
//! checkout or an explicit `DSH_TAURI_RUNTIME` just gets a log line, so tracked
//! files never change behind the user's back.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::exec;
use crate::install;
use crate::progress::{self, Stage};
use crate::runtime::{self, PACKAGE, Source};
use crate::version;

const VIEW_TIMEOUT: Duration = Duration::from_secs(90);

/// Guards against one check per restart within a single process lifetime.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Spawn the check once; `dir` is the runtime the server is running from.
pub fn spawn_once(dir: PathBuf, source: Source) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    if !runtime::auto_update_enabled() {
        eprintln!("[runtime] 自动更新已关闭（DSH_TAURI_OFFLINE / DSH_TAURI_AUTO_UPDATE），跳过版本检查");
        return;
    }
    let spawned = std::thread::Builder::new().name("dsh-update".to_owned()).spawn(move || {
        if let Err(error) = check(&dir, source) {
            eprintln!("[runtime] 更新检查失败（下次启动再试）：{error}");
        }
    });
    if let Err(error) = spawned {
        eprintln!("[runtime] 无法启动更新检查线程：{error}");
    }
}

fn check(dir: &Path, source: Source) -> Result<(), String> {
    let local = runtime::version_at(dir).ok_or_else(|| format!("{} 缺少可读的 package.json version", dir.display()))?;
    let upstream = upstream_version()?;
    if !version::is_newer(&upstream, &local) {
        eprintln!("[runtime] 已是最新版本 {local}（上游 {upstream}）");
        return Ok(());
    }
    if !source.updatable() {
        // Never rewrite a checkout or an explicit directory: those may hold
        // tracked files or an install the user manages by hand.
        eprintln!(
            "[runtime] 发现新版 {upstream}（本地 {local}），但运行时来自 {}，不自动写入；需要更新请在该目录手动 npm install",
            source.label()
        );
        return Ok(());
    }
    eprintln!("[runtime] 发现新版 {upstream}（本地 {local}），后台安装到 {}", dir.display());
    progress::set(Stage::Updating, format!("后台更新 DSH 运行时到 {upstream}"));
    install::install(dir, "latest")?;
    let installed = runtime::version_at(dir).unwrap_or(upstream);
    eprintln!("[runtime] 已更新到 {installed}，下次启动生效");
    notify(&installed);
    Ok(())
}

/// Find the first semver-looking token in the output of `npm view ... version`.
///
/// Scans lines top-to-bottom and, within each line, tokens left-to-right; the
/// first token that fully satisfies the version grammar wins. A token must be
/// the whole version (optional `v`/`V` prefix, `major.minor.patch`, optional
/// `-prerelease` and `+build`), so error codes, URLs and ISO timestamps never
/// qualify. The leading `v` is stripped from the returned string.
fn parse_upstream_version(output: &str) -> Option<String> {
    for line in output.lines() {
        for token in line.split_whitespace() {
            let token = token.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '\'' | '"' | '(' | ')' | '[' | ']'));
            if let Some(version) = parse_version_token(token) {
                return Some(version);
            }
        }
    }
    None
}

/// Validate one whitespace-delimited token as a version and normalize it.
fn parse_version_token(token: &str) -> Option<String> {
    let bytes = token.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'v') | Some(b'V')));
    let start = i;

    // core: three dot-separated runs of ASCII digits
    for part in 0..3 {
        let begin = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == begin {
            return None;
        }
        if part < 2 {
            if bytes.get(i) != Some(&b'.') {
                return None;
            }
            i += 1;
        }
    }

    if bytes.get(i) == Some(&b'-') {
        i = scan_identifiers(bytes, i + 1)?;
    }
    if bytes.get(i) == Some(&b'+') {
        i = scan_identifiers(bytes, i + 1)?;
    }
    if i != bytes.len() {
        return None;
    }
    Some(token[start..i].to_owned())
}

/// Consume one or more dot-separated identifiers of alphanumerics/hyphens.
fn scan_identifiers(bytes: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let begin = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
            i += 1;
        }
        if i == begin {
            return None;
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            continue;
        }
        return Some(i);
    }
}

/// `npm view @deepseek-ai/dsh version` honours the user's npm config (mirrors).
fn upstream_version() -> Result<String, String> {
    let outcome = exec::run(&install::npm(), &["view", PACKAGE, "version"], VIEW_TIMEOUT)?;
    if !outcome.ok() {
        return Err(format!("`npm view {PACKAGE} version` 失败（{}）：{}", outcome.status, outcome.tail(4)));
    }
    parse_upstream_version(&outcome.stdout)
        .ok_or_else(|| format!("`npm view {PACKAGE} version` 没有输出可识别的版本号"))
}

/// Desktop notification; a missing notification daemon is only logged.
fn notify(version: &str) {
    let body = format!("已更新到 {version}，重启生效");
    match Command::new("notify-send").args(["-a", "DSH", "DSH 已更新", body.as_str()]).status() {
        Ok(status) if status.success() => eprintln!("[runtime] 已通过系统通知告知用户：{body}"),
        Ok(status) => eprintln!("[runtime] notify-send 退出码 {status}"),
        Err(error) => eprintln!("[runtime] notify-send 不可用：{error}")
    }
}

#[cfg(test)]
mod tests {
    use super::parse_upstream_version;

    #[test]
    fn parses_prerelease() {
        assert_eq!(parse_upstream_version("0.2.2-rc.5"), Some("0.2.2-rc.5".to_owned()));
    }

    #[test]
    fn strips_leading_v() {
        assert_eq!(parse_upstream_version("v0.3.0"), Some("0.3.0".to_owned()));
        assert_eq!(parse_upstream_version("V1.2.3"), Some("1.2.3".to_owned()));
    }

    #[test]
    fn preserves_build_metadata() {
        assert_eq!(parse_upstream_version("1.2.3+build.7"), Some("1.2.3+build.7".to_owned()));
    }

    #[test]
    fn finds_version_in_noisy_output() {
        let output = "\n\nnpm notice new minor version available\nnpm notice config registry=https://registry.npmjs.org/\n  0.4.1  \nnpm notice done in 1.2s\n";
        assert_eq!(parse_upstream_version(output), Some("0.4.1".to_owned()));
    }

    #[test]
    fn rejects_npm_error_output() {
        let output = "npm error code EROFS\nnpm error syscall open\nnpm error path /usr/lib/node_modules\nnpm error A complete log of this run can be found in: /root/.npm/_logs/2026-11-26T03_42_09_000Z-debug-0.log\n";
        assert_eq!(parse_upstream_version(output), None);
    }

    #[test]
    fn rejects_empty_garbage_and_non_versions() {
        assert_eq!(parse_upstream_version(""), None);
        assert_eq!(parse_upstream_version("  \n\t  "), None);
        assert_eq!(parse_upstream_version("not-a-version"), None);
        assert_eq!(parse_upstream_version("### !! ?? ^&*"), None);
        assert_eq!(parse_upstream_version("2026-11-26T03:42:09+08:00"), None);
        assert_eq!(parse_upstream_version("1.2"), None);
        assert_eq!(parse_upstream_version("1.2.3.4"), None);
    }

    #[test]
    fn takes_first_of_multiple_versions() {
        // Deliberate choice: scan lines top-to-bottom, tokens left-to-right and
        // return the very first valid version. npm prints one version on stdout;
        // if a warning line happens to contain a version too, the earliest
        // occurrence is the least surprising and deterministic answer.
        let output = "npm notice v9.9.9\n1.0.0\n2.0.0\n";
        assert_eq!(parse_upstream_version(output), Some("9.9.9".to_owned()));
        assert_eq!(parse_upstream_version("1.0.0 2.0.0"), Some("1.0.0".to_owned()));
    }

    #[test]
    fn ignores_punctuation_around_token() {
        assert_eq!(parse_upstream_version("[1.2.3]"), Some("1.2.3".to_owned()));
        assert_eq!(parse_upstream_version("(1.2.3),"), Some("1.2.3".to_owned()));
    }
}
