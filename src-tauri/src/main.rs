//! Tauri shell for DeepSeek Harness: a native window plus a thin supervisor for
//! the Node-side bridge that boots and proxies the upstream desktop Host.

mod bridge;

fn main() {
    let app = tauri::Builder::default()
        .setup(|app| {
            bridge::launch(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("cannot build the Tauri application");

    app.run(|_handle, event| {
        if matches!(event, tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }) {
            bridge::shutdown();
        }
    });
}
