//! Install `@deepseek-ai/dsh` with npm, never touching the live runtime.
//!
//! Every install builds the tree in a staging directory and swaps it in only
//! after npm succeeded (see `swap.rs`), so a failed or interrupted install
//! leaves the previous runtime usable. The npm child runs in its own session and
//! is killed as a process group on timeout or shell shutdown (`exec.rs`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::exec;
use crate::runtime::{has_package, PACKAGE};
use crate::swap::Staging;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);

/// npm honours the user's own config, so mirrors work out of the box.
pub fn npm() -> PathBuf {
    std::env::var_os("DSH_TAURI_NPM")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("npm"))
}

pub fn npm_available() -> Result<(), String> {
    let program = npm();
    match exec::run(&program, &["--version"], Duration::from_secs(30)) {
        Ok(outcome) if outcome.ok() => Ok(()),
        Ok(outcome) => Err(version_probe_error(
            &outcome.status.to_string(),
            &outcome.tail(4),
            outcome.timed_out,
            &program.display().to_string(),
        )),
        Err(error) => Err(run_error_message(&error))
    }
}

/// Classify an `exec::run` failure instead of blaming a missing npm for every
/// error: only a real `No such file or directory` means npm was not found.
fn run_error_message(error: &str) -> String {
    if error == "shell is shutting down" {
        return "外壳正在关闭，npm 版本探测已取消".to_owned();
    }
    if error.contains("No such file or directory") {
        return npm_missing();
    }
    format!(
        "npm 版本探测未能执行：{error}\n\n请核对 DSH_TAURI_NPM 指向的 npm 路径与执行权限（例如 DSH_TAURI_NPM=/path/to/npm）后重试。"
    )
}

/// A spawned npm that fails its version probe is a different problem from a
/// missing npm: keep the real status, stderr and configured path for the user.
fn version_probe_error(status_text: &str, stderr_tail: &str, timed_out: bool, npm_label: &str) -> String {
    let detail = if timed_out {
        format!("执行超时（30 秒），最后状态：{status_text}")
    } else {
        format!("执行失败，退出状态：{status_text}")
    };
    let tail = if stderr_tail.trim().is_empty() {
        "（stderr 无输出）"
    } else {
        stderr_tail
    };
    format!(
        "已找到 npm，但 `npm --version` {detail}。\n\nnpm stderr 末尾：\n{tail}\n\n当前 DSH_TAURI_NPM（npm 路径）：{npm_label}\n修复建议：确认该路径指向可执行的 npm；若 npm 不在 PATH，用 DSH_TAURI_NPM=/path/to/npm 指定后重试；若 stderr 显示脚本或依赖问题，请先修复 npm 环境。"
    )
}

fn npm_missing() -> String {
    format!(
        "找不到可用的 npm：外壳需要用 npm 安装 {PACKAGE}。\n\n请安装 Node.js 24+（发行版包管理器可能版本过旧，建议从 https://nodejs.org/ 或 nvm 获取），npm 随 Node 一起安装。\n\n已安装但不在 PATH 时，用 DSH_TAURI_NPM=/path/to/npm 指定。"
    )
}

/// The npm package spec: `<package>@<spec>`, e.g. `@deepseek-ai/dsh@latest`.
fn install_target(spec: &str) -> String {
    format!("{PACKAGE}@{spec}")
}

/// Exact argv handed to npm, in order.
fn install_args(prefix: &str, target: &str) -> [String; 7] {
    [
        "install".to_owned(),
        "--prefix".to_owned(),
        prefix.to_owned(),
        "--no-audit".to_owned(),
        "--no-fund".to_owned(),
        "--loglevel=error".to_owned(),
        target.to_owned()
    ]
}

/// POSIX single-quote a value for a copy-pasteable shell command line:
/// wrap it in `'…'`, and write each internal `'` as the close-escape-reopen
/// sequence `'\''` so the shell passes it through literally.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The command a user can run by hand when an install fails.
fn manual_command(manual: &Path, target: &str) -> String {
    format!(
        "npm install --prefix {} {target}",
        shell_quote(&manual.display().to_string())
    )
}

/// Install `spec` (usually `latest`) into `dir` atomically.
pub fn install(dir: &Path, spec: &str) -> Result<(), String> {
    let staging = Staging::new(dir)?;
    let prefix = staging.path().display().to_string();
    eprintln!("[runtime] 在临时目录 {prefix} 安装，完成后原子替换 {}", dir.display());
    run_install(&prefix, spec, dir)?;
    if !has_package(staging.path()) {
        let command = manual_command(dir, &install_target("latest"));
        return Err(format!(
            "安装结束，但临时目录 {prefix} 里仍缺少 {PACKAGE} 的入口文件；原运行时未改动。\n\n请手动运行：\n  {command}"
        ));
    }
    staging.commit()
}

/// `npm install --prefix <staging> …`, logged line by line and time-bounded.
/// Error text always points at `manual`, the real runtime path the user can use.
fn run_install(prefix: &str, spec: &str, manual: &Path) -> Result<(), String> {
    let target = install_target(spec);
    let args = install_args(prefix, &target);
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = exec::run(&npm(), &borrowed, INSTALL_TIMEOUT)?;
    for line in outcome.stdout.lines().chain(outcome.stderr.lines()) {
        if !line.trim().is_empty() {
            eprintln!("[runtime] npm: {line}");
        }
    }
    let command = manual_command(manual, &target);
    if outcome.timed_out {
        return Err(format!(
            "安装 {target} 超时（{} 分钟），已终止 npm 及其子进程。\n\n请检查网络后重试，或手动运行：\n  {command}",
            INSTALL_TIMEOUT.as_secs() / 60
        ));
    }
    if !outcome.ok() {
        return Err(format!(
            "安装 {target} 失败（{}）。\n\nnpm 输出：\n{}\n\n请检查网络或代理后重试，或手动运行：\n  {command}",
            outcome.status,
            outcome.tail(8)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{install_args, install_target, manual_command, npm_missing, run_error_message, shell_quote, version_probe_error};
    use std::path::Path;

    #[test]
    fn install_target_uses_latest_spec() {
        assert_eq!(install_target("latest"), "@deepseek-ai/dsh@latest");
    }

    #[test]
    fn install_target_uses_a_concrete_version() {
        assert_eq!(install_target("1.2.3"), "@deepseek-ai/dsh@1.2.3");
        assert_eq!(install_target("0.2.2-rc.5"), "@deepseek-ai/dsh@0.2.2-rc.5");
    }

    #[test]
    fn install_args_keep_the_documented_order() {
        let args = install_args("/tmp/dsh-staging", "@deepseek-ai/dsh@latest");
        assert_eq!(args.len(), 7);
        assert_eq!(args[0], "install");
        assert_eq!(args[1], "--prefix");
        assert_eq!(args[2], "/tmp/dsh-staging");
        assert_eq!(args[3], "--no-audit");
        assert_eq!(args[4], "--no-fund");
        assert_eq!(args[5], "--loglevel=error");
        assert_eq!(args[6], "@deepseek-ai/dsh@latest");
    }

    #[test]
    fn manual_command_includes_path_and_target() {
        let manual = Path::new("/home/user/.dsh/dsh-tauri/runtime");
        let target = install_target("1.2.3");
        let command = manual_command(manual, &target);
        assert_eq!(
            command,
            "npm install --prefix '/home/user/.dsh/dsh-tauri/runtime' @deepseek-ai/dsh@1.2.3"
        );
        assert!(command.contains("/home/user/.dsh/dsh-tauri/runtime"));
        assert!(command.contains("@deepseek-ai/dsh@1.2.3"));
    }

    #[test]
    fn shell_quote_wraps_spaces_and_escapes_single_quotes() {
        assert_eq!(shell_quote("/home/My User/x"), "'/home/My User/x'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
        assert_eq!(
            shell_quote("/home/O'Brien/My Runtime"),
            "'/home/O'\\''Brien/My Runtime'"
        );
        let target = install_target("latest");
        assert_eq!(
            manual_command(Path::new("/home/My User/.dsh/dsh-tauri/runtime"), &target),
            "npm install --prefix '/home/My User/.dsh/dsh-tauri/runtime' @deepseek-ai/dsh@latest"
        );
        assert_eq!(
            manual_command(Path::new("/home/O'Brien/runtime"), &target),
            "npm install --prefix '/home/O'\\''Brien/runtime' @deepseek-ai/dsh@latest"
        );
    }

    #[test]
    fn npm_missing_mentions_install_hints() {
        let message = npm_missing();
        assert!(message.contains("npm"));
        assert!(message.contains("24+"));
        assert!(message.contains("nodejs.org"));
        assert!(message.contains("DSH_TAURI_NPM"));
        assert!(!message.contains("sudo apt"), "must not suggest an apt command that cannot install Node 24+");
    }

    #[test]
    fn version_probe_error_reports_timeout_status_tail_and_path() {
        let message = version_probe_error(
            "signal: 15 (SIGTERM)",
            "npm ERR! lifecycle script hung",
            true,
            "/opt/custom/bin/npm",
        );
        assert!(message.contains("超时"), "timeout branch must say 超时");
        assert!(message.contains("signal: 15 (SIGTERM)"));
        assert!(message.contains("npm ERR! lifecycle script hung"));
        assert!(message.contains("/opt/custom/bin/npm"));
        assert!(!message.contains("找不到可用的 npm"), "must not claim npm is missing");
    }

    #[test]
    fn version_probe_error_reports_failure_without_timeout() {
        let message = version_probe_error(
            "exit status: 127",
            "npm ERR! EACCES permission denied",
            false,
            "npm",
        );
        assert!(!message.contains("超时"), "non-timeout branch must not say 超时");
        assert!(message.contains("exit status: 127"));
        assert!(message.contains("npm ERR! EACCES permission denied"));
        assert!(message.contains("npm"));
        assert!(!message.contains("找不到可用的 npm"), "must not claim npm is missing");
    }

    #[test]
    fn run_error_message_shutdown_does_not_suggest_installing_node() {
        let message = run_error_message("shell is shutting down");
        assert_eq!(message, "外壳正在关闭，npm 版本探测已取消");
        assert!(!message.contains("安装 Node"), "shutdown is not a missing npm");
        assert!(!message.contains("找不到可用的 npm"));
    }

    #[test]
    fn run_error_message_no_such_file_uses_npm_missing() {
        let message = run_error_message("无法运行 /opt/bin/npm：No such file or directory (os error 2)");
        assert_eq!(message, npm_missing(), "a real spawn ENOENT means npm is missing");
    }

    #[test]
    fn run_error_message_permission_denied_keeps_error_and_path_hint() {
        let message = run_error_message("无法运行 /opt/bin/npm：Permission denied (os error 13)");
        assert!(message.contains("无法运行 /opt/bin/npm：Permission denied (os error 13)"));
        assert!(message.contains("DSH_TAURI_NPM"));
        assert!(!message.contains("安装 Node"), "permission denied is not a missing npm");
        assert!(!message.contains("找不到可用的 npm"));
    }
}
