//! Owns the DSH web server: spawns it from the npm runtime this app ships,
//! watches its output for the URL, and keeps it alive.
//!
//! There is no Electron and no DSH Desktop installation here: `@deepseek-ai/dsh`
//! is a plain Node package, so the shell only needs a Node executable and that
//! package's directory.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::json;
use tauri::{AppHandle, Manager, Url};

/// How many times a crashed server is restarted before the error window shows.
const MAX_RESTARTS: u32 = 3;

struct Session {
    child: Child,
}

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

fn session() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub node: PathBuf,
    pub entry: PathBuf,
    pub home: PathBuf,
    pub profile: String,
    pub host: String,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
}

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

impl Config {
    /// Locate the runtime this app should drive.
    ///
    /// Order: explicit overrides, the checkout's `runtime/` (development), then
    /// the resource directory (packaged). Anything missing is reported with the
    /// exact path so the error window can say what to install.
    pub fn resolve(app: Option<&AppHandle>) -> Result<Self, String> {
        let runtime = env_path("DSH_TAURI_RUNTIME")
            .or_else(|| {
                let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("runtime");
                checkout.join("node_modules").is_dir().then_some(checkout)
            })
            .or_else(|| {
                // A runtime installed next to the user's DSH home, so a packaged
                // build can use one without shipping 1 GB of node_modules.
                let installed = home_dir().join(".dsh").join("dsh-tauri").join("runtime");
                installed.join("node_modules").is_dir().then_some(installed)
            })
            .or_else(|| {
                app.and_then(|handle| handle.path().resource_dir().ok())
                    .map(|dir| dir.join("runtime"))
                    .filter(|path| path.join("node_modules").is_dir())
            })
            .ok_or_else(|| {
                "no DSH runtime found: run `npm install` in runtime/ (development) or set DSH_TAURI_RUNTIME"
                    .to_owned()
            })?;

        let package = runtime.join("node_modules").join("@deepseek-ai").join("dsh").join("lib").join("bin.js");
        if !package.is_file() {
            return Err(format!(
                "{} is missing: install @deepseek-ai/dsh into {} (`npm install @deepseek-ai/dsh`)",
                package.display(),
                runtime.display()
            ));
        }

        let node = env_path("DSH_TAURI_NODE").unwrap_or_else(|| PathBuf::from("node"));
        Ok(Self {
            node,
            entry: package,
            home: env_path("DSH_HOME").unwrap_or_else(|| home_dir().join(".dsh")),
            profile: std::env::var("DSH_TAURI_PROFILE").unwrap_or_else(|_| "web".to_owned()),
            host: std::env::var("DSH_TAURI_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned())
        })
    }
}

/// Start the server and keep it supervised on a background thread.
pub fn launch(app: AppHandle) {
    std::thread::Builder::new()
        .name("dsh-supervisor".to_owned())
        .spawn(move || {
            if let Err(error) = run(&app) {
                eprintln!("[dsh-tauri] {error}");
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Err(error) = crate::shell_ui::show_error(&handle, &error) {
                        eprintln!("[dsh-tauri] cannot show the error window: {error}");
                    }
                });
            }
        })
        .expect("spawning the supervisor thread");
}

fn run(app: &AppHandle) -> Result<(), String> {
    let config = Config::resolve(Some(app))?;
    eprintln!("[dsh-tauri] runtime {} (profile {})", config.entry.display(), config.profile);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match run_once(app, &config)? {
            None => return Ok(()),
            Some(reason) => {
                if SHUTTING_DOWN.load(Ordering::Relaxed) {
                    return Ok(());
                }
                eprintln!("[dsh-tauri] DSH exited unexpectedly ({reason}), restart {attempt}/{MAX_RESTARTS}");
                if attempt >= MAX_RESTARTS {
                    return Err(format!(
                        "DSH 服务反复退出：{reason}\n\n请重试；如果仍然失败，在终端里手动运行 `dsh web` 查看完整日志。"
                    ));
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }
}

/// One server lifetime. `None` means the shell asked it to stop.
fn run_once(app: &AppHandle, config: &Config) -> Result<Option<String>, String> {
    let mut command = Command::new(&config.node);
    command
        .arg(&config.entry)
        .arg("--profile")
        .arg(&config.profile)
        .arg("--host")
        .arg(&config.host)
        .arg("--port")
        .arg("0")
        .arg("--no-open")
        .env("DSH_HOME", &config.home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // If the shell dies (crash, logout, SIGKILL) the server must follow it
    // instead of staying behind holding the port and the profile.
    // SAFETY: between fork and exec only async-signal-safe calls are made, and
    // `prctl` passes an integer flag and constant signal number.
    let spawn = unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        })
        .spawn()
    };
    let mut child = spawn.map_err(|error| {
        format!(
            "cannot start `{} {}`: {error}\n\n安装 Node 24+（或在 DSH_TAURI_NODE 指定路径），然后重试。",
            config.node.display(),
            config.entry.display()
        )
    })?;

    let stdout = child.stdout.take().ok_or("DSH stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("DSH stderr is unavailable")?;
    let pid = child.id();
    *session().lock().unwrap() = Some(Session { child });

    // Server logs go to the terminal; the UI is what matters to the user.
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("[dsh] {line}");
        }
    });

    let mut announced = false;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if !announced {
            if let Some(url) = web_url(&line) {
                announced = true;
                eprintln!("[dsh-tauri] serving {url}");
                publish_url(app, &url)?;
                continue;
            }
        }
        if !line.trim().is_empty() {
            eprintln!("[dsh] {line}");
        }
    }

    let status = session()
        .lock()
        .unwrap()
        .take()
        .map(|mut session| session.child.wait());
    Ok(Some(match status {
        Some(Ok(status)) => match status.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by a signal".to_owned()
        },
        _ => format!("process {pid} ended")
    }))
}

/// Pull the URL out of `dsh web: http://127.0.0.1:PORT/?token=…`.
fn web_url(line: &str) -> Option<String> {
    let start = line.find("http://").or_else(|| line.find("https://"))?;
    let rest = &line[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let url = &rest[..end];
    url.parse::<Url>().ok().map(|_| url.to_owned())
}

/// Open the window on the first URL, navigate it afterwards (restarts).
fn publish_url(app: &AppHandle, url: &str) -> Result<(), String> {
    let parsed: Url = url.parse().map_err(|error| format!("bad URL from DSH: {error}"))?;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.navigate(parsed);
            let _ = window.set_focus();
            return;
        }
        match crate::shell_ui::build_main_window(&handle, parsed) {
            Ok(_) => {}
            Err(error) => eprintln!("[dsh-tauri] {error}")
        }
    })
    .map_err(|error| format!("cannot reach the main thread: {error}"))
}

/// Ask the server to stop, then forget the handle. Called from the tray/menu.
pub fn shutdown() {
    SHUTTING_DOWN.store(true, Ordering::Relaxed);
    let mut guard = session().lock().unwrap();
    if let Some(session) = guard.as_mut() {
        let pid = session.child.id();
        // SIGTERM lets the server flush sessions; SIGKILL would leave WAL debris.
        let _ = Command::new("kill").arg("-TERM").arg(pid.to_string()).status();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Ok(Some(_)) = session.child.try_wait() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = session.child.kill();
    }
    *guard = None;
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

#[cfg(test)]
mod tests {
    use super::web_url;

    #[test]
    fn reads_the_url_from_the_runtime_banner() {
        let line = "dsh web: http://127.0.0.1:44123/?token=abc-DEF_123";
        assert_eq!(web_url(line).as_deref(), Some("http://127.0.0.1:44123/?token=abc-DEF_123"));
    }

    #[test]
    fn ignores_lines_without_a_url() {
        assert_eq!(web_url("[dsh] loading plugins"), None);
        assert_eq!(web_url("http://"), None);
    }
}
