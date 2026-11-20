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
/// Origins learned from the Host's `ready` event: the loopback proxy the window
/// loads, and the Host origin that Platform sign-in callbacks belong to.
static PROXY_ORIGIN: Mutex<Option<String>> = Mutex::new(None);
static HOST_ORIGIN: Mutex<Option<String>> = Mutex::new(None);

fn remember_origins(proxy: &str, host: &str) {
    if let Ok(mut guard) = PROXY_ORIGIN.lock() {
        *guard = Some(proxy.to_owned());
    }
    if let Ok(mut guard) = HOST_ORIGIN.lock() {
        *guard = Some(host.to_owned());
    }
}

/// Origin of the Host, once the bridge has reported it.
pub fn host_origin() -> Option<String> {
    HOST_ORIGIN.lock().ok()?.clone()
}

/// Send a command that is not meant for the Host IPC channel.
fn send_command(message: &Value) -> Result<(), String> {
    let session = SESSION.get().ok_or("bridge is not running")?;
    let mut guard = session.lock().map_err(|_| "bridge session lock is poisoned")?;
    let session = guard.as_mut().ok_or("bridge has exited")?;
    let line = message.to_string();
    session
        .stdin
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|()| session.stdin.flush())
        .map_err(|error| format!("cannot write to the bridge: {error}"))
}

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
        // A checkout wins over bundled resources so `cargo run` follows the
        // working tree; packaged installs only ship the resource copy.
        let bridge = env_path("DSH_TAURI_BRIDGE")
            .or_else(|| {
                let checkout =
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("bridge").join("host-bridge.mjs");
                checkout.exists().then_some(checkout)
            })
            .or_else(|| {
                app.and_then(|handle| handle.path().resource_dir().ok())
                    .map(|dir| dir.join("bridge").join("host-bridge.mjs"))
                    .filter(|path| path.exists())
            })
            .ok_or_else(|| "cannot locate bridge/host-bridge.mjs; set DSH_TAURI_BRIDGE".to_owned())?;
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
                let handle = app.clone();
                let message = error.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Err(error) = crate::shell_ui::build_error_window(&handle, &message) {
                        eprintln!("[dsh-tauri] cannot show the error window: {error}");
                    }
                });
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
            Some("host") => {
                eprintln!("[dsh-tauri] host event: {}", host_event_kind(&event));
                handle_host_event(app, event.get("message"));
            }
            Some("platform-login-done") => {
                eprintln!(
                    "[dsh-tauri] platform sign-in callback: status {}",
                    event.get("status").and_then(Value::as_u64).unwrap_or(0)
                );
                close_login_window(app);
            }
            Some("error") => eprintln!("[dsh-tauri] bridge: {}", event.get("message").and_then(Value::as_str).unwrap_or("unknown error")),
            Some("title") => set_window_title(app, event.get("title").and_then(Value::as_str).unwrap_or("")),
            Some("exit") => break,
            _ => {}
        }
    }
    Ok(())
}

/// React to Host events that need shell furniture rather than the WebView.
fn handle_host_event(app: &AppHandle, message: Option<&Value>) {
    let Some(message) = message else { return };
    if message.get("type").and_then(Value::as_str) != Some("platform-login") {
        return;
    }
    match message.get("action").and_then(Value::as_str) {
        Some("open") => {
            let Some(url) = message.get("url").and_then(Value::as_str) else { return };
            let handle = app.clone();
            let url = url.to_owned();
            let _ = app.run_on_main_thread(move || {
                if let Err(error) = crate::shell_ui::build_login_window(&handle, &url) {
                    eprintln!("[dsh-tauri] cannot open the sign-in window: {error}");
                }
            });
        }
        Some("close") => close_login_window(app),
        _ => {}
    }
}

fn close_login_window(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("login") {
            let _ = window.close();
        }
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.set_focus();
        }
    });
}

/// Ask the bridge to replay a Platform sign-in callback through the proxy.
pub fn complete_platform_login(url: &str) {
    let command = serde_json::json!({ "type": "platform-callback", "url": url });
    if let Err(error) = send_command(&command) {
        eprintln!("[dsh-tauri] cannot complete the Platform sign-in: {error}");
    }
}

fn set_window_title(app: &AppHandle, title: &str) {
    if title.is_empty() {
        return;
    }
    let handle = app.clone();
    let title = title.to_owned();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.set_title(&title);
        }
    });
}

fn open_window(app: &AppHandle, ready: &Value) -> Result<(), String> {
    let url = ready.get("url").and_then(Value::as_str).ok_or("ready event carries no url")?;
    let parsed = Url::parse(url).map_err(|error| format!("invalid bridge url {url}: {error}"))?;
    let handle = app.clone();
    let url_for_log = url.to_owned();
    let host_url = ready.get("hostUrl").and_then(Value::as_str).unwrap_or("");
    let proxy_origin = parsed.origin().ascii_serialization();
    if let Ok(host) = Url::parse(host_url) {
        let host_origin = host.origin().ascii_serialization();
        remember_origins(&proxy_origin, &host_origin);
        eprintln!("[dsh-tauri] host origin {host_origin}");
    }
    // Development hook: open the sign-in window up front to exercise the OAuth
    // callback relay without waiting for the Host to ask for a sign-in.
    if let Ok(login) = std::env::var("DSH_TAURI_OPEN_LOGIN") {
        if !login.is_empty() {
            let handle = app.clone();
            let parsed = login;
            let _ = app.run_on_main_thread(move || {
                if let Err(error) = crate::shell_ui::build_login_window(&handle, &parsed) {
                    eprintln!("[dsh-tauri] cannot open the sign-in window: {error}");
                }
            });
        }
    }
    app.run_on_main_thread(move || {
        if let Some(existing) = handle.get_webview_window("main") {
            let _ = existing.navigate(parsed);
            return;
        }
        match crate::shell_ui::build_main_window(&handle, parsed) {
            Ok(window) => {
                eprintln!("[dsh-tauri] window opened at {url_for_log}");
                // Smoke hook: exercise the close path (tray vs. quit) headlessly.
                if std::env::var("DSH_TAURI_SMOKE_CLOSE").is_ok() {
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        eprintln!("[dsh-tauri] smoke: requesting window close");
                        if let Err(error) = window.close() {
                            eprintln!("[dsh-tauri] smoke: close failed: {error}");
                        }
                    });
                }
            }
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
