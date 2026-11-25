//! Run a helper process with a deadline, capturing both streams.
//!
//! npm is the only heavy helper the shell drives: it must never hang the boot,
//! its output must reach the log verbatim, and a timeout or shell shutdown must
//! kill the whole process group — npm spawns lifecycle scripts of its own.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Group ids of the helpers currently running, so shutdown can stop every one.
static RUNNING: Mutex<Vec<i32>> = Mutex::new(Vec::new());

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

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Option<JoinHandle<String>> {
    pipe.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buffer = String::new();
            let _ = pipe.read_to_string(&mut buffer);
            buffer
        })
    })
}

/// Signal the helper's whole process group: it is its own session leader.
fn signal_group(pid: i32, signal: i32) {
    // SAFETY: a negative pid addresses the process group; the group only holds
    // the helper and the scripts it started, and ESRCH after it exited is fine.
    unsafe {
        libc::kill(-pid, signal);
    }
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
        stdout: stdout.and_then(|handle| handle.join().ok()).unwrap_or_default(),
        stderr: stderr.and_then(|handle| handle.join().ok()).unwrap_or_default(),
        timed_out
    })
}

#[cfg(test)]
mod tests {
    use super::{kill_running, run, should_force_kill, still_needs_kill, Registration, RUNNING};
    use std::path::Path;
    use std::sync::{mpsc, Mutex};
    use std::time::{Duration, Instant};

    /// `RUNNING` is process-wide, so these tests must not overlap.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Test helper: is this pid currently in the global RUNNING set?
    fn is_registered(pid: i32) -> bool {
        RUNNING.lock().map(|guard| guard.contains(&pid)).unwrap_or(false)
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
            return; // simulate an early `?` return
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
}
