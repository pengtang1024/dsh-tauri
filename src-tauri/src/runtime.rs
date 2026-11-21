//! Locate the npm runtime this shell drives, and say where it came from.
//!
//! The runtime is a directory whose `node_modules/@deepseek-ai/dsh` provides
//! `lib/bin.js`. `ensure` finds one among the known locations, or installs it on
//! a first launch (`install.rs`). The [`Source`] it returns decides whether the
//! updater may rewrite that directory: a developer checkout and an explicit
//! `DSH_TAURI_RUNTIME` are never touched, only the automatic install locations.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::install;
use crate::progress::{self, Stage};

pub const PACKAGE: &str = "@deepseek-ai/dsh";
const ENTRY: &str = "node_modules/@deepseek-ai/dsh/lib/bin.js";

/// Where the runtime came from; only `Home` and `Resource` are ours to rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `DSH_TAURI_RUNTIME`: the user pointed at a directory, leave it alone.
    Explicit,
    /// The repository's `runtime/` (development): tracked files live there.
    Checkout,
    /// `~/.dsh/dsh-tauri/runtime`: installed and updated by this shell.
    Home,
    /// The packaged resource directory.
    Resource
}

impl Source {
    pub fn updatable(self) -> bool {
        matches!(self, Source::Home | Source::Resource)
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Explicit => "DSH_TAURI_RUNTIME 指定的目录",
            Source::Checkout => "仓库 runtime/（checkout）",
            Source::Home => "~/.dsh/dsh-tauri/runtime",
            Source::Resource => "打包资源目录"
        }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
}

fn env_is(name: &str, values: &[&str]) -> bool {
    values.contains(&std::env::var(name).unwrap_or_default().trim())
}

/// `DSH_TAURI_OFFLINE=1` disables every network operation (install and check).
pub fn offline() -> bool {
    env_is("DSH_TAURI_OFFLINE", &["1", "true", "yes", "on"])
}

/// `DSH_TAURI_AUTO_UPDATE=0` keeps installing but stops the update check.
pub fn auto_update_enabled() -> bool {
    !offline() && !env_is("DSH_TAURI_AUTO_UPDATE", &["0", "false", "no", "off"])
}

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

/// Where an automatic install goes.
pub fn install_dir() -> PathBuf {
    home_dir().join(".dsh").join("dsh-tauri").join("runtime")
}

/// The script `node` must run: `<dir>/node_modules/@deepseek-ai/dsh/lib/bin.js`.
pub fn entry(dir: &Path) -> PathBuf {
    dir.join(ENTRY)
}

/// The package's entry point exists, so this directory is a usable runtime.
pub fn has_package(dir: &Path) -> bool {
    entry(dir).is_file()
}

/// Development checkout, installed runtime, then packaged resources.
fn locate(app: Option<&AppHandle>) -> Option<(PathBuf, Source)> {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("runtime");
    let mut candidates = vec![(checkout, Source::Checkout), (install_dir(), Source::Home)];
    if let Some(resource) = app
        .and_then(|handle| handle.path().resource_dir().ok())
        .map(|dir| dir.join("runtime"))
    {
        candidates.push((resource, Source::Resource));
    }
    candidates.into_iter().find(|(dir, _)| has_package(dir))
}

/// Return a usable runtime, installing one on a first launch.
pub fn ensure(app: Option<&AppHandle>) -> Result<(PathBuf, Source), String> {
    if let Some(dir) = env_path("DSH_TAURI_RUNTIME") {
        return if has_package(&dir) { Ok((dir, Source::Explicit)) } else { Err(explicit_missing(&dir)) };
    }
    if let Some(found) = locate(app) {
        return Ok(found);
    }
    if offline() {
        return Err(offline_error());
    }
    install_fresh().map(|dir| (dir, Source::Home))
}

fn explicit_missing(dir: &Path) -> String {
    format!(
        "DSH_TAURI_RUNTIME 指向的 {} 不是可用的运行时（缺少 {ENTRY}）。\n\n请在该目录安装：\n  npm install --prefix {} {PACKAGE}@latest",
        dir.display(),
        dir.display()
    )
}

fn offline_error() -> String {
    let dir = install_dir();
    format!(
        "没有可用的 DSH 运行时，而当前是离线模式（DSH_TAURI_OFFLINE=1），不会联网安装。\n\n请手动安装后重试：\n  mkdir -p {}\n  npm install --prefix {} {PACKAGE}@latest\n\n或者去掉 DSH_TAURI_OFFLINE 让外壳自动安装。",
        dir.display(),
        dir.display()
    )
}

/// Install into the default location during a first launch.
fn install_fresh() -> Result<PathBuf, String> {
    let dir = install_dir();
    install::npm_available()?;
    progress::set(Stage::Installing, "首次启动：正在安装 DSH 运行时");
    eprintln!("[runtime] 首次启动：安装 {PACKAGE}@latest 到 {}", dir.display());
    install::install(&dir, "latest")?;
    if !has_package(&dir) {
        return Err(format!(
            "安装结束，但 {} 里仍缺少 {ENTRY}。\n\n请手动运行：\n  npm install --prefix {} {PACKAGE}@latest",
            dir.display(),
            dir.display()
        ));
    }
    eprintln!("[runtime] 运行时就绪：{}", dir.display());
    Ok(dir)
}

/// Version recorded by the installed package, if it is readable.
pub fn version_at(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("node_modules").join(PACKAGE).join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("version")?.as_str().map(str::to_owned)
}
