//! Supervise the DSH web server: spawn it, publish its URL, restart it after a
//! crash and stop it on shutdown.
//!
//! The runtime itself is located (and installed) by `runtime::ensure`; this
//! module only owns the child process and the window hand-off.

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::AppHandle;

use crate::banner::web_url;
use crate::dsh::Config;
use crate::progress::{self, Stage};

/// How many times a crashed server is restarted before the error window shows.
const MAX_RESTARTS: u32 = 3;

struct Session {
    child: Child
}

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

fn session() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

/// Start the server and keep it supervised on a background thread.
pub fn launch(app: AppHandle) {
    std::thread::Builder::new()
        .name("dsh-supervisor".to_owned())
        .spawn(move || {
            if let Err(error) = run(&app) {
                eprintln!("[dsh-tauri] {error}");
                progress::fail(error.clone());
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
    progress::set(Stage::Locating, "正在查找 DSH 运行时");
    let config = Config::resolve(Some(app))?;
    eprintln!("[dsh-tauri] runtime {} (profile {})", config.entry.display(), config.profile);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        progress::set(Stage::Starting, format!("正在启动 DSH 服务（第 {attempt} 次尝试）"));
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
    let mut child = spawn_server(config)?;
    let stdout = child.stdout.take().ok_or("DSH stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("DSH stderr is unavailable")?;
    let pid = child.id();
    *session().lock().unwrap() = Some(Session { child });
    forward_stderr(stderr);

    let mut announced = false;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if !announced {
            if let Some(url) = web_url(&line) {
                announced = true;
                eprintln!("[dsh-tauri] serving {url}");
                progress::set(Stage::Ready, "服务已启动，正在打开界面");
                crate::shell_ui::show_url(app, &url)?;
                // The UI is up: look for a newer release in the background. It
                // never touches this process, so the new version applies on the
                // next launch.
                crate::update::spawn_once(config.runtime.clone(), config.source);
                continue;
            }
        }
        if !line.trim().is_empty() {
            eprintln!("[dsh] {line}");
        }
    }
    Ok(Some(exit_reason(pid)))
}

/// Build and spawn `node <entry> --profile … --port 0`, detached from our stdin.
fn spawn_server(config: &Config) -> Result<Child, String> {
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
        command
            .pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })
            .spawn()
    };
    spawn.map_err(|error| {
        format!(
            "cannot start `{} {}`: {error}\n\n安装 Node 24+（或在 DSH_TAURI_NODE 指定路径），然后重试。",
            config.node.display(),
            config.entry.display()
        )
    })
}

/// Server logs go to the terminal; the UI is what matters to the user.
fn forward_stderr(stderr: std::process::ChildStderr) {
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("[dsh] {line}");
        }
    });
}

/// Describe how the server we started ended.
fn exit_reason(pid: u32) -> String {
    let status = session().lock().unwrap().take().map(|mut session| session.child.wait());
    match status {
        Some(Ok(status)) => match status.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by a signal".to_owned()
        },
        _ => format!("process {pid} ended")
    }
}

/// Ask the server to stop, then forget the handle. Called from the tray/menu.
pub fn shutdown() {
    SHUTTING_DOWN.store(true, Ordering::Relaxed);
    // An install or update may be running npm: stop it instead of leaving it
    // behind with a half-written staging directory.
    crate::exec::kill_running();
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
