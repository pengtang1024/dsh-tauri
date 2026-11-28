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

/// Where the persistent profile choice lives (inside the Harness home).
const PROFILE_CONFIG_FILE: &str = "dsh-tauri-profile.json";
/// Default profile: the ordinary `dsh web` profile, unchanged from before.
pub const DEFAULT_PROFILE: &str = "web";
/// The official Electron desktop profile. Booted through `runProfile`.
pub const DESKTOP_PROFILE: &str = "desktop";

/// Sanitize an environment override: only a simple profile name is accepted,
/// never a path or a value with control characters (config injection).
pub(crate) fn valid_profile_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Read the persistent profile choice. A missing, malformed or hostile file
/// falls back to the default; a name is never accepted from an arbitrary path.
pub(crate) fn persisted_profile(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join(PROFILE_CONFIG_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let raw = value.get("profile")?.as_str()?;
    valid_profile_name(raw).then(|| raw.to_owned())
}

/// Pure precedence: DSH_TAURI_PROFILE > persisted config > default web.
pub(crate) fn pick_profile(env_override: Option<&str>, persisted: Option<&str>) -> String {
    env_override
        .filter(|value| valid_profile_name(value))
        .or_else(|| persisted.filter(|value| valid_profile_name(value)))
        .unwrap_or(DEFAULT_PROFILE)
        .to_owned()
}

/// Resolve the effective profile, shared by boot and the read-only status panel.
pub fn resolve_profile(home: &Path) -> String {
    let override_value = std::env::var("DSH_TAURI_PROFILE").ok();
    pick_profile(override_value.as_deref(), persisted_profile(home).as_deref())
}

/// Write the persistent profile choice atomically inside DSH_HOME.
pub fn set_persisted_profile(home: &Path, profile: &str) -> Result<(), String> {
    if !valid_profile_name(profile) {
        return Err(format!("无效的 profile 名称：{profile}"));
    }
    std::fs::create_dir_all(home).map_err(|error| format!("无法创建配置目录 {}: {error}", home.display()))?;
    let path = home.join(PROFILE_CONFIG_FILE);
    let mut staging = path.as_os_str().to_os_string();
    staging.push(format!(".tmp-{}", std::process::id()));
    let staging = PathBuf::from(staging);
    let payload = json!({ "profile": profile }).to_string();
    std::fs::write(&staging, &payload)
        .and_then(|()| std::fs::rename(&staging, &path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&staging);
            format!("无法写入 {}: {error}", path.display())
        })
}

/// Validate a desktop profile before booting it.
pub fn validate_desktop_profile(home: &Path, profile: &str) -> Result<PathBuf, String> {
    if profile != DESKTOP_PROFILE {
        return Ok(home.join("profiles").join(profile));
    }
    let dir = home.join("profiles").join(DESKTOP_PROFILE);
    if !dir.is_dir() || !dir.join("package.json").is_file() {
        return Err(format!(
            "Desktop profile 目录不可用：{}。\n\n请先用官方桌面端启动一次以创建该 profile，然后重试；也可切回 Web profile。",
            dir.display()
        ));
    }
    Ok(dir)
}


#[derive(Debug, Clone)]
pub struct Config {
    pub node: PathBuf,
    pub runtime: PathBuf,
    pub source: crate::runtime::Source,
    pub entry: PathBuf,
    pub home: PathBuf,
    pub profile: String,
    /// Absolute path of the profile directory (validated for desktop).
    pub profile_dir: PathBuf,
    pub host: String
}

impl Config {
    /// Locate the runtime this app should drive, installing it on a first launch.
    pub fn resolve(app: Option<&AppHandle>) -> Result<Self, String> {
        let (runtime, source) = crate::runtime::ensure(app)?;
        let home = crate::runtime::env_path("DSH_HOME")
            .unwrap_or_else(|| crate::runtime::home_dir().join(".dsh"));
        let profile = resolve_profile(&home);
        let profile_dir = validate_desktop_profile(&home, &profile)?;
        let entry = crate::runtime::entry(&runtime);
        if !entry.is_file() {
            return Err(format!(
                "{} 缺少 @deepseek-ai/dsh 的入口文件。\n\n请手动运行：\n  npm install --prefix {} @deepseek-ai/dsh@latest",
                runtime.display(),
                runtime.display()
            ));
        }

        let node = crate::runtime::env_path("DSH_TAURI_NODE").unwrap_or_else(|| PathBuf::from("node"));
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
            home,
            profile,
            profile_dir,
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
    let home = crate::runtime::env_path("DSH_HOME").unwrap_or_else(|| crate::runtime::home_dir().join(".dsh"));
    let profile = resolve_profile(&home);
    let entry = crate::runtime::locate_existing(Some(app)).map(|dir| crate::runtime::entry(&dir));
    info_into(entry.as_deref(), Some(&home), &profile)
}

#[cfg(test)]
mod tests {
    use super::{
        info_into, persisted_profile, pick_profile, set_persisted_profile,
        validate_desktop_profile, valid_profile_name,
    };
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
    fn pick_profile_honours_environment_then_persisted_then_default() {
        assert_eq!(pick_profile(Some("desktop"), Some("web")), "desktop");
        assert_eq!(pick_profile(None, Some("custom-1")), "custom-1");
        assert_eq!(pick_profile(None, None), "web");
        // Invalid values never win, regardless of their source.
        assert_eq!(pick_profile(Some("../etc/passwd"), Some("desktop")), "desktop");
        assert_eq!(pick_profile(Some(""), Some("bad name")), "web");
        assert!(valid_profile_name("desktop"));
        assert!(!valid_profile_name("web\ncredential"));
    }

    #[test]
    fn desktop_profile_validation_requires_an_official_profile() {
        let dir = std::env::temp_dir().join(format!("dsh-tauri-profile-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dest = dir.join("profiles").join("desktop");
        std::fs::create_dir_all(&dest).expect("create desktop dir");
        assert!(validate_desktop_profile(&dir, "desktop").is_err(), "missing package.json must fail");
        std::fs::write(dest.join("package.json"), "{}").expect("write package.json");
        assert_eq!(validate_desktop_profile(&dir, "desktop").expect("valid"), dest);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persisted_profile_round_trip_is_atomic_and_validated() {
        let dir = std::env::temp_dir().join(format!("dsh-tauri-persist-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(persisted_profile(&dir), None);
        set_persisted_profile(&dir, "desktop").expect("persist desktop");
        assert_eq!(persisted_profile(&dir).as_deref(), Some("desktop"));
        assert!(set_persisted_profile(&dir, "bad/name").is_err());
        assert_eq!(persisted_profile(&dir).as_deref(), Some("desktop"));
        let _ = std::fs::remove_dir_all(&dir);
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
