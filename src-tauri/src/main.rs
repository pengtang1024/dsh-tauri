//! Tauri shell for DeepSeek Harness.
//!
//! The shell provides the window, the tray and the native capabilities; the
//! Harness itself comes from the npm runtime this app ships (`@deepseek-ai/dsh`)
//! and runs as a plain Node process — no Electron, no DSH Desktop install.

mod banner;
mod dsh;
mod exec;
mod install;
mod native;
mod progress;
mod runtime;
mod shell_ui;
mod single_instance;
mod supervisor;
mod swap;
mod update;
mod version;

/// Build the readable message shown on the error page when a window cannot open.
fn window_error_message(stage: &str, error: &str) -> String {
    format!("{stage}：{error}")
}

/// Soft ceiling requested for `RLIMIT_NOFILE` (see `raise_nofile_limit`).
const NOFILE_TARGET: u64 = 65_536;

/// Soft limit to install: raise to `NOFILE_TARGET`, never above the hard limit
/// and never below the current soft limit.
fn nofile_target(soft: u64, hard: u64) -> u64 {
    let desired = NOFILE_TARGET.max(soft);
    if hard == libc::RLIM_INFINITY { desired } else { desired.min(hard) }
}

/// Raise the soft `RLIMIT_NOFILE` to `NOFILE_TARGET` (bounded by the hard limit).
///
/// The WebKitGTK web process inherits this process's limit, and WebKit's NVIDIA
/// DMABUF compositor leaks one `sync_file` fd per repainted frame (949 leaked
/// fds observed on a live web process). Once the 1024-fd soft limit is used up,
/// WebKit logs `Failed to allocate shared memory buffer` and the surface stays
/// frozen on its last frame — the persistent black band / clipped right edge
/// users report after a while. Raising the ceiling buys ~500x headroom until
/// WebKit stops leaking; failures are logged and never fatal.
fn raise_nofile_limit() {
    // SAFETY: getrlimit only fills the struct we pass; no other pointers are
    // involved and the struct is fully initialized before it is read.
    let mut limits = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits) } != 0 {
        eprintln!("[dsh-tauri] 无法读取 RLIMIT_NOFILE：{}", std::io::Error::last_os_error());
        return;
    }
    let soft = limits.rlim_cur;
    let target = nofile_target(soft, limits.rlim_max);
    if target <= soft {
        return;
    }
    let raised = libc::rlimit { rlim_cur: target, rlim_max: limits.rlim_max };
    // SAFETY: setrlimit takes a fully initialized struct by pointer and needs no
    // other preconditions; the hard limit is passed through unchanged.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } != 0 {
        eprintln!(
            "[dsh-tauri] 无法把 RLIMIT_NOFILE 从 {soft} 提高到 {target}：{}",
            std::io::Error::last_os_error()
        );
    }
}

/// WebKit env switch for the NVIDIA DMABUF renderer (the leaking code path).
const WEBKIT_DMABUF_ENV: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

/// True when an NVIDIA driver is loaded: only its DMABUF renderer leaks the
/// `sync_file` fds measured above.
fn nvidia_present() -> bool {
    std::path::Path::new("/sys/module/nvidia").exists()
        || std::path::Path::new("/proc/driver/nvidia/version").exists()
}

/// Whether to disable WebKit's DMABUF renderer. An explicit user value always
/// wins; otherwise it is disabled on NVIDIA, where the renderer leaks one
/// `sync_file` fd per repainted frame (measured: +374 fds over 30 resizes,
/// 0 after disabling). The fd ceiling alone only postpones that leak.
fn should_disable_dmabuf(user_value: Option<&str>, nvidia: bool) -> bool {
    user_value.is_none() && nvidia
}

/// Apply the renderer workaround before GTK/WebKit are initialized.
fn apply_webkit_workarounds() {
    let user = std::env::var(WEBKIT_DMABUF_ENV).ok();
    if !should_disable_dmabuf(user.as_deref(), nvidia_present()) {
        return;
    }
    std::env::set_var(WEBKIT_DMABUF_ENV, "1");
}

fn main() {
    // Must run before GTK/WebKit start: the web process inherits both of these.
    raise_nofile_limit();
    apply_webkit_workarounds();
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            native::pick_directory,
            native::clipboard_read,
            native::clipboard_write,
            native::open_external,
            native::notify,
            native::set_title,
            native::shell_info,
            progress::runtime_status
        ])
        .setup(|app| {
            if !single_instance::claim(app.handle()) {
                // The primary instance is already up: it just focused itself.
                std::process::exit(0);
            }
            // Safe now that we are the only instance: drop staging/old runtime
            // trees left by an interrupted install or a finished update.
            swap::clean_stale_runtime();
            let handle = app.handle().clone();
            if let Err(error) = shell_ui::install_tray(&handle) {
                // A tray is nice to have: a headless session must not lose the app.
                eprintln!("[dsh-tauri] tray unavailable: {error}");
            }
            // Development hook: attach to an already running `dsh web` instead of
            // spawning one (`DSH_TAURI_URL="http://127.0.0.1:PORT/?token=…"`).
            if let Ok(url) = std::env::var("DSH_TAURI_URL") {
                match url.parse::<tauri::Url>() {
                    Ok(parsed) if banner::is_safe_app_url(&parsed) => {
                        if let Err(error) = shell_ui::build_main_window(&handle, parsed) {
                            let message = window_error_message("无法打开开发 URL 指定的窗口", &error);
                            if let Err(error) = shell_ui::show_error(&handle, &message) {
                                eprintln!("[dsh-tauri] cannot show the error window: {error}");
                            }
                        }
                        return Ok(());
                    }
                    Ok(_) => eprintln!(
                        "[dsh-tauri] ignoring DSH_TAURI_URL: 只接受 loopback http(s) 地址"
                    ),
                    Err(error) => eprintln!("[dsh-tauri] ignoring DSH_TAURI_URL: {error}")
                }
            }
            if let Err(error) = shell_ui::build_boot_window(&handle) {
                let message = window_error_message("无法创建启动窗口", &error);
                if let Err(error) = shell_ui::show_error(&handle, &message) {
                    eprintln!("[dsh-tauri] cannot show the error window: {error}");
                }
                return Ok(());
            }
            supervisor::launch(handle);
            Ok(())
        })
        .on_menu_event(|app, event| shell_ui::handle_menu_event(app, event.id().as_ref()))
        .build(tauri::generate_context!())
        .expect("cannot build the Tauri application");

    app.run(|_handle, event| {
        if matches!(event, tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }) {
            supervisor::shutdown();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        nofile_target, raise_nofile_limit, should_disable_dmabuf, window_error_message,
        NOFILE_TARGET, WEBKIT_DMABUF_ENV,
    };

    #[test]
    fn boot_window_error_contains_stage_and_error() {
        let message = window_error_message("无法创建启动窗口", "permission denied");
        assert!(message.contains("无法创建启动窗口"));
        assert!(message.contains("permission denied"));
    }

    #[test]
    fn dev_url_window_error_contains_stage_and_error() {
        let message = window_error_message("无法打开开发 URL 指定的窗口", "no display");
        assert!(message.contains("无法打开开发 URL 指定的窗口"));
        assert!(message.contains("no display"));
    }

    // The NVIDIA DMABUF fd leak only wedges the UI once the inherited soft
    // RLIMIT_NOFILE is used up, so the ceiling itself is part of the contract.
    #[test]
    fn nofile_target_raises_the_default_soft_limit() {
        assert_eq!(nofile_target(1024, 524_288), NOFILE_TARGET);
    }

    #[test]
    fn nofile_target_never_exceeds_the_hard_limit() {
        assert_eq!(nofile_target(1024, 4096), 4096);
        assert_eq!(nofile_target(1024, 1024), 1024);
    }

    #[test]
    fn nofile_target_never_lowers_an_already_higher_soft_limit() {
        assert_eq!(nofile_target(100_000, 524_288), 100_000);
        assert_eq!(nofile_target(200_000, libc::RLIM_INFINITY), 200_000);
    }

    #[test]
    fn nofile_target_treats_an_infinite_hard_limit_as_the_ceiling() {
        assert_eq!(nofile_target(1024, libc::RLIM_INFINITY), NOFILE_TARGET);
    }

    #[test]
    fn dmabuf_is_disabled_only_on_nvidia_without_a_user_choice() {
        assert!(should_disable_dmabuf(None, true));
        assert!(!should_disable_dmabuf(None, false));
        // An explicit user value (0 or 1) is respected in both directions.
        assert!(!should_disable_dmabuf(Some("0"), true));
        assert!(!should_disable_dmabuf(Some("1"), true));
    }

    /// The workaround must land in the environment WebKit reads, and it must
    /// never overwrite a value the user already set.
    #[test]
    fn webkit_workaround_sets_the_env_only_without_a_user_value() {
        // SAFETY: single-threaded within this test; other tests do not read this var.
        if std::env::var(WEBKIT_DMABUF_ENV).is_err() {
            std::env::set_var(WEBKIT_DMABUF_ENV, "0");
        }
        super::apply_webkit_workarounds();
        assert_eq!(std::env::var(WEBKIT_DMABUF_ENV).as_deref(), Ok("0"));
    }

    /// The live limit must end up at the computed target and inside the hard
    /// limit: that is exactly what the WebKit web process inherits.
    #[test]
    fn raise_nofile_limit_applies_the_target() {
        let mut before = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: getrlimit fills the struct we pass and needs no other state.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut before) }, 0);
        let expected = nofile_target(before.rlim_cur, before.rlim_max);
        raise_nofile_limit();
        let mut after = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: same contract as above.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut after) }, 0);
        assert_eq!(after.rlim_cur, expected);
        assert!(after.rlim_cur <= after.rlim_max);
        assert_eq!(after.rlim_max, before.rlim_max, "hard limit must stay untouched");
    }
}
