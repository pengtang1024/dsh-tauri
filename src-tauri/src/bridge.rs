//! Supervises the Node-side bridge (`bridge/host-bridge.mjs`), which owns the
//! upstream DSH desktop Host process and republishes its fork-IPC traffic as
//! newline-delimited JSON on stdout.
//!
//! The Rust shell never talks to the Host directly: it reads bridge events,
//! opens the window once the loopback origin is up, and forwards renderer
//! requests as `{"type":"host","message":…}` lines on the bridge's stdin.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;
use tauri::{AppHandle, Manager, Url};

/// Bridge child handle plus its command channel, kept alive for the app's life.
struct Session {
    child: Child,
    stdin: ChildStdin,
}

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

/// Everything the bridge needs to boot the desktop Host.
pub struct Config {
    pub node: PathBuf,
    pub bridge: PathBuf,
    pub root: PathBuf,
    pub profile: PathBuf,
    pub runtime: PathBuf,
    pub executable: PathBuf,
    pub home: PathBuf,
    pub shim: Option<PathBuf>,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
}

/// Home directory without pulling in another dependency.
fn home_dir() -> PathBuf {
    env_path("HOME")
        .or_else(|| env_path("USERPROFILE"))
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Locate an installed DSH Desktop application root (an `app.asar` that carries
/// `lib/host.js`). The shell reuses the official install instead of vendoring a
/// second copy of the Host, so a Tauri build is a few megabytes rather than a
/// duplicate 500 MB bundle.
fn asar_has_host(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut buffer = vec![0u8; 16 * 1024 * 1024];
    let Ok(read) = file.read(&mut buffer) else { return false };
    // The asar header (a JSON directory listing) sits at the start of the file.
    buffer[..read].windows(8).any(|window| window == b"\"host.js")
}

/// Accept either a directory root (extracted install) or an `app.asar` archive.
fn is_app_root(candidate: &Path) -> bool {
    if candidate.is_dir() {
        return candidate.join("lib/host.js").exists();
    }
    candidate.is_file() && asar_has_host(candidate)
}

fn find_app_root() -> Option<PathBuf> {
    if let Some(explicit) = env_path("DSH_TAURI_APP_ROOT").or_else(|| env_path("DSH_TAURI_ROOT")) {
        return is_app_root(&explicit).then_some(explicit);
    }
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from("/opt/DSH NEXT/resources/app.asar"));
    if let Ok(local) = std::env::var("XDG_DATA_HOME").map(PathBuf::from).or_else(|_| Ok::<_, std::env::VarError>(home_dir().join(".local/share"))) {
        candidates.push(local.join("dsh-desktop/resources/app.asar"));
    }
    candidates.push(PathBuf::from("/usr/lib/dsh-desktop/resources/app.asar"));
    candidates.push(PathBuf::from("/Applications/DSH.app/Contents/Resources/app.asar"));
    candidates.into_iter().find(|candidate| is_app_root(candidate))
}

impl Config {
    /// Resolve the bridge configuration, honouring `DSH_TAURI_*` overrides so a
    /// development checkout can point at a different installation.
    pub fn resolve(app: Option<&AppHandle>) -> Result<Self, String> {
        let root = find_app_root().ok_or_else(|| {
            "no DSH Desktop installation found; install DSH Desktop or set DSH_TAURI_APP_ROOT to \
             its resources/app.asar"
                .to_owned()
        })?;
        // `resources/…/app.asar` → `resources`
        let resources = root
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("unexpected app root {}", root.display()))?;
        // Packaged builds carry the bridge under the resource directory; a
        // development checkout uses the sibling `bridge/` folder.
        let bridge = env_path("DSH_TAURI_BRIDGE")
            .or_else(|| {
                app.and_then(|handle| handle.path().resource_dir().ok())
                    .map(|dir| dir.join("bridge").join("host-bridge.mjs"))
                    .filter(|path| path.exists())
            })
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("bridge").join("host-bridge.mjs")
            });
        let home = env_path("DSH_HOME").unwrap_or_else(|| home_dir().join(".dsh"));
        let node = env_path("DSH_TAURI_NODE")
            .or_else(|| {
                let bundled = resources.join("runtime").join("x64").join("primary-runtime")
                    .join("dependencies").join("node").join("bin").join("node");
                bundled.exists().then_some(bundled)
            })
            .unwrap_or_else(|| PathBuf::from("node"));
        Ok(Self {
            node,
            bridge,
            profile: env_path("DSH_TAURI_PROFILE").unwrap_or_else(|| home.join("profiles").join("desktop")),
            runtime: env_path("DSH_TAURI_PRIMARY_RUNTIME")
                .unwrap_or_else(|| resources.join("runtime").join("x64").join("primary-runtime")),
            executable: env_path("DSH_TAURI_EXECUTABLE")
                .or_else(|| resources.join("..").join("dsh-desktop-next").canonicalize().ok())
                .unwrap_or_else(|| PathBuf::from("dsh-desktop-next")),
            home,
            shim: env_path("DSH_TAURI_SHIM"),
            root,
        })
    }
}

/// Spawn the bridge and stream its events on a background thread.
pub fn launch(app: AppHandle) {
    std::thread::Builder::new()
        .name("dsh-bridge".to_owned())
        .spawn(move || {
            if let Err(error) = run(&app) {
                eprintln!("[dsh-tauri] bridge failed: {error}");
            }
        })
        .expect("spawning the bridge thread");
}

fn run(app: &AppHandle) -> Result<(), String> {
    let config = Config::resolve(Some(app))?;
    let mut command = Command::new(&config.node);
    command
        .arg(&config.bridge)
        .arg("--root")
        .arg(&config.root)
        .arg("--profile")
        .arg(&config.profile)
        .arg("--runtime")
        .arg(&config.runtime)
        .arg("--executable")
        .arg(&config.executable)
        .arg("--home")
        .arg(&config.home);
    if let Some(shim) = &config.shim {
        command.arg("--shim").arg(shim);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", config.node.display()))?;

    let stdin = child.stdin.take().ok_or("bridge stdin is unavailable")?;
    let stdout = child.stdout.take().ok_or("bridge stdout is unavailable")?;
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("[bridge] {line}");
            }
        });
    }

    let _ = SESSION.set(Mutex::new(Some(Session { child, stdin })));

    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("cannot read bridge output: {error}"))?;
        let event: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("[dsh-tauri] ignoring malformed bridge line ({error}): {line}");
                continue;
            }
        };
        match event.get("type").and_then(Value::as_str) {
            Some("ready") => open_window(app, &event)?,
            // Host events carry credentials (platform tokens) and bulky payloads:
            // log the kind only, never the body.
            Some("host") => eprintln!("[dsh-tauri] host event: {}", host_event_kind(&event)),
            Some("error") => eprintln!("[dsh-tauri] bridge: {}", event.get("message").and_then(Value::as_str).unwrap_or("unknown error")),
            Some("exit") => break,
            _ => {}
        }
    }
    Ok(())
}

fn open_window(app: &AppHandle, ready: &Value) -> Result<(), String> {
    let url = ready.get("url").and_then(Value::as_str).ok_or("ready event carries no url")?;
    let parsed = Url::parse(url).map_err(|error| format!("invalid bridge url {url}: {error}"))?;
    let handle = app.clone();
    let url_for_log = url.to_owned();
    app.run_on_main_thread(move || {
        if let Some(existing) = handle.get_webview_window("main") {
            let _ = existing.navigate(parsed);
            return;
        }
        match crate::shell_ui::build_main_window(&handle, parsed) {
            Ok(_) => eprintln!("[dsh-tauri] window opened at {url_for_log}"),
            Err(error) => eprintln!("[dsh-tauri] {error}")
        }
    })
    .map_err(|error| format!("cannot reach the main thread: {error}"))
}

/// Event kind of a Host message, for logging without leaking its payload.
fn host_event_kind(event: &Value) -> String {
    let kind = event
        .get("message")
        .and_then(|message| message.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("{kind} (payload redacted)")
}

/// Forward a message to the Host through the bridge's stdin.
///
/// Reserved for the native-surface bridge (permission answers, settings, menus);
/// the renderer currently reaches the Host over its own WebSocket instead.
#[allow(dead_code)]
pub fn send(message: &Value) -> Result<(), String> {
    let session = SESSION.get().ok_or("bridge is not running")?;
    let mut guard = session.lock().map_err(|_| "bridge session lock is poisoned")?;
    let session = guard.as_mut().ok_or("bridge has exited")?;
    let line = serde_json::json!({ "type": "host", "message": message }).to_string();
    session
        .stdin
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|()| session.stdin.flush())
        .map_err(|error| format!("cannot write to the bridge: {error}"))
}

/// Stop the bridge (and therefore the Host) if it is still running.
pub fn shutdown() {
    let Some(lock) = SESSION.get() else { return };
    let Ok(mut guard) = lock.lock() else { return };
    let Some(mut session) = guard.take() else { return };
    let _ = session.stdin.write_all(b"{\"type\":\"shutdown\"}\n");
    let _ = session.stdin.flush();
    std::thread::sleep(std::time::Duration::from_millis(400));
    let _ = session.child.kill();
    let _ = session.child.wait();
}
