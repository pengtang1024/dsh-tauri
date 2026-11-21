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
    match exec::run(&npm(), &["--version"], Duration::from_secs(30)) {
        Ok(outcome) if outcome.ok() => Ok(()),
        _ => Err(npm_missing())
    }
}

fn npm_missing() -> String {
    format!(
        "找不到可用的 npm：外壳需要用 npm 安装 {PACKAGE}。\n\n请先安装 Node.js 24+ 与 npm，例如：\n  sudo apt install nodejs npm\n\n已安装但不在 PATH 时，用 DSH_TAURI_NPM=/path/to/npm 指定。"
    )
}

/// Install `spec` (usually `latest`) into `dir` atomically.
pub fn install(dir: &Path, spec: &str) -> Result<(), String> {
    let staging = Staging::new(dir)?;
    let prefix = staging.path().display().to_string();
    eprintln!("[runtime] 在临时目录 {prefix} 安装，完成后原子替换 {}", dir.display());
    run_install(&prefix, spec, dir)?;
    if !has_package(staging.path()) {
        return Err(format!(
            "安装结束，但临时目录 {prefix} 里仍缺少 {PACKAGE} 的入口文件；原运行时未改动。\n\n请手动运行：\n  npm install --prefix {} {PACKAGE}@latest",
            dir.display()
        ));
    }
    staging.commit()
}

/// `npm install --prefix <staging> …`, logged line by line and time-bounded.
/// Error text always points at `manual`, the real runtime path the user can use.
fn run_install(prefix: &str, spec: &str, manual: &Path) -> Result<(), String> {
    let target = format!("{PACKAGE}@{spec}");
    let args = ["install", "--prefix", prefix, "--no-audit", "--no-fund", "--loglevel=error", &target];
    let outcome = exec::run(&npm(), &args, INSTALL_TIMEOUT)?;
    for line in outcome.stdout.lines().chain(outcome.stderr.lines()) {
        if !line.trim().is_empty() {
            eprintln!("[runtime] npm: {line}");
        }
    }
    let command = format!("npm install --prefix {} {target}", manual.display());
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
