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

fn main() {
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
                    Ok(parsed) => {
                        if let Err(error) = shell_ui::build_main_window(&handle, parsed) {
                            let message = window_error_message("无法打开开发 URL 指定的窗口", &error);
                            if let Err(error) = shell_ui::show_error(&handle, &message) {
                                eprintln!("[dsh-tauri] cannot show the error window: {error}");
                            }
                        }
                        return Ok(());
                    }
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
    use super::window_error_message;

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
}
