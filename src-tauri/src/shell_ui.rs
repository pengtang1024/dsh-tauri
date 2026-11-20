//! Native shell furniture: the application menu, the tray icon and window
//! geometry persistence — the parts of the Electron shell that live outside the
//! web application itself.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow, WebviewWindowBuilder, Wry};

/// Window geometry remembered across launches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self { width: 1280.0, height: 840.0, x: None, y: None, maximized: false }
    }
}

fn state_path() -> PathBuf {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"));
    home.join(".dsh").join("dsh-tauri-window-state.json")
}

pub fn load_window_state() -> WindowState {
    fs::read_to_string(state_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Persist geometry; failures are logged but never block shutdown.
pub fn save_window_state(window: &WebviewWindow<Wry>) {
    let Ok(size) = window.inner_size() else { return };
    let position = window.outer_position().ok();
    let scale = window.scale_factor().unwrap_or(1.0);
    let state = WindowState {
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
        x: position.map(|point| (f64::from(point.x) / scale).round() as i32),
        y: position.map(|point| (f64::from(point.y) / scale).round() as i32),
        maximized: window.is_maximized().unwrap_or(false)
    };
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(error) = fs::write(&path, serde_json::to_string_pretty(&state).unwrap_or_default()) {
        eprintln!("[dsh-tauri] cannot persist window state: {error}");
    }
}

/** Read `closeToTray` from the shared desktop preferences (default: true). */
fn close_to_tray() -> bool {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"));
    let path = home.join(".dsh").join("desktop-preferences.json");
    let Ok(text) = fs::read_to_string(path) else { return true };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return true };
    value.get("closeToTray").and_then(serde_json::Value::as_bool).unwrap_or(true)
}

/// Menu shared by the window and the tray. Editing items are predefined so the
/// WebView keeps its native clipboard and undo behaviour.
pub fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let edit = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?
        ]
    )?;

    let reload = MenuItem::with_id(app, "reload", "重新加载", true, Some("CmdOrCtrl+R"))?;
    let fullscreen = MenuItem::with_id(app, "fullscreen", "全屏", true, Some("F11"))?;
    let devtools = MenuItem::with_id(app, "devtools", "开发者工具", true, Some("CmdOrCtrl+Shift+I"))?;
    let view = Submenu::with_items(app, "查看", true, &[&reload, &fullscreen, &devtools])?;

    let minimize = MenuItem::with_id(app, "minimize", "最小化", true, Some("CmdOrCtrl+M"))?;
    let close = MenuItem::with_id(app, "close", "关闭窗口", true, Some("CmdOrCtrl+W"))?;
    let window = Submenu::with_items(app, "窗口", true, &[&minimize, &close])?;

    Menu::with_items(app, &[&edit, &view, &window])
}

/// Wire menu actions; unknown ids are ignored so predefined items keep working.
pub fn handle_menu_event(app: &AppHandle<Wry>, id: &str) {
    let Some(window) = app.get_webview_window("main") else { return };
    match id {
        "reload" => {
            let _ = window.eval("window.location.reload()");
        }
        "fullscreen" => {
            let _ = window.set_fullscreen(!window.is_fullscreen().unwrap_or(false));
        }
        "devtools" => {
            #[cfg(debug_assertions)]
            window.open_devtools();
        }
        "minimize" => {
            let _ = window.minimize();
        }
        "close" => {
            save_window_state(&window);
            let _ = window.close();
        }
        _ => {}
    }
}

/// Tray icon: left click restores the window, the menu offers show/quit.
pub fn install_tray(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("DSH")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                crate::bridge::shutdown();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Explain a fatal startup problem in a real window: a console-only message
/// looks like the application never launched.
pub fn build_error_window(app: &AppHandle<Wry>, message: &str) -> Result<WebviewWindow<Wry>, String> {
    let encoded = urlencoding(message);
    WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App(format!("error.html#{encoded}").into()))
        .title("DSH Tauri")
        .inner_size(760.0, 520.0)
        .min_inner_size(520.0, 380.0)
        .build()
        .map_err(|error| format!("cannot open the error window: {error}"))
}

/** Percent-encode a message so it survives the fragment of a local document. */
fn urlencoding(message: &str) -> String {
    let mut encoded = String::with_capacity(message.len());
    for byte in message.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' | b' ' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}"))
        }
    }
    encoded
}

/**
 * Open the Platform sign-in window.
 *
 * The page is a normal HTTPS site; the only special part is the last hop — when
 * it navigates to the Host's `/oauth/callback`, the request has to be replayed
 * as the native renderer, so the bridge does it through the loopback proxy and
 * this window is closed once that settles.
 */
pub fn build_login_window(app: &AppHandle<Wry>, url: &str) -> Result<WebviewWindow<Wry>, String> {
    let target = tauri::Url::parse(url).map_err(|error| format!("invalid sign-in url: {error}"))?;
    let host_origin = crate::bridge::host_origin().unwrap_or_default();
    let window = WebviewWindowBuilder::new(app, "login", tauri::WebviewUrl::External(target))
        .title("登录 DeepSeek")
        .inner_size(520.0, 760.0)
        .on_navigation(move |url| {
            let is_callback = url.path() == "/oauth/callback"
                && (host_origin.is_empty() || url.origin().ascii_serialization() == host_origin);
            if is_callback {
                crate::bridge::complete_platform_login(url.as_str());
                return false;
            }
            true
        })
        .build()
        .map_err(|error| format!("cannot open the sign-in window: {error}"))?;
    Ok(window)
}

/// Build the main window with the remembered geometry and shared menu.
pub fn build_main_window(app: &AppHandle<Wry>, url: tauri::Url) -> Result<WebviewWindow<Wry>, String> {
    let state = load_window_state();
    let menu = build_menu(app).map_err(|error| format!("cannot build the application menu: {error}"))?;
    let mut builder = WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
        .title("DSH")
        .inner_size(state.width, state.height)
        .min_inner_size(880.0, 600.0)
        .menu(menu);
    // Wayland compositors own window placement, so a remembered position is
    // advisory: it is applied where the platform allows it and ignored elsewhere.
    if let (Some(x), Some(y)) = (state.x, state.y) {
        builder = builder.position(f64::from(x), f64::from(y));
    }
    let window = builder.build().map_err(|error| format!("cannot open the window: {error}"))?;
    if state.maximized {
        let _ = window.maximize();
    }
    // Closing must honour `closeToTray`: keep the Host alive in the tray instead
    // of tearing the session down, unless the user disabled it.
    let closable = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            if close_to_tray() {
                api.prevent_close();
                let _ = closable.hide();
            } else {
                save_window_state(&closable);
            }
        }
        tauri::WindowEvent::Destroyed => save_window_state(&closable),
        _ => {}
    });
    Ok(window)
}
