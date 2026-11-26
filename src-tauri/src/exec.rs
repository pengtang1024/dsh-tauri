//! Run a helper process with a deadline, capturing both streams.
//!
//! npm is the only heavy helper the shell drives: it must never hang the boot,
//! its output must reach the log verbatim, and a timeout or shell shutdown must
//! kill the whole process group — npm spawns lifecycle scripts of its own.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Group ids of the helpers currently running, so shutdown can stop every one.
static RUNNING: Mutex<Vec<i32>> = Mutex::new(Vec::new());

/// Once the shell starts shutting down no new helper may be spawned: a background
/// updater could otherwise start npm after `kill_running()` took its snapshot.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// A helper may spawn a background grandchild that keeps the pipe open after
/// the helper itself exited. Waiting for EOF without a deadline would hang the
/// shell, so output collection is bounded to this window.
const DRAIN_WAIT: Duration = Duration::from_millis(200);

/// Result of a finished (or killed) helper process.
pub struct Outcome {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool
}

impl Outcome {
    /// A helper succeeded only if it exited before the deadline with status 0.
    pub fn ok(&self) -> bool {
        !self.timed_out && self.status.success()
    }

    /// Last few non-empty stderr lines, for a readable error page.
    pub fn tail(&self, lines: usize) -> String {
        let mut kept: Vec<&str> = self.stderr.lines().filter(|line| !line.trim().is_empty()).collect();
        if kept.len() > lines {
            kept.drain(..kept.len() - lines);
        }
        kept.join("\n")
    }
}

/// UTF-8 容错转换：无效字节替换为 U+FFFD，有效文本原样保留。
///
/// 用字节流而非 read_to_string：后者遇到任一无效字节会返回 Err 并把
/// 整个 buffer 清空，导致诊断输出整段丢失。
fn lossy_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Drain a pipe on its own thread and report the captured text through a
/// channel. The reader may block forever if a grandchild inherits the pipe, so
/// the caller waits with a deadline (`recv_timeout`) instead of joining.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Option<Receiver<String>> {
    pipe.map(|mut pipe| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            // Ignore send failure: the caller may have already timed out.
            let _ = tx.send(lossy_output(&bytes));
        });
        rx
    })
}

/// Collect one drained stream within the bounded wait window. A timeout means
/// a grandchild still holds the pipe; log it and use the empty string rather
/// than blocking the shell indefinitely.
fn collect(receiver: Option<Receiver<String>>, stream: &str) -> String {
    match receiver {
        None => String::new(),
        Some(rx) => match rx.recv_timeout(DRAIN_WAIT) {
            Ok(text) => text,
            Err(RecvTimeoutError::Timeout) => {
                eprintln!("[runtime] 采集 {stream} 超时（{DRAIN_WAIT:?}），孙进程可能仍持有管道；跳过");
                String::new()
            }
            Err(RecvTimeoutError::Disconnected) => {
                eprintln!("[runtime] 采集 {stream} 的管道已断开，输出为空");
                String::new()
            }
        }
    }
}

/// Signal the helper's whole process group: it is its own session leader.
fn signal_group(pid: i32, signal: i32) {
    // SAFETY: a negative pid addresses the process group; the group only holds
    // the helper and the scripts it started, and ESRCH after it exited is fine.
    unsafe {
        libc::kill(-pid, signal);
    }
}

/// Pure shutdown gate: new helpers may spawn only before shutdown begins.
fn can_spawn(shutting_down: bool) -> bool {
    !shutting_down
}

/// Decide whether a timed-out helper still needs SIGKILL after the grace period.
///
/// Once try_wait has reaped the child, its pid may already have been reused by
/// an unrelated process group, so signalling the group would be unsafe.
fn should_force_kill(reaped_during_grace: bool) -> bool {
    !reaped_during_grace
}

/// Decide which snapshotted process groups still need SIGKILL after the grace
/// period. A helper that exited during the grace window has already been
/// unregistered by its RAII guard, so its pid may have been reused by an
/// unrelated group and must not be signalled.
fn still_needs_kill(snapshot: &[i32], live: &[i32]) -> Vec<i32> {
    snapshot.iter().copied().filter(|pid| live.contains(pid)).collect()
}

/// Stop every helper still in flight (called while the shell shuts down).
///
/// Each registered group gets SIGTERM first; after a short grace period every
/// group still registered after the grace period gets SIGKILL. Terminating one
/// helper must never affect another one's registration.
pub fn kill_running() {
    let pids: Vec<i32> = RUNNING.lock().map(|guard| guard.clone()).unwrap_or_default();
    if pids.is_empty() {
        return;
    }
    for pid in &pids {
        eprintln!("[runtime] 退出：终止正在运行的 npm（进程组 {pid}）");
        signal_group(*pid, libc::SIGTERM);
    }
    std::thread::sleep(Duration::from_millis(300));
    // Re-read after the grace period: helpers that exited on SIGTERM have
    // already unregistered themselves, and their pids may have been reused.
    let live: Vec<i32> = RUNNING.lock().map(|guard| guard.clone()).unwrap_or_default();
    for pid in still_needs_kill(&pids, &live) {
        signal_group(pid, libc::SIGKILL);
    }
}

/// Mark the shell as shutting down and stop every helper already in flight.
/// The flag is set before the kill snapshot so a concurrent `run()` cannot
/// spawn a replacement helper that shutdown would never find.
pub fn begin_shutdown() {
    SHUTDOWN.store(true, Ordering::SeqCst);
    kill_running();
}

/// Register a helper's process group as in flight.
fn register_running(pid: i32) {
    if let Ok(mut guard) = RUNNING.lock() {
        if !guard.contains(&pid) {
            guard.push(pid);
        }
    }
}

/// Forget only the helper that just finished; other in-flight helpers stay
/// registered so shutdown can still find them.
fn clear_running(pid: i32) {
    if let Ok(mut guard) = RUNNING.lock() {
        guard.retain(|&registered| registered != pid);
    }
}

/// RAII guard: a helper's process group stays registered for exactly as long as
/// this value lives, including when a scope is left early via `?` or `return`.
struct Registration(i32);

impl Registration {
    fn new(pid: i32) -> Self {
        register_running(pid);
        Self(pid)
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        clear_running(self.0);
    }
}

/// Run `program args…`; `Err` only when the process cannot be spawned at all.
///
/// `stdout`/`stderr` are drained on their own threads so a chatty helper can
/// never fill a pipe and deadlock while we wait for the deadline.
pub fn run(program: &Path, args: &[&str], timeout: Duration) -> Result<Outcome, String> {
    if !can_spawn(SHUTDOWN.load(Ordering::SeqCst)) {
        return Err("shell is shutting down".to_owned());
    }
    // SAFETY: between fork and exec only async-signal-safe calls are made, and
    // `setsid` has no memory-safety preconditions.
    let mut child = unsafe {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .pre_exec(|| {
                // Own session, so lifecycle scripts die with the group as well.
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                // The shell can die without running shutdown() (SIGTERM/SIGKILL):
                // take npm down with it. `setsid` does not change the parent.
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                // The shell may already be gone (reparented before prctl ran).
                if libc::getppid() == 1 {
                    return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
                }
                Ok(())
            })
            .spawn()
    }
    .map_err(|error| format!("无法运行 {}：{error}", program.display()))?;
    let pid = child.id() as i32;
    let _registration = Registration::new(pid);

    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                return Err(format!("等待 {} 时出错：{error}", program.display()));
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            signal_group(pid, libc::SIGTERM);
            let grace = Instant::now() + Duration::from_secs(2);
            let mut reaped_during_grace = false;
            while Instant::now() < grace {
                if let Ok(Some(_)) = child.try_wait() {
                    reaped_during_grace = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if should_force_kill(reaped_during_grace) {
                signal_group(pid, libc::SIGKILL);
            }
            break child.wait().map_err(|error| format!("无法回收 {}：{error}", program.display()))?;
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    Ok(Outcome {
        status,
        stdout: collect(stdout, "stdout"),
        stderr: collect(stderr, "stderr"),
        timed_out
    })
}

#[cfg(test)]
mod tests {
    use super::{can_spawn, kill_running, lossy_output, run, should_force_kill, still_needs_kill, Registration, RUNNING};
    use std::path::Path;
    use std::sync::{mpsc, Mutex};
    use std::time::{Duration, Instant};

    /// `RUNNING` is process-wide, so these tests must not overlap.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Test helper: is this pid currently in the global RUNNING set?
    fn is_registered(pid: i32) -> bool {
        RUNNING.lock().map(|guard| guard.contains(&pid)).unwrap_or(false)
    }

    /// 合法 UTF-8 字节流必须原样转成字符串，不引入替换符。
    #[test]
    fn lossy_output_keeps_valid_utf8_verbatim() {
        let bytes = b"hi \xe4\xbd\xa0\xe5\xa5\xbd ok".to_vec(); // "hi 你好 ok"
        assert_eq!(lossy_output(&bytes), "hi 你好 ok");
    }

    /// 无效字节不得吞掉整段输出：前后文本与替换符都要保留（非空）。
    #[test]
    fn lossy_output_preserves_text_around_invalid_bytes() {
        let bytes = b"hi\xffok".to_vec();
        let output = lossy_output(&bytes);
        assert!(!output.is_empty(), "invalid byte must not erase the whole output");
        assert_eq!(output, "hi\u{FFFD}ok");
    }

    /// A running shell may still spawn helpers.
    #[test]
    fn can_spawn_while_running() {
        assert!(can_spawn(false), "helpers must spawn before shutdown begins");
    }

    /// Once shutdown begins, no helper may be spawned at all.
    #[test]
    fn cannot_spawn_while_shutting_down() {
        assert!(!can_spawn(true), "shutdown must reject every new helper");
    }

    /// A child still alive after the grace period must be force-killed.
    #[test]
    fn force_kill_when_not_reaped() {
        assert!(should_force_kill(false), "an unreaped child still needs SIGKILL");
    }

    /// A child already reaped must NOT be signalled: its pid may be reused.
    #[test]
    fn no_force_kill_when_reaped() {
        assert!(!should_force_kill(true), "a reaped child must not receive SIGKILL");
    }

    /// Only the snapshotted groups still registered after the grace period
    /// need SIGKILL; the ones that exited during it must be left alone.
    #[test]
    fn still_needs_kill_keeps_only_registered_pids() {
        assert_eq!(still_needs_kill(&[1, 2, 3], &[2, 3]), vec![2, 3]);
    }

    /// When every helper exited during the grace period, no pid is safe to kill.
    #[test]
    fn still_needs_kill_empty_when_everyone_exited() {
        assert_eq!(still_needs_kill(&[1, 2], &[]), Vec::<i32>::new());
    }

    #[test]
    fn kill_running_stops_the_whole_process_group() {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let worker = std::thread::spawn(|| run(Path::new("/bin/sh"), &["-c", "sleep 30"], Duration::from_secs(60)));
        std::thread::sleep(Duration::from_millis(500));
        let started = Instant::now();
        kill_running();
        let outcome = worker.join().expect("worker thread").expect("helper started");
        assert!(!outcome.status.success(), "helper must be killed, got {:?}", outcome.status);
        assert!(!outcome.timed_out);
        assert!(started.elapsed() < Duration::from_secs(10), "kill must not wait for the sleep");
    }

    #[test]
    fn a_timeout_kills_the_whole_process_group() {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let started = Instant::now();
        let outcome = run(Path::new("/bin/sh"), &["-c", "sleep 30"], Duration::from_secs(1)).expect("helper started");
        assert!(outcome.timed_out, "the deadline must be reported");
        assert!(!outcome.status.success(), "helper must be killed on timeout");
        assert!(started.elapsed() < Duration::from_secs(10), "timeout must not wait for the sleep");
    }

    /// Two overlapping helpers must both be stopped by a single `kill_running()`:
    /// the old single-slot registry lost the first pid and left it orphaned.
    #[test]
    fn kill_running_stops_every_concurrent_helper() {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();
        let worker1 = std::thread::spawn(move || {
            let result = run(Path::new("/bin/sh"), &["-c", "sleep 30"], Duration::from_secs(60));
            let _ = tx1.send(result);
        });
        let worker2 = std::thread::spawn(move || {
            let result = run(Path::new("/bin/sh"), &["-c", "sleep 30"], Duration::from_secs(60));
            let _ = tx2.send(result);
        });

        // Both helpers must be registered before we ask shutdown to stop them.
        let wait_started = Instant::now();
        loop {
            let registered = RUNNING.lock().map(|guard| guard.len()).unwrap_or(0);
            if registered >= 2 {
                break;
            }
            assert!(wait_started.elapsed() < Duration::from_secs(10), "both helpers must register");
            std::thread::sleep(Duration::from_millis(20));
        }

        let started = Instant::now();
        kill_running();
        for rx in [&rx1, &rx2] {
            let outcome = rx.recv_timeout(Duration::from_secs(10)).expect("helper must exit promptly").expect("helper started");
            assert!(!outcome.status.success(), "every helper must be killed, got {:?}", outcome.status);
            assert!(!outcome.timed_out, "the deadline must not be the reason it stopped");
        }
        assert!(started.elapsed() < Duration::from_secs(10), "kill must not wait for the sleep");
        worker1.join().expect("worker 1 thread");
        worker2.join().expect("worker 2 thread");
    }

    /// RAII guarantee: leaving a scope early (like the timeout branch's
    /// `child.wait()?` path) must still unregister the pid via Drop.
    #[test]
    fn registration_unregisters_on_early_return() {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let pid = i32::MAX - 1234;
        fn early_return(pid: i32) {
            let _registration = Registration::new(pid);
            assert!(is_registered(pid), "pid must be registered inside the scope");
            // Simulate an early `?` return: control leaves the scope here, so the
            // guard must unregister the pid on drop even without a trailing statement.
        }
        assert!(!is_registered(pid), "pid must be clean before the test");
        early_return(pid);
        assert!(!is_registered(pid), "early return must still unregister the pid via Drop");
    }

    /// Direct check: the guard registers on construction and clears on drop.
    #[test]
    fn registration_clears_on_drop() {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let pid = i32::MAX - 1235;
        assert!(!is_registered(pid), "pid must be clean before the test");
        {
            let _registration = Registration::new(pid);
            assert!(is_registered(pid), "pid must be registered while the guard lives");
        }
        assert!(!is_registered(pid), "pid must be cleared after the guard drops");
    }

    /// A background grandchild that inherits the pipe must not hold the shell
    /// hostage: the old unconditional join() waited for its exit (seconds),
    /// while the bounded recv_timeout returns after DRAIN_WAIT with no output.
    #[test]
    fn grandchild_holding_pipe_does_not_hang_run() {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let started = Instant::now();
        // The shell exits right after `echo`, but `sleep 5` keeps the inherited
        // stdout/stderr pipes open far beyond the bounded drain window.
        let outcome = run(
            Path::new("/bin/sh"),
            &["-c", "(sleep 5 &) ; echo done"],
            Duration::from_secs(60),
        )
        .expect("helper started");
        assert!(outcome.status.success(), "the shell itself exited 0");
        assert!(!outcome.timed_out, "the deadline was not reached");
        assert_eq!(outcome.stdout, "", "drain timed out and yielded empty output");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "run() must not wait for the background grandchild, took {:?}",
            started.elapsed()
        );
    }
}
