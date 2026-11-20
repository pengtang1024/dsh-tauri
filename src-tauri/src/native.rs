//! Native capabilities the browser cannot provide, exposed to the page as Tauri
//! commands. Everything here is Linux/GTK native: the shell is a desktop client
//! for the web UI, so these replace what a browser would otherwise refuse.

use std::process::Command;

use tauri::{AppHandle, Manager};

/// Run a helper and capture stdout, reporting the failure text verbatim.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("{program} exited with {}", output.status)
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Folder chooser for the workspace picker (`__DSH_DIRECTORY_PICKER__`).
#[tauri::command]
pub fn pick_directory(title: Option<String>) -> Result<Option<String>, String> {
    let mut args = vec!["--file-selection", "--directory", "--title"];
    let title = title.unwrap_or_else(|| "选择目录".to_owned());
    args.push(title.as_str());
    match run("zenity", &args) {
        Ok(stdout) => {
            let path = stdout.trim().trim_end_matches('/');
            Ok((!path.is_empty()).then(|| path.to_owned()))
        }
        // zenity exits 1 when the user cancels; that is not an error.
        Err(error) if error.contains("exited with exit status: 1") => Ok(None),
        Err(error) => Err(error)
    }
}

#[tauri::command]
pub fn clipboard_read() -> Result<String, String> {
    run("wl-paste", &["--no-newline"]).map_err(|error| format!("{error}（需要 wl-clipboard）"))
}

#[tauri::command]
pub fn clipboard_write(text: String) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run wl-copy: {error}（需要 wl-clipboard）"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(text.as_bytes()).map_err(|error| format!("cannot write to wl-copy: {error}"))?;
    }
    child.wait().map_err(|error| format!("wl-copy failed: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    // Never hand a non-URL to the shell: the renderer is remote content.
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")) {
        return Err(format!("refusing to open {url}: only http(s) and mailto are allowed"));
    }
    run("xdg-open", &[&url]).map(|_| ())
}

#[tauri::command]
pub fn notify(title: String, body: String) -> Result<(), String> {
    run("notify-send", &["-a", "DSH", &title, &body]).map(|_| ())
}

#[tauri::command]
pub fn set_title(app: AppHandle, title: String) -> Result<(), String> {
    if title.is_empty() {
        return Ok(());
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.set_title(&title);
        }
    })
    .map_err(|error| format!("cannot reach the main thread: {error}"))
}

#[tauri::command]
pub fn shell_info(app: AppHandle) -> serde_json::Value {
    crate::dsh::info(&app)
}

/// Script injected before any page code: rebuilds the globals a desktop client
/// is expected to provide, on top of the Tauri commands above.
pub const SHIM: &str = r#"
(() => {
  const call = (command, args) => {
    const core = globalThis.__TAURI__ && globalThis.__TAURI__.core;
    if (!core) return Promise.reject(new Error('Tauri bridge unavailable'));
    return core.invoke(command, args);
  };
  globalThis.__DSH_TAURI__ = {
    protocolVersion: 1,
    info: () => call('shell_info'),
    openExternal: (url) => call('open_external', { url }),
    notify: (title, body) => call('notify', { title, body }),
    clipboard: {
      readText: () => call('clipboard_read'),
      writeText: (text) => call('clipboard_write', { text })
    }
  };
  globalThis.__DSH_DIRECTORY_PICKER__ = {
    pick: (options) => call('pick_directory', { title: options && options.title })
  };
  // WebKit keeps document.title to itself; mirror it into the native title bar.
  let last = '';
  const syncTitle = () => {
    const title = document.title;
    if (title && title !== last) {
      last = title;
      call('set_title', { title }).catch(() => {});
    }
  };
  const start = () => {
    syncTitle();
    new MutationObserver(syncTitle).observe(document.head || document.documentElement, {
      subtree: true,
      childList: true,
      characterData: true
    });
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
})();
"#;
