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
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// Maximum number of stale-socket takeovers before we give up and fail open.
const MAX_TAKEOVER_ATTEMPTS: u8 = 3;

/// Socket file name used when falling back to the shared temp directory.
///
/// The temp directory is shared between users, so the uid keeps one user's
/// instance from taking over another user's socket.
fn fallback_socket_name(uid: u32) -> String {
    format!("dsh-tauri-{uid}.sock")
}

/// Address of the primary instance.
///
/// XDG_RUNTIME_DIR is already per-user, so the stable name is safe there. Only
/// when it is missing or empty do we fall back to the shared temp directory,
/// where the socket name must carry the uid instead.
fn socket_path() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("dsh-tauri.sock"),
        _ => {
            // SAFETY: getuid() has no preconditions and cannot fail.
            let uid = unsafe { libc::getuid() } as u32;
            std::env::temp_dir().join(fallback_socket_name(uid))
        }
    }
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

/// Log message for an accept-thread spawn failure (kept pure for testing).
fn spawn_failed_message(error: &std::io::Error) -> String {
    format!("无法启动单实例监听线程，允许本实例继续启动：{error}")
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
                let spawned = std::thread::Builder::new()
                    .name("dsh-tauri-single-instance".to_owned())
                    .spawn(move || accept_forever(listener, handle));
                if let Err(error) = spawned {
                    eprintln!("[dsh-tauri] {}", spawn_failed_message(&error));
                    // binding socket 成功但线程没起来：内核 backlog 会让后续启动
                    // connect/write 都成功，造成「没有实例在跑却每次闪退」的假死。
                    // 仅当路径仍是我们刚绑定的 socket 才移除；listener 随闭包 drop
                    // 自动关闭。fail-open：本实例继续启动。
                    if is_stale_socket(&path) {
                        if let Err(remove_error) = std::fs::remove_file(&path) {
                            eprintln!(
                                "[dsh-tauri] 无法移除刚绑定但监听线程未启动的 socket: {remove_error}"
                            );
                        }
                    }
                }
                return true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                // A live primary owns the socket; ask it to come forward.
                match UnixStream::connect(&path) {
                    Ok(mut stream) => {
                        match stream.write_all(b"focus\n") {
                            Ok(()) => return false,
                            Err(write_error) => {
                                eprintln!(
                                    "[dsh-tauri] 无法把焦点请求发给已有实例：{write_error}；允许本实例启动"
                                );
                                return true;
                            }
                        }
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
        // A client that connects but never sends a newline must not block this
        // thread forever: the next focus request would never be handled.
        // A read timeout (or any read error) just skips that one connection.
        if stream.set_read_timeout(Some(Duration::from_secs(2))).is_err() {
            continue;
        }
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
    fn fallback_socket_name_includes_uid() {
        assert_eq!(fallback_socket_name(1000), "dsh-tauri-1000.sock");
    }

    #[test]
    fn fallback_socket_name_zero_uid() {
        assert_eq!(fallback_socket_name(0), "dsh-tauri-0.sock");
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

    #[test]
    fn spawn_failed_message_includes_prefix_and_error() {
        let message = spawn_failed_message(&std::io::Error::other("资源耗尽"));
        assert!(message.starts_with("无法启动单实例监听"), "{message}");
        assert!(message.contains("资源耗尽"), "{message}");
    }
}
