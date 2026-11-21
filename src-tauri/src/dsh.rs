//! What the shell needs to run DSH: where Node lives, which runtime directory to
//! drive, and the facts the renderer shim reports.
//!
//! There is no Electron and no DSH Desktop installation here: `@deepseek-ai/dsh`
//! is a plain Node package, so the shell only needs a Node executable and that
//! package's directory. Locating (and, on a first launch, installing) that
//! directory is `runtime::ensure`'s job.

use std::path::PathBuf;

use serde_json::json;
use tauri::AppHandle;

#[derive(Debug, Clone)]
pub struct Config {
    pub node: PathBuf,
    pub runtime: PathBuf,
    pub source: crate::runtime::Source,
    pub entry: PathBuf,
    pub home: PathBuf,
    pub profile: String,
    pub host: String
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
}

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

impl Config {
    /// Locate the runtime this app should drive, installing it on a first launch.
    pub fn resolve(app: Option<&AppHandle>) -> Result<Self, String> {
        let (runtime, source) = crate::runtime::ensure(app)?;
        let entry = crate::runtime::entry(&runtime);
        if !entry.is_file() {
            return Err(format!(
                "{} 缺少 @deepseek-ai/dsh 的入口文件。\n\n请手动运行：\n  npm install --prefix {} @deepseek-ai/dsh@latest",
                runtime.display(),
                runtime.display()
            ));
        }

        let node = env_path("DSH_TAURI_NODE").unwrap_or_else(|| PathBuf::from("node"));
        Ok(Self {
            node,
            runtime,
            source,
            entry,
            home: env_path("DSH_HOME").unwrap_or_else(|| home_dir().join(".dsh")),
            profile: std::env::var("DSH_TAURI_PROFILE").unwrap_or_else(|_| "web".to_owned()),
            host: std::env::var("DSH_TAURI_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned())
        })
    }
}

/// Facts the renderer shim exposes (profile paths, versions).
pub fn info(app: &AppHandle) -> serde_json::Value {
    let config = Config::resolve(Some(app)).ok();
    json!({
        "runtime": config.as_ref().map(|config| config.entry.display().to_string()),
        "home": config.as_ref().map(|config| config.home.display().to_string()),
        "profile": config.as_ref().map(|config| config.profile.clone()),
        "desktopVersion": option_env!("CARGO_PKG_VERSION")
    })
}
