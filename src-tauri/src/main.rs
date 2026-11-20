//! Tauri shell for DeepSeek Harness: a native window plus a thin supervisor for
//! the Node-side bridge that boots and proxies the upstream desktop Host.

mod bridge;
mod shell_ui;

fn main() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            if let Err(error) = shell_ui::install_tray(&handle) {
                // A tray is nice to have: a headless session must not lose the app.
                eprintln!("[dsh-tauri] tray unavailable: {error}");
            }
            bridge::launch(handle);
            Ok(())
        })
        .on_menu_event(|app, event| shell_ui::handle_menu_event(app, event.id().as_ref()))
        .build(tauri::generate_context!())
        .expect("cannot build the Tauri application");

    app.run(|_handle, event| {
        if matches!(event, tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }) {
            bridge::shutdown();
        }
    });
}
