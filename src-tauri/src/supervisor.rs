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
/// Suppresses restarts in the supervisor loop after a shell-initiated stop.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
/// One-shot gate: the tray exit handler calls `shutdown()` before `app.exit(0)`,
/// and the resulting `RunEvent::Exit` calls it again. Only the first caller may
/// tear down the server.
static SHUTDOWN_DONE: AtomicBool = AtomicBool::new(false);

fn session() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

/// Claim the one-shot shutdown gate: `true` for the first caller, `false` for
/// every later one. Pure over the supplied atomic so the idempotence guarantee
/// is testable without spawning a server.
fn claim_shutdown(done: &AtomicBool) -> bool {
    !done.swap(true, Ordering::SeqCst)
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

/// `libc::kill` returns 0 on success and -1 (with errno set) on failure.
fn term_result_ok(rc: i32) -> bool {
    rc == 0
}

/// Ask the server to stop, then forget the handle. Called from the tray/menu.
pub fn shutdown() {
    // `app.exit(0)` from the tray fires `RunEvent::Exit`, so the same exit can
    // reach this function twice. Repeating the teardown could SIGTERM a pid that
    // was already reaped and reused; the gate makes the whole operation one-shot.
    if !claim_shutdown(&SHUTDOWN_DONE) {
        return;
    }
    SHUTTING_DOWN.store(true, Ordering::Relaxed);
    // An install or update may be running npm: stop it instead of leaving it
    // behind with a half-written staging directory.
    crate::exec::kill_running();
    let mut guard = session().lock().unwrap();
    if let Some(session) = guard.as_mut() {
        let pid = session.child.id();
        // SIGTERM lets the server flush sessions; SIGKILL would leave WAL debris.
        // SAFETY: `pid` comes from the `Child` we spawned ourselves; sending a
        // signal to that process has no memory-safety implications. The only
        // failure mode is a stale/unreachable pid, reported via errno below.
        let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        if !term_result_ok(result) {
            eprintln!(
                "[dsh-tauri] cannot SIGTERM server {pid}: {}",
                std::io::Error::last_os_error()
            );
        }
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::{claim_shutdown, term_result_ok};

    #[test]
    fn claim_shutdown_is_idempotent() {
        let done = AtomicBool::new(false);
        assert!(claim_shutdown(&done));
        assert!(!claim_shutdown(&done));
        assert!(!claim_shutdown(&done));
    }

    #[test]
    fn claim_shutdown_has_exactly_one_winner_across_threads() {
        let done = Arc::new(AtomicBool::new(false));
        let winners = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let done = Arc::clone(&done);
                let winners = Arc::clone(&winners);
                std::thread::spawn(move || {
                    if claim_shutdown(&done) {
                        winners.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("claim thread panicked");
        }
        assert_eq!(winners.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn term_result_ok_on_success() {
        assert!(term_result_ok(0));
    }

    #[test]
    fn term_result_not_ok_on_failure() {
        assert!(!term_result_ok(-1));
    }
}
