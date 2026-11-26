//! What the shell needs to run DSH: where Node lives, which runtime directory to
//! drive, and the facts the renderer shim reports.
//!
//! There is no Electron and no DSH Desktop installation here: `@deepseek-ai/dsh`
//! is a plain Node package, so the shell only needs a Node executable and that
//! package's directory. Locating (and, on a first launch, installing) that
//! directory is `runtime::ensure`'s job.

use std::path::{Path, PathBuf};

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
        let host = std::env::var("DSH_TAURI_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
        if !crate::banner::is_loopback_host(&host) {
            return Err(format!(
                "DSH_TAURI_HOST={host} 不是 loopback 地址；外壳只能打开 127.0.0.1 / localhost / ::1 上的服务。请去掉该变量或设为 127.0.0.1。"
            ));
        }
        Ok(Self {
            node,
            runtime,
            source,
            entry,
            home: env_path("DSH_HOME").unwrap_or_else(|| home_dir().join(".dsh")),
            profile: std::env::var("DSH_TAURI_PROFILE").unwrap_or_else(|_| "web".to_owned()),
            host
        })
    }
}

/// Pure assembly of the shim's info payload: any `None` becomes JSON `null`.
pub(crate) fn info_into(entry: Option<&Path>, home: Option<&Path>, profile: &str) -> serde_json::Value {
    json!({
        "runtime": entry.map(|path| path.display().to_string()),
        "home": home.map(|path| path.display().to_string()),
        "profile": profile,
        "desktopVersion": option_env!("CARGO_PKG_VERSION")
    })
}

/// Facts the renderer shim exposes (profile paths, versions).
///
/// This is deliberately read-only: a renderer calling `info()` must never
/// trigger an npm install (which would race the supervisor's staged install).
pub fn info(app: &AppHandle) -> serde_json::Value {
    let home = env_path("DSH_HOME").unwrap_or_else(|| home_dir().join(".dsh"));
    let profile = std::env::var("DSH_TAURI_PROFILE").unwrap_or_else(|_| "web".to_owned());
    let entry = crate::runtime::locate_existing(Some(app)).map(|dir| crate::runtime::entry(&dir));
    info_into(entry.as_deref(), Some(&home), &profile)
}

#[cfg(test)]
mod tests {
    use super::info_into;
    use crate::banner::is_loopback_host;
    use serde_json::Value;
    use std::path::Path;

    #[test]
    fn loopback_hosts_are_accepted() {
        for host in [
            "127.0.0.1",
            "localhost",
            "LOCALHOST",
            "::1",
            "[::1]",
            "  127.0.0.1  ",
            "\tlocalhost\n",
            " [::1] ",
        ] {
            assert!(is_loopback_host(host), "expected {host:?} to be loopback");
        }
    }

    #[test]
    fn non_loopback_hosts_are_rejected() {
        for host in [
            "0.0.0.0",
            "192.168.1.5",
            "example.com",
            "",
            "   ",
            "127.0.0.2",
            "[::2]",
        ] {
            assert!(!is_loopback_host(host), "expected {host:?} to be non-loopback");
        }
    }

    #[test]
    fn info_into_all_some_has_exact_fields_and_runtime_entry() {
        let value = info_into(
            Some(Path::new("/tmp/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js")),
            Some(Path::new("/tmp/home/.dsh")),
            "web",
        );
        assert_eq!(value["runtime"], Value::String("/tmp/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js".to_owned()));
        assert_eq!(value["home"], Value::String("/tmp/home/.dsh".to_owned()));
        assert_eq!(value["profile"], Value::String("web".to_owned()));
        let version = &value["desktopVersion"];
        assert!(version.is_string() || version.is_null(), "desktopVersion must be a string or null, got {version}");
        assert_eq!(value.as_object().map(|map| map.len()), Some(4));
    }

    #[test]
    fn info_into_runtime_none_is_null() {
        let value = info_into(None, Some(Path::new("/home/user/.dsh")), "web");
        assert!(value["runtime"].is_null());
        assert_eq!(value["home"], Value::String("/home/user/.dsh".to_owned()));
        assert_eq!(value["profile"], Value::String("web".to_owned()));
        assert_eq!(value.as_object().map(|map| map.len()), Some(4));
    }

    #[test]
    fn info_into_home_none_is_null() {
        let value = info_into(Some(Path::new("/tmp/bin.js")), None, "web");
        assert!(value["home"].is_null());
        assert_eq!(value["runtime"], Value::String("/tmp/bin.js".to_owned()));
        assert_eq!(value["profile"], Value::String("web".to_owned()));
        assert_eq!(value.as_object().map(|map| map.len()), Some(4));
    }

    #[test]
    fn info_into_profile_special_chars_pass_through() {
        let profile = "dev/中文 \"quoted\" <tag>&?=+ %0A";
        let value = info_into(None, None, profile);
        assert_eq!(value["profile"], Value::String(profile.to_owned()));
        assert!(value["runtime"].is_null());
        assert!(value["home"].is_null());
        assert_eq!(value.as_object().map(|map| map.len()), Some(4));
    }
}
