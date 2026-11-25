//! One shell per desktop session.
//!
//! Two shells on one profile would start two Hosts over the same ~/.dsh
//! database, so the second launch hands its intent to the first one and exits.
//! A Unix socket in the runtime directory is the cheapest channel that cannot
//! outlive the process: the listener dies with the process, so a stale file is
//! removed on the next start.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// Maximum number of stale-socket takeovers before we give up and fail open.
const MAX_TAKEOVER_ATTEMPTS: u8 = 3;

/// Address of the primary instance, under the per-user runtime directory.
fn socket_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join("dsh-tauri.sock")
}

/// True only when `path` itself is a Unix socket.
///
/// symlink_metadata does not follow links, so a symlink pointing at a socket
/// is reported as false: we must never remove a link a user placed there.
fn is_stale_socket(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_socket())
        .unwrap_or(false)
}

/// Returns true when this process should keep running.
pub fn claim(app: &AppHandle) -> bool {
    let path = socket_path();
    let mut takeovers = 0u8;

    // Bounded loop, never recursion: a permanently occupied path or a socket
    // we cannot unlink must not overflow the stack. On exhaustion we fail open
    // so the user can still start the shell.
    loop {
        match UnixListener::bind(&path) {
            Ok(listener) => {
                let handle = app.clone();
                std::thread::Builder::new()
                    .name("dsh-tauri-single-instance".to_owned())
                    .spawn(move || accept_forever(listener, handle))
                    .ok();
                return true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                // A live primary owns the socket; ask it to come forward.
                match UnixStream::connect(&path) {
                    Ok(mut stream) => {
                        let _ = stream.write_all(b"focus\n");
                        return false;
                    }
                    Err(connect_error) => {
                        // Connect failed. Only a genuine socket may be removed;
                        // a regular file or directory at this path is not ours.
                        if !is_stale_socket(&path) {
                            eprintln!(
                                "[dsh-tauri] instance socket path is occupied by a non-socket entry (connect failed: {connect_error}); not removing it, allowing startup"
                            );
                            return true;
                        }
                        if takeovers >= MAX_TAKEOVER_ATTEMPTS {
                            eprintln!(
                                "[dsh-tauri] failed to take over the stale instance socket after {MAX_TAKEOVER_ATTEMPTS} attempts; allowing startup"
                            );
                            return true;
                        }
                        takeovers += 1;
                        if let Err(remove_error) = std::fs::remove_file(&path) {
                            eprintln!(
                                "[dsh-tauri] cannot remove stale instance socket (attempt {takeovers}/{MAX_TAKEOVER_ATTEMPTS}): {remove_error}"
                            );
                        }
                        // Retry bind.
                    }
                }
            }
            Err(error) => {
                eprintln!("[dsh-tauri] cannot claim the instance socket: {error}");
                return true;
            }
        }
    }
}

/// Label of the window that should answer a focus request.
///
/// The main window does not exist during startup (only the splash does), so
/// falling back to the splash keeps the second launch from being swallowed.
fn focus_label(main_exists: bool) -> &'static str {
    if main_exists { "main" } else { "splash" }
}

fn accept_forever(listener: UnixListener, app: AppHandle) {
    for stream in listener.incoming().flatten() {
        let mut line = String::new();
        if BufReader::new(stream).read_line(&mut line).is_err() {
            continue;
        }
        if line.trim() != "focus" {
            continue;
        }
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let main_exists = handle.get_webview_window("main").is_some();
            let label = focus_label(main_exists);
            if let Some(window) = handle.get_webview_window(label) {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    /// Unique temporary directory, removed on drop (including any sockets).
    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let seq = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "dsh-tauri-single-instance-test-{}-{seq}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create unique test dir");
            Self(dir)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn focus_label_main_exists_returns_main() {
        assert_eq!(focus_label(true), "main");
    }

    #[test]
    fn focus_label_main_missing_returns_splash() {
        assert_eq!(focus_label(false), "splash");
    }

    #[test]
    fn is_stale_socket_missing_path_is_false() {
        let dir = TestDir::new();
        assert!(!is_stale_socket(&dir.join("missing.sock")));
    }

    #[test]
    fn is_stale_socket_regular_file_is_false() {
        let dir = TestDir::new();
        let file = dir.join("regular.txt");
        std::fs::write(&file, b"not a socket").expect("write regular file");
        assert!(!is_stale_socket(&file));
    }

    #[test]
    fn is_stale_socket_directory_is_false() {
        let dir = TestDir::new();
        let subdir = dir.join("subdir");
        std::fs::create_dir(&subdir).expect("create subdir");
        assert!(!is_stale_socket(&subdir));
    }

    #[test]
    fn is_stale_socket_real_socket_is_true() {
        let dir = TestDir::new();
        let socket = dir.join("live.sock");
        let listener = UnixListener::bind(&socket).expect("bind socket");
        assert!(is_stale_socket(&socket));
        drop(listener);
    }

    #[test]
    fn is_stale_socket_symlink_to_socket_is_false() {
        let dir = TestDir::new();
        let socket = dir.join("target.sock");
        let listener = UnixListener::bind(&socket).expect("bind socket");
        let link = dir.join("link.sock");
        symlink(&socket, &link).expect("create symlink");
        // symlink_metadata must not follow the link: a link is never removable.
        assert!(!is_stale_socket(&link));
        drop(listener);
    }
}
