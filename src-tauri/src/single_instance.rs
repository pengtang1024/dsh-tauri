//! One shell per desktop session.
//!
//! Two shells on one profile would start two Hosts over the same `~/.dsh`
//! database, so the second launch hands its intent to the first one and exits.
//! A Unix socket in the runtime directory is the cheapest channel that cannot
//! outlive the process: the listener dies with the process, so a stale file is
//! removed on the next start.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Address of the primary instance, under the per-user runtime directory.
fn socket_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join("dsh-tauri.sock")
}

/// Returns `true` when this process should keep running.
pub fn claim(app: &AppHandle) -> bool {
    let path = socket_path();
    match UnixListener::bind(&path) {
        Ok(listener) => {
            let handle = app.clone();
            std::thread::Builder::new()
                .name("dsh-tauri-single-instance".to_owned())
                .spawn(move || accept_forever(listener, handle))
                .ok();
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            // A live primary owns the socket; ask it to come forward.
            match UnixStream::connect(&path) {
                Ok(mut stream) => {
                    let _ = stream.write_all(b"focus\n");
                    false
                }
                Err(_) => {
                    // The file is stale: take it over.
                    let _ = std::fs::remove_file(&path);
                    claim(app)
                }
            }
        }
        Err(error) => {
            eprintln!("[dsh-tauri] cannot claim the instance socket: {error}");
            true
        }
    }
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
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        });
    }
}
