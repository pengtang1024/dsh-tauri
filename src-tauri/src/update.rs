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

/// `npm view @deepseek-ai/dsh version` honours the user's npm config (mirrors).
fn upstream_version() -> Result<String, String> {
    let outcome = exec::run(&install::npm(), &["view", PACKAGE, "version"], VIEW_TIMEOUT)?;
    if !outcome.ok() {
        return Err(format!("`npm view {PACKAGE} version` 失败（{}）：{}", outcome.status, outcome.tail(4)));
    }
    outcome
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .last()
        .map(str::to_owned)
        .ok_or_else(|| format!("`npm view {PACKAGE} version` 没有输出"))
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
