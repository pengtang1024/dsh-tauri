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

/// Group id of the helper currently running, so shutdown can stop it.
static RUNNING: Mutex<Option<i32>> = Mutex::new(None);

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

/// Stop a helper that is still in flight (called while the shell shuts down).
pub fn kill_running() {
    let pid = RUNNING.lock().ok().and_then(|guard| *guard);
    let Some(pid) = pid else { return };
    eprintln!("[runtime] 退出：终止正在运行的 npm（进程组 {pid}）");
    signal_group(pid, libc::SIGTERM);
    std::thread::sleep(Duration::from_millis(300));
    signal_group(pid, libc::SIGKILL);
}

fn clear_running() {
    if let Ok(mut guard) = RUNNING.lock() {
        *guard = None;
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
    if let Ok(mut guard) = RUNNING.lock() {
        *guard = Some(pid);
    }

    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                clear_running();
                return Err(format!("等待 {} 时出错：{error}", program.display()));
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            signal_group(pid, libc::SIGTERM);
            let grace = Instant::now() + Duration::from_secs(2);
            while Instant::now() < grace {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            signal_group(pid, libc::SIGKILL);
            break child.wait().map_err(|error| format!("无法回收 {}：{error}", program.display()))?;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    clear_running();

    Ok(Outcome {
        status,
        stdout: stdout.and_then(|handle| handle.join().ok()).unwrap_or_default(),
        stderr: stderr.and_then(|handle| handle.join().ok()).unwrap_or_default(),
        timed_out
    })
}

#[cfg(test)]
mod tests {
    use super::{kill_running, run};
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// `RUNNING` is process-wide, so these two tests must not overlap.
    static SERIAL: Mutex<()> = Mutex::new(());

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
}
