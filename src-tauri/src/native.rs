//! Native capabilities the browser cannot provide, exposed to the page as Tauri
//! commands. Everything here is Linux/GTK native: the shell is a desktop client
//! for the web UI, so these replace what a browser would otherwise refuse.

use std::process::Command;

use tauri::{AppHandle, Manager};

/// Schemes the renderer may ask the desktop to hand to an external program.
const ALLOWED_URL_SCHEMES: [&str; 3] = ["https://", "http://", "mailto:"];

/// Whether `url` may be passed to an external program.
///
/// The page is remote content, so only known schemes are accepted (matched
/// case-insensitively). ASCII control characters are rejected even though
/// they arrive as a single argv entry: a downstream handler could still read
/// them as separators. There is deliberately no host allowlist — the shell
/// must be able to open arbitrary external documentation links.
pub(crate) fn external_url_allowed(url: &str) -> bool {
    if url.is_empty() || url.bytes().any(|byte| byte.is_ascii_control()) {
        return false;
    }
    ALLOWED_URL_SCHEMES.iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    })
}

/// Outcome of a helper invocation. The exit code is classified here, once,
/// instead of being inferred from an error message.
enum HelperOutcome {
    Ok(String),
    Cancelled,
    Failed(String),
}

impl HelperOutcome {
    /// Pure classification of a finished helper. `code` is
    /// `output.status.code()` (`None` when the helper was killed by a signal);
    /// `cancel_code` is the exit code this caller treats as cancellation.
    fn from_code(code: Option<i32>, cancel_code: Option<i32>, program: &str) -> Self {
        match code {
            Some(0) => Self::Ok(String::new()),
            Some(code) if Some(code) == cancel_code => Self::Cancelled,
            Some(code) => Self::Failed(format!("{program} exited with exit status: {code}")),
            None => Self::Failed(format!("{program} terminated by signal")),
        }
    }
}

/// Run a helper and classify its exit code. `cancel_code` marks the one exit
/// code that means "the user cancelled"; with `None`, every non-zero exit is
/// a failure.
fn run_helper(program: &str, args: &[&str], cancel_code: Option<i32>) -> HelperOutcome {
    let output = match Command::new(program).args(args).output() {
        Ok(output) => output,
        Err(error) => return HelperOutcome::Failed(format!("cannot run {program}: {error}")),
    };
    match HelperOutcome::from_code(output.status.code(), cancel_code, program) {
        HelperOutcome::Ok(_) => {
            HelperOutcome::Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        HelperOutcome::Cancelled => HelperOutcome::Cancelled,
        HelperOutcome::Failed(_) => {
            // Keep stderr taking precedence, and otherwise reproduce the old
            // `run` message verbatim (including signal termination).
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            HelperOutcome::Failed(if stderr.is_empty() {
                format!("{program} exited with {}", output.status)
            } else {
                stderr
            })
        }
    }
}

/// Run a helper and capture stdout, reporting the failure text verbatim.
/// Every non-zero exit is a failure here; there is no cancellation case.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    match run_helper(program, args, None) {
        HelperOutcome::Ok(stdout) => Ok(stdout),
        HelperOutcome::Failed(error) => Err(error),
        HelperOutcome::Cancelled => Err(format!("{program} was cancelled")),
    }
}

/// Normalize zenity's stdout into a usable directory path.
fn picked_path(stdout: &str) -> Option<String> {
    let path = stdout.trim().trim_end_matches('/');
    (!path.is_empty()).then(|| path.to_owned())
}

/// Folder chooser for the workspace picker (`__DSH_DIRECTORY_PICKER__`).
#[tauri::command]
pub fn pick_directory(title: Option<String>) -> Result<Option<String>, String> {
    let mut args = vec!["--file-selection", "--directory", "--title"];
    let title = title.unwrap_or_else(|| "选择目录".to_owned());
    args.push(title.as_str());
    // zenity exits 1 when the user cancels; compare the code, never error text.
    match run_helper("zenity", &args, Some(1)) {
        HelperOutcome::Ok(stdout) => Ok(picked_path(&stdout)),
        HelperOutcome::Cancelled => Ok(None),
        HelperOutcome::Failed(error) => Err(error),
    }
}

#[tauri::command]
pub fn clipboard_read() -> Result<String, String> {
    run("wl-paste", &["--no-newline"]).map_err(|error| format!("{error}（需要 wl-clipboard）"))
}

/// Classify `wl-copy`'s exit code without needing a real `ExitStatus`.
///
/// `Some(0)` means a clean exit; `None` as the code means the child was killed
/// by a signal, which must also be reported rather than silently succeed.
fn status_message(code: Option<i32>) -> Option<String> {
    match code {
        Some(0) => None,
        Some(code) => Some(format!("wl-copy 退出码 {code}")),
        None => Some("wl-copy 被信号终止".to_owned()),
    }
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
    let status = child.wait().map_err(|error| format!("wl-copy failed: {error}"))?;
    if let Some(message) = status_message(status.code()) {
        return Err(format!(
            "wl-copy 写入剪贴板失败：{message}（需要 wl-clipboard 与可用的 Wayland 剪贴板管理器）"
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    // Never hand a non-URL to the shell: the renderer is remote content.
    if !external_url_allowed(&url) {
        // Echo the scheme only: the rest of the URL may carry tokens in its
        // query string, so it must never reach a log. Sanitize the scheme too
        // in case the rejected URL contains a control character before ':'.
        let scheme = url
            .split_once(':')
            .map(|(prefix, _)| prefix)
            .filter(|prefix| !prefix.is_empty())
            .unwrap_or("<none>");
        let scheme: String = scheme
            .chars()
            .take(32)
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.') {
                    c
                } else {
                    '?'
                }
            })
            .collect();
        return Err(format!(
            "refusing to open {scheme}: only http(s) and mailto are allowed"
        ));
    }
    run("xdg-open", &[&url]).map(|_| ())
}

/// Build the `notify-send` argv for a notification.
///
/// The `--` terminator stops GLib/GOption from interpreting a title or body
/// that starts with `-` (e.g. `--help`, `-u`) as an option, so page-provided
/// text is always passed through as plain summary/body text.
fn notify_args<'a>(title: &'a str, body: &'a str) -> Vec<&'a str> {
    vec!["-a", "DSH", "--", title, body]
}

#[tauri::command]
pub fn notify(title: String, body: String) -> Result<(), String> {
    let args = notify_args(&title, &body);
    run("notify-send", &args).map(|_| ())
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

#[cfg(test)]
mod tests {
    use super::{external_url_allowed, notify_args, picked_path, status_message, HelperOutcome};

    #[test]
    fn allows_https() {
        assert!(external_url_allowed("https://example.com/docs"));
    }

    #[test]
    fn allows_http() {
        assert!(external_url_allowed("http://example.com"));
    }

    #[test]
    fn allows_mailto() {
        assert!(external_url_allowed("mailto:someone@example.com"));
    }

    #[test]
    fn allows_uppercase_scheme() {
        assert!(external_url_allowed("HTTPS://example.com/docs"));
        assert!(external_url_allowed("HTTP://example.com"));
        assert!(external_url_allowed("MAILTO:someone@example.com"));
        assert!(external_url_allowed("HtTpS://example.com"));
    }

    #[test]
    fn allows_url_with_query_parameters() {
        assert!(external_url_allowed(
            "https://example.com/search?q=rust&lang=zh&token=abc%2Fdef#frag"
        ));
        assert!(external_url_allowed("http://example.com/a?x=1&y=2"));
        assert!(external_url_allowed("mailto:someone@example.com?subject=hi%20there"));
    }

    #[test]
    fn rejects_file_scheme() {
        assert!(!external_url_allowed("file:///etc/passwd"));
    }

    #[test]
    fn rejects_javascript_scheme() {
        assert!(!external_url_allowed("javascript:alert(1)"));
    }

    #[test]
    fn rejects_data_scheme() {
        assert!(!external_url_allowed("data:text/html,<script>alert(1)</script>"));
    }

    #[test]
    fn rejects_ftp_scheme() {
        assert!(!external_url_allowed("ftp://example.com/file"));
    }

    #[test]
    fn rejects_empty_string() {
        assert!(!external_url_allowed(""));
    }

    #[test]
    fn rejects_newline() {
        assert!(!external_url_allowed("https://example.com/a\nb"));
        assert!(!external_url_allowed("https://example.com/a\r\nX-Evil: 1"));
    }

    #[test]
    fn rejects_nul() {
        assert!(!external_url_allowed("https://example.com/a\0b"));
    }

    #[test]
    fn rejects_del() {
        assert!(!external_url_allowed("https://example.com/a\u{7f}b"));
    }

    #[test]
    fn rejects_all_ascii_controls() {
        for code in 0u8..=0x1f {
            let control = code as char;
            let url = format!("https://example.com/a{control}b");
            assert!(!external_url_allowed(&url), "control U+{code:04X} was accepted");
        }
        assert!(!external_url_allowed("https://example.com/a\u{7f}b"));
    }

    #[test]
    fn rejects_scheme_prefix_without_delimiter() {
        assert!(!external_url_allowed("https"));
        assert!(!external_url_allowed("mailto"));
        assert!(!external_url_allowed("httpsx://example.com"));
    }

    #[test]
    fn picked_path_returns_plain_path() {
        assert_eq!(
            picked_path("/home/user/project"),
            Some("/home/user/project".to_owned())
        );
    }

    #[test]
    fn picked_path_strips_trailing_slashes() {
        assert_eq!(
            picked_path("/home/user/project///"),
            Some("/home/user/project".to_owned())
        );
    }

    #[test]
    fn picked_path_strips_surrounding_whitespace() {
        assert_eq!(
            picked_path("  /home/user/project  \n"),
            Some("/home/user/project".to_owned())
        );
    }

    #[test]
    fn picked_path_rejects_empty() {
        assert_eq!(picked_path(""), None);
    }

    #[test]
    fn picked_path_rejects_whitespace_only() {
        assert_eq!(picked_path("  \n\t "), None);
    }

    #[test]
    fn picked_path_rejects_slashes_only() {
        assert_eq!(picked_path("/"), None);
        assert_eq!(picked_path("////"), None);
    }

    #[test]
    fn cancel_code_marks_only_the_configured_cancel() {
        assert!(matches!(
            HelperOutcome::from_code(Some(1), Some(1), "zenity"),
            HelperOutcome::Cancelled
        ));
        assert!(matches!(
            HelperOutcome::from_code(Some(1), None, "cmd"),
            HelperOutcome::Failed(_)
        ));
    }

    #[test]
    fn success_is_ok_even_with_a_cancel_code() {
        assert!(matches!(
            HelperOutcome::from_code(Some(0), Some(1), "zenity"),
            HelperOutcome::Ok(_)
        ));
    }

    #[test]
    fn other_exit_codes_are_failures() {
        assert!(matches!(
            HelperOutcome::from_code(Some(2), Some(1), "zenity"),
            HelperOutcome::Failed(_)
        ));
        assert!(matches!(
            HelperOutcome::from_code(Some(2), None, "cmd"),
            HelperOutcome::Failed(_)
        ));
    }

    #[test]
    fn signal_termination_is_failure() {
        assert!(matches!(
            HelperOutcome::from_code(None, Some(1), "zenity"),
            HelperOutcome::Failed(_)
        ));
        assert!(matches!(
            HelperOutcome::from_code(None, None, "cmd"),
            HelperOutcome::Failed(_)
        ));
    }

    #[test]
    fn notify_args_has_terminator_in_exact_position() {
        let args = notify_args("title", "body");
        assert_eq!(args.len(), 5);
        assert_eq!(args, vec!["-a", "DSH", "--", "title", "body"]);
    }

    #[test]
    fn notify_args_preserves_dash_prefixed_title() {
        let title = "--help";
        let args = notify_args(title, "body");
        assert_eq!(args.len(), 5);
        assert_eq!(args[3], title);
        assert_eq!(args[4], "body");
        // The terminator precedes the page-provided title, so it can never be
        // re-parsed as an option by notify-send.
        assert_eq!(args[2], "--");
    }

    #[test]
    fn notify_args_preserves_dash_prefixed_body() {
        let body = "-u attacker";
        let args = notify_args("title", body);
        assert_eq!(args.len(), 5);
        assert_eq!(args[3], "title");
        assert_eq!(args[4], body);
        assert_eq!(args[2], "--");
    }

    #[test]
    fn notify_args_preserves_empty_values() {
        let args = notify_args("", "");
        assert_eq!(args.len(), 5);
        assert_eq!(args, vec!["-a", "DSH", "--", "", ""]);
    }

    #[test]
    fn status_message_accepts_clean_exit() {
        assert_eq!(status_message(Some(0)), None);
    }

    #[test]
    fn status_message_reports_exit_code_one() {
        let message = status_message(Some(1)).expect("non-zero exit must fail");
        assert!(message.contains('1'), "unexpected message: {message}");
    }

    #[test]
    fn status_message_reports_exit_code_two() {
        let message = status_message(Some(2)).expect("non-zero exit must fail");
        assert!(message.contains('2'), "unexpected message: {message}");
    }

    #[test]
    fn status_message_reports_signal_termination() {
        let message = status_message(None).expect("signal termination must fail");
        assert!(message.contains("信号"), "unexpected message: {message}");
    }
}
