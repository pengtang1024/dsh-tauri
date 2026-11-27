//! Supervise the DSH web server: spawn it, publish its URL, restart it after a
//! crash and stop it on shutdown.
//!
//! The runtime itself is located (and installed) by `runtime::ensure`; this
//! module only owns the child process and the window hand-off.

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;

use crate::banner::web_url;
use crate::dsh::Config;
use crate::progress::{self, Stage};

/// How many times a crashed server is restarted before the error window shows.
const MAX_RESTARTS: u32 = 3;
/// How long the shell waits for the server's first URL before giving up.
/// A live-but-silent server must not leave the splash on "Starting" forever.
const WEB_URL_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a server must stay healthy after publishing its URL before an
/// isolated later crash stops counting toward the storm limit.
const RESTART_RESET_AFTER: Duration = Duration::from_secs(60);

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
/// Timestamp (epoch millis) of the last URL successfully published to the UI.
/// A crash after a long healthy run is isolated, not part of a restart storm.
static LAST_READY_MS: AtomicU64 = AtomicU64::new(0);

fn session() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

/// Take a mutex guard even when a previous holder panicked while holding it.
///
/// `progress::lock()` already recovers this way; the supervisor must not turn
/// an unrelated thread's panic into a second panic when it only needs to read
/// or clear the session.
fn recover_lock<T>(
    result: std::sync::LockResult<std::sync::MutexGuard<'_, T>>
) -> std::sync::MutexGuard<'_, T> {
    result.unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Claim the one-shot shutdown gate: `true` for the first caller, `false` for
/// every later one. Pure over the supplied atomic so the idempotence guarantee
/// is testable without spawning a server.
fn claim_shutdown(done: &AtomicBool) -> bool {
    !done.swap(true, Ordering::SeqCst)
}

/// Pure error-page text for a supervisor thread that could not be spawned.
/// Kept separate from `launch` so the wording is testable without exhausting
/// the OS thread limit.
fn spawn_failed_message(error: &std::io::Error) -> String {
    format!("无法启动服务监控线程：{error}")
}

/// Message the stdout reader thread sends back to the supervisor.
enum UrlEvent {
    /// The first loopback URL printed by the server.
    Url(String),
    /// stdout reached EOF (or the pipe broke) before a URL was seen.
    Eof,
}

/// Outcome of waiting for the server's first URL.
enum UrlWait {
    Url(String),
    /// stdout ended / channel disconnected: treat as a normal server exit.
    Ended,
    /// No URL within [`WEB_URL_TIMEOUT`]: the server is live but silent.
    TimedOut,
}

/// Classify a `recv_timeout` result so the decision logic is testable without
/// spawning a real server or waiting a minute.
fn classify_url_wait(result: Result<UrlEvent, RecvTimeoutError>) -> UrlWait {
    match result {
        Ok(UrlEvent::Url(url)) => UrlWait::Url(url),
        Ok(UrlEvent::Eof) | Err(RecvTimeoutError::Disconnected) => UrlWait::Ended,
        Err(RecvTimeoutError::Timeout) => UrlWait::TimedOut,
    }
}

/// Error-page text for a silent server. The timeout value comes from the same
/// constant as the wait itself, and no URL is echoed back.
fn startup_timeout_message() -> String {
    format!(
        "DSH 启动超时（{} 秒）：服务已启动但未在规定时间内提供访问地址。\n\n请在终端手动运行 `dsh web` 查看日志。",
        WEB_URL_TIMEOUT.as_secs()
    )
}

/// A timeout during shutdown is expected: the tray already owns the teardown
/// and must not be followed by an error page.
fn should_report_timeout(shutting_down: bool) -> bool {
    !shutting_down
}

/// Milliseconds since the UNIX epoch, used for the restart-stability window.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Record a successful URL publication as the start of the stability window.
/// An epoch clock that went backwards yields 0, which disables the reset (a
/// crash before any readiness is still a genuine restart-storm candidate).
fn mark_ready(last_ready_ms: &AtomicU64, now: u64) {
    if now > 0 {
        last_ready_ms.store(now, Ordering::SeqCst);
    }
}

/// Pure stability-window decision. Returns the new attempt count and whether
/// the ready marker was consumed. The marker MUST be cleared by the caller on
/// `true`: otherwise a storm following a previously healthy run would reset on
/// every crash against the same stale timestamp and never reach the error page.
/// `None` (never ready / already consumed) or a backwards clock never resets.
/// The threshold is inclusive: exactly 60s counts as stable.
fn restart_decision(attempt: u32, last_ready_ms: Option<u64>, now: u64) -> (u32, bool) {
    if attempt == 0 {
        return (0, false);
    }
    let stable = last_ready_ms
        .filter(|ready| now.saturating_sub(*ready) >= RESTART_RESET_AFTER.as_millis() as u64)
        .is_some();
    if stable { (0, true) } else { (attempt, false) }
}

/// Start the server and keep it supervised on a background thread.
pub fn launch(app: AppHandle) {
    // The supervisor closure takes `app`; keep a clone for the spawn-failure
    // path, where no such closure will ever run.
    let error_app = app.clone();
    let spawned = std::thread::Builder::new()
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
        });
    match spawned {
        Ok(_) => {}
        Err(error) => {
            // No supervisor thread exists, so no closure can report this one.
            // Take the same error-page path here instead of panicking: a panic
            // after single-instance bound its socket leaves a stale socket and
            // every later launch exits silently with no usable instance.
            eprintln!("[dsh-tauri] spawning the supervisor thread failed: {error}");
            let message = spawn_failed_message(&error);
            progress::fail(message.clone());
            let handle = error_app.clone();
            let _ = error_app.run_on_main_thread(move || {
                if let Err(error) = crate::shell_ui::show_error(&handle, &message) {
                    eprintln!("[dsh-tauri] cannot show the error window: {error}");
                }
            });
        }
    }
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
                // A server that ran healthy for a full stability window earns
                // a fresh restart budget; only tight crash storms hit the cap.
                // Consume the marker on reset so a storm that follows a long
                // healthy run cannot keep resetting against the stale timestamp.
                let ready = LAST_READY_MS.load(Ordering::SeqCst);
                let (next, consumed) = restart_decision(
                    attempt, (ready > 0).then_some(ready), now_millis()
                );
                attempt = next;
                if consumed {
                    LAST_READY_MS.store(0, Ordering::SeqCst);
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

/// Stop a server that was spawned but cannot be supervised (a pipe is missing).
/// `drop(Child)` does not kill the process on Unix, so the explicit kill/wait is
/// what keeps this startup failure from leaving the DSH service as an orphan.
fn stop_failed_server(child: &mut Child, error: &str) -> String {
    let _ = child.kill();
    let _ = child.wait();
    error.to_owned()
}

/// One server lifetime. `None` means the shell asked it to stop.
fn run_once(app: &AppHandle, config: &Config) -> Result<Option<String>, String> {
    let mut child = spawn_server(config)?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (stdout, stderr) = match (stdout, stderr) {
        (Some(stdout), Some(stderr)) => (stdout, stderr),
        (None, _) => return Err(stop_failed_server(&mut child, "DSH stdout is unavailable")),
        (_, None) => return Err(stop_failed_server(&mut child, "DSH stderr is unavailable")),
    };
    let rx = match spawn_url_reader(stdout) {
        Ok(rx) => rx,
        Err(error) => {
            return Err(stop_failed_server(
                &mut child,
                &format!("cannot start stdout reader: {error}")
            ))
        }
    };
    let pid = child.id();
    *recover_lock(session().lock()) = Some(Session { child });
    forward_stderr(stderr);

    // Watchdog: a live server that never prints its URL used to block the
    // supervisor forever in BufReader::lines(), leaving the splash on Starting.
    // Only the first URL is time-boxed; once announced the UI is up and the
    // server may run for as long as it likes.
    let url = match classify_url_wait(rx.recv_timeout(WEB_URL_TIMEOUT)) {
        UrlWait::Url(url) => url,
        UrlWait::Ended => return Ok(Some(exit_reason(pid))),
        UrlWait::TimedOut => {
            // During a user-initiated exit the tray already tore the server
            // down: stay silent, no error page.
            if !should_report_timeout(SHUTTING_DOWN.load(Ordering::Relaxed)) {
                return Ok(None);
            }
            // Kill through the handle we own, never a bare pid that could have
            // been recycled by the time we signal it.
            {
                let mut guard = recover_lock(session().lock());
                if let Some(session) = guard.as_mut() {
                    let _ = session.child.kill();
                    let _ = session.child.wait();
                }
                *guard = None;
            }
            let message = startup_timeout_message();
            progress::fail(message.clone());
            // No retry: a silent server would only repeat the same 60s hang.
            return Err(message);
        }
    };
    eprintln!("[dsh-tauri] serving {url}");
    progress::set(Stage::Ready, "服务已启动，正在打开界面");
    crate::shell_ui::show_url(app, &url)?;
    // The UI is up: start the stability window. A later isolated crash must
    // not inherit restart attempts from crashes hours earlier.
    mark_ready(&LAST_READY_MS, now_millis());
    // The UI is up: look for a newer release in the background. It never
    // touches this process, so the new version applies on the next launch.
    crate::update::spawn_once(config.runtime.clone(), config.source);

    // Supervise until stdout ends. The reader thread sends Eof only when no
    // URL was seen; after a URL it just drops the sender, which also wakes us.
    let _ = rx.recv();
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

/// Read stdout on a dedicated thread so a live-but-silent server cannot block
/// the supervisor forever. The first loopback URL (or EOF, if none) is sent
/// back over the channel; every other line is logged exactly as before.
fn spawn_url_reader(
    stdout: std::process::ChildStdout
) -> Result<mpsc::Receiver<UrlEvent>, std::io::Error> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dsh-stdout-reader".to_owned())
        .spawn(move || {
            let mut sent_url = false;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if !sent_url {
                    if let Some(url) = web_url(&line) {
                        sent_url = true;
                        if tx.send(UrlEvent::Url(url)).is_err() {
                            return;
                        }
                        continue;
                    }
                }
                if !line.trim().is_empty() {
                    eprintln!("[dsh] {line}");
                }
            }
            if !sent_url {
                let _ = tx.send(UrlEvent::Eof);
            }
        })?;
    Ok(rx)
}

/// Pure fallback text when the session was already taken (e.g. by `shutdown`).
/// `{pid}` is expanded by `exit_reason`; during shutdown the reason is explicit
/// instead of looking like an unexpected exit.
fn missing_session_reason(shutting_down: bool) -> &'static str {
    if shutting_down {
        "服务已按关闭流程停止（会话已被 shutdown 取走）"
    } else {
        "process {pid} ended"
    }
}

/// Describe how the server we started ended.
fn exit_reason(pid: u32) -> String {
    let status = recover_lock(session().lock()).take().map(|mut session| session.child.wait());
    match status {
        Some(Ok(status)) => match status.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by a signal".to_owned()
        },
        _ => missing_session_reason(SHUTTING_DOWN.load(Ordering::Relaxed))
            .replace("{pid}", &pid.to_string())
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
    // behind with a half-written staging directory. The flag set by
    // begin_shutdown() also blocks any helper a background updater tries to
    // spawn from here on, so no replacement npm can slip past this snapshot.
    crate::exec::begin_shutdown();
    let mut guard = recover_lock(session().lock());
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
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::Arc;

    use super::{
        claim_shutdown, classify_url_wait, mark_ready, missing_session_reason, recover_lock,
        restart_decision, should_report_timeout, spawn_failed_message, startup_timeout_message,
        term_result_ok, UrlEvent, UrlWait, RESTART_RESET_AFTER, WEB_URL_TIMEOUT,
    };

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
    fn recover_lock_survives_a_thread_panicking_while_holding_the_mutex() {
        // A fresh mutex keeps the poisoning local to this test: the global
        // SESSION is shared with production code and must stay unpoisoned.
        let mutex = std::sync::Arc::new(std::sync::Mutex::new(42u32));
        let poisoned_by = std::sync::Arc::clone(&mutex);
        let holder = std::thread::spawn(move || {
            let _guard = poisoned_by.lock().expect("fresh mutex is not poisoned yet");
            panic!("poison the mutex while holding its guard");
        });
        assert!(holder.join().is_err(), "holder thread must panic");
        assert!(mutex.is_poisoned(), "mutex must be poisoned before recovery");

        // `.unwrap()` would panic here; recover_lock must yield the guard and
        // the data the panicking thread left behind.
        let guard = recover_lock(mutex.lock());
        assert_eq!(*guard, 42, "guarded data survived the panic");
    }

    #[test]
    fn term_result_ok_on_success() {
        assert!(term_result_ok(0));
    }

    #[test]
    fn term_result_not_ok_on_failure() {
        assert!(!term_result_ok(-1));
    }

    #[test]
    fn missing_session_reason_during_shutdown_is_explicit() {
        let reason = missing_session_reason(true);
        assert!(reason.contains("关闭"), "unexpected reason: {reason}");
        assert!(!reason.contains("{pid}"), "placeholder must not leak: {reason}");
    }

    #[test]
    fn missing_session_reason_outside_shutdown_keeps_original_text() {
        assert_eq!(missing_session_reason(false), "process {pid} ended");
        assert_eq!(missing_session_reason(false).replace("{pid}", "42"), "process 42 ended");
    }

    #[test]
    fn spawn_failed_message_keeps_prefix_and_original_error() {
        let error = std::io::Error::other("resource temporarily unavailable");
        let message = spawn_failed_message(&error);
        assert!(message.starts_with("无法启动服务监控线程"), "unexpected message: {message}");
        assert!(message.contains("resource temporarily unavailable"), "error lost: {message}");
    }

    #[test]
    fn startup_timeout_message_has_seconds_and_manual_command_without_url() {
        let message = startup_timeout_message();
        assert!(
            message.contains("60 秒"),
            "timeout seconds missing or drifted: {message}"
        );
        assert_eq!(WEB_URL_TIMEOUT.as_secs(), 60, "constant must match message");
        assert!(message.contains("dsh web"), "manual command missing: {message}");
        assert!(
            !message.contains("http://") && !message.contains("https://"),
            "error page must not echo the full URL: {message}"
        );
    }

    #[test]
    fn classify_url_wait_distinguishes_url_end_and_timeout() {
        assert!(matches!(
            classify_url_wait(Ok(UrlEvent::Url("http://127.0.0.1:1234".to_owned()))),
            UrlWait::Url(url) if url == "http://127.0.0.1:1234"
        ));
        assert!(matches!(
            classify_url_wait(Ok(UrlEvent::Eof)),
            UrlWait::Ended
        ));
        assert!(matches!(
            classify_url_wait(Err(RecvTimeoutError::Disconnected)),
            UrlWait::Ended
        ));
        assert!(matches!(
            classify_url_wait(Err(RecvTimeoutError::Timeout)),
            UrlWait::TimedOut
        ));
    }

    #[test]
    fn should_report_timeout_is_false_while_shutting_down() {
        assert!(!should_report_timeout(true), "no error page during shutdown");
        assert!(should_report_timeout(false), "report a real startup timeout");
    }

    #[test]
    fn restart_attempt_does_not_reset_before_the_stability_window() {
        let ready = 1_000_000u64;
        let window = RESTART_RESET_AFTER.as_millis() as u64;
        assert_eq!(restart_decision(2, Some(ready), ready + window - 1), (2, false));
        assert_eq!(restart_decision(2, Some(ready), ready), (2, false));
    }

    #[test]
    fn restart_attempt_resets_at_the_stability_window_boundary() {
        let ready = 1_000_000u64;
        let window = RESTART_RESET_AFTER.as_millis() as u64;
        assert_eq!(restart_decision(2, Some(ready), ready + window), (0, true));
        assert_eq!(restart_decision(2, Some(ready), ready + 3 * window), (0, true));
    }

    #[test]
    fn consecutive_crashes_still_reach_the_restart_limit() {
        // No ready publication, or a crash inside the window: the storm guard
        // must stay intact so three quick crashes still show the error page.
        assert_eq!(restart_decision(3, None, 10_000), (3, false));
        assert_eq!(restart_decision(3, Some(9_000), 10_000), (3, false));
        // Zero attempts can never be reset into a negative budget.
        assert_eq!(restart_decision(0, Some(0), 10_000), (0, false));
    }

    #[test]
    fn a_storm_after_a_healthy_run_cannot_reset_against_a_consumed_marker() {
        // The ready marker must be cleared after the first reset; otherwise
        // crashes 2 and 3 would reset again and never reach the error page.
        let ready = 1_000_000u64;
        let window = RESTART_RESET_AFTER.as_millis() as u64;
        let (attempt, consumed) = restart_decision(2, Some(ready), ready + window);
        assert_eq!((attempt, consumed), (0, true));
        let marker = std::sync::atomic::AtomicU64::new(if consumed { 0 } else { ready });
        // Mirror run(): a zero / cleared marker is passed as None, never Some(0).
        let stored = marker.load(Ordering::SeqCst);
        let ready_arg = (stored > 0).then_some(stored);
        assert_eq!(restart_decision(1, ready_arg, ready + window), (1, false));
        assert_eq!(restart_decision(2, None, ready + 2 * window), (2, false));
    }

    #[test]
    fn mark_ready_records_a_monotonic_timestamp() {
        let clock = std::sync::atomic::AtomicU64::new(0);
        mark_ready(&clock, 0);
        assert_eq!(clock.load(Ordering::SeqCst), 0, "epoch 0 means never ready");
        mark_ready(&clock, 5_000);
        assert_eq!(clock.load(Ordering::SeqCst), 5_000);
    }
}
