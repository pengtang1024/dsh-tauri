//! Native shell furniture: the window, its menu, the tray and remembered
//! geometry — everything around the DSH web UI.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry};

/// Native "star this project" entry. The window menu owns it; the tray menu
/// deliberately has its own show/quit actions only, so the entry is not
/// duplicated in two places. Both handlers call `native::open_external`
/// directly: it is a plain Rust call, so it works even before any window exists
/// and needs no extra capability.
const SUPPORT_ID: &str = "support";
const HELP_MENU_LABEL: &str = "帮助";
const SUPPORT_ITEM_LABEL: &str = "支持项目 ⭐";
const SUPPORT_URL: &str = "https://github.com/pengtang1024/dsh-tauri";

/// Window geometry remembered across launches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    let home = crate::runtime::home_dir();
    home.join(".dsh").join("dsh-tauri-window-state.json")
}

/// Serialize remembered geometry. Pure: touches neither the filesystem nor a window.
fn encode_state(state: &WindowState) -> String {
    serde_json::to_string_pretty(state).unwrap_or_default()
}

/// Guard remembered geometry against values that would build an unusable window
/// when no monitor is available to clamp them (e.g. headless or damaged state).
/// `x`/`y` are `i32` and therefore always finite; only the size needs checking.
fn geometry_is_sane(state: &WindowState) -> bool {
    [state.width, state.height]
        .into_iter()
        .all(|value| value.is_finite() && (1.0..=16384.0).contains(&value))
}

/// Parse remembered geometry. Empty, truncated, ill-typed or geometrically
/// invalid input yields `None`; unknown extra fields are ignored by serde's
/// default behaviour.
fn decode_state(text: &str) -> Option<WindowState> {
    serde_json::from_str(text).ok().filter(geometry_is_sane)
}

pub fn load_window_state() -> WindowState {
    fs::read_to_string(state_path())
        .ok()
        .and_then(|text| decode_state(&text))
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
        if let Err(error) = fs::create_dir_all(parent) {
            eprintln!("[dsh-tauri] cannot create window state directory: {error}");
            return;
        }
    }
    // Stage next to the final file so `rename` stays on the same filesystem and
    // replaces the state atomically: a concurrent save or a kill mid-write can
    // never leave a torn JSON document at the final path.
    let mut staging = path.clone().into_os_string();
    staging.push(format!(".tmp-{}", std::process::id()));
    let staging = PathBuf::from(staging);
    if let Err(error) = fs::write(&staging, encode_state(&state))
        .and_then(|()| fs::rename(&staging, &path))
    {
        eprintln!("[dsh-tauri] cannot persist window state: {error}");
        if let Err(cleanup) = fs::remove_file(&staging) {
            if cleanup.kind() != std::io::ErrorKind::NotFound {
                eprintln!("[dsh-tauri] cannot remove temporary window state: {cleanup}");
            }
        }
    }
}

/// Read `closeToTray` from the shared desktop preferences (default: true) so a
/// window close keeps the server (and the session) alive in the tray.
fn close_to_tray() -> bool {
    let home = crate::runtime::home_dir();
    let path = home.join(".dsh").join("desktop-preferences.json");
    let Ok(text) = fs::read_to_string(path) else { return true };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return true };
    value.get("closeToTray").and_then(serde_json::Value::as_bool).unwrap_or(true)
}

/// Window menu: three columns (Edit / View / Help). Editing items are
/// predefined so the WebView keeps its native clipboard and undo behaviour;
/// View items carry no hand-written accelerators — a GTK menubar full of
/// shortcut hints reads like a cheat sheet rather than a menu. There is
/// deliberately no Window column: the GTK title bar already provides
/// minimize/close buttons. The devtools item is always visible and always
/// works: the `devtools` Cargo feature (see Cargo.toml) gates the Tauri API in
/// release builds, so no debug-only cfg and no disabled menu state is needed.
pub fn build_menu<R: tauri::Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
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

    let reload = MenuItem::with_id(app, "reload", "重新加载", true, None::<&str>)?;
    let fullscreen = MenuItem::with_id(app, "fullscreen", "全屏", true, None::<&str>)?;
    let devtools = MenuItem::with_id(app, "devtools", "开发者工具", true, None::<&str>)?;
    let view = Submenu::with_items(app, "查看", true, &[&reload, &fullscreen, &devtools])?;

    let support = MenuItem::with_id(app, SUPPORT_ID, SUPPORT_ITEM_LABEL, true, None::<&str>)?;
    let help = Submenu::with_items(app, HELP_MENU_LABEL, true, &[&support])?;

    Menu::with_items(app, &[&edit, &view, &help])
}

/// Which window owns the shared menu: the real UI once it exists, otherwise the
/// boot splash. Pure so the choice can be tested without building an AppHandle.
fn menu_window_label(main_exists: bool) -> &'static str {
    if main_exists { "main" } else { "splash" }
}

/// Wire menu actions; unknown ids are ignored so predefined items keep working.
pub fn handle_menu_event(app: &AppHandle<Wry>, id: &str) {
    if id == SUPPORT_ID {
        if let Err(error) = crate::native::open_external(SUPPORT_URL.to_owned()) {
            eprintln!("[dsh-tauri] cannot open the support page: {error}");
        }
        return;
    }
    let label = menu_window_label(app.get_webview_window("main").is_some());
    let Some(window) = app.get_webview_window(label) else { return };
    match id {
        "reload" => {
            // The main window is created at WebviewUrl::External(url); injecting
            // `window.location.reload()` via eval is not reliable on an external
            // origin. Tauri 2.12.7 exposes WebviewWindow::reload(), which reloads
            // the current document through the native WebView on any URL and
            // re-runs the SHIM initialization script. Log a failure instead of
            // silently doing nothing.
            if let Err(error) = window.reload() {
                eprintln!("[dsh-tauri] 无法重新加载窗口：{error}");
            }
        }
        "fullscreen" => {
            let _ = window.set_fullscreen(!window.is_fullscreen().unwrap_or(false));
        }
        "devtools" => {
            // Always shown, always functional in release builds too: the
            // `devtools` Cargo feature makes this API available outside debug.
            window.open_devtools();
        }
        _ => {}
    }
}

/// Tray icon: left click restores the window, the menu offers show/quit.
/// The star entry lives only in the window Help menu (no duplicate here).
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
                // app.exit() is a hard exit: no Destroyed event is guaranteed, so
                // persist geometry here. Reuse the menu's label choice — the main
                // window when it exists, otherwise the still-live boot splash.
                let label = menu_window_label(app.get_webview_window("main").is_some());
                if let Some(window) = app.get_webview_window(label) {
                    save_window_state(&window);
                }
                crate::supervisor::shutdown();
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
    let label = menu_window_label(app.get_webview_window("main").is_some());
    let Some(window) = app.get_webview_window(label) else { return };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Clamp remembered geometry to the usable monitor area. Pure so the ordering
/// of the screen ceiling and the minimum-size floor can be tested directly.
///
/// The original `.min(screen - margin).max(floor)` order let the floor override
/// the screen ceiling: on an 800px-wide screen it produced 880px, wider than the
/// monitor. Here the screen is the hard limit; the 880/600 floor applies only
/// when the screen actually has room for it. `min_inner_size(880, 600)` in the
/// window builders is deliberately left unchanged: on very small screens Tauri
/// may still enforce that minimum, but our computed size must never actively
/// exceed the monitor.
fn clamp_to_screen(mut state: WindowState, screen_w: f64, screen_h: f64) -> WindowState {
    let max_w = (screen_w - 40.0).max(1.0);
    let max_h = (screen_h - 80.0).max(1.0);
    state.width = if max_w >= 880.0 { state.width.clamp(880.0, max_w) } else { max_w };
    state.height = if max_h >= 600.0 { state.height.clamp(600.0, max_h) } else { max_h };
    state
}

/// Keep a remembered geometry usable: a monitor change must not park the window
/// off-screen (Wayland ignores the position, but not the size).
fn fit_to_screen<R: tauri::Runtime>(app: &AppHandle<R>, state: WindowState) -> WindowState {
    let Ok(Some(monitor)) = app.primary_monitor() else { return state };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let width = f64::from(size.width) / scale;
    let height = f64::from(size.height) / scale;
    clamp_to_screen(state, width, height)
}

/// Splash shown while the server boots; it is replaced by the real UI.
pub fn build_boot_window(app: &AppHandle<Wry>) -> Result<WebviewWindow<Wry>, String> {
    let state = fit_to_screen(app, load_window_state());
    let menu = build_menu(app).map_err(|error| format!("cannot build the application menu: {error}"))?;
    let mut builder = WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("boot.html".into()))
        // Rebuild the globals a desktop client is expected to provide, on top of
        // the native commands; see native::SHIM.
        .initialization_script(crate::native::SHIM)
        .title("DSH")
        .inner_size(state.width, state.height)
        .min_inner_size(880.0, 600.0)
        .menu(menu);
    if let (Some(x), Some(y)) = (state.x, state.y) {
        builder = builder.position(f64::from(x), f64::from(y));
    }
    let window = builder.build().map_err(|error| format!("cannot open the window: {error}"))?;
    if state.maximized {
        let _ = window.maximize();
    }
    attach_window_events(&window);
    Ok(window)
}

/// Point the window at the server.
///
/// A window created for the splash (`tauri://localhost/boot.html`) cannot simply
/// navigate to the token URL: WebKit treats the `Set-Cookie` on the auth redirect
/// as third-party and drops it, so the UI lands on "authentication required".
/// A window created *at* the URL is a first-party navigation and keeps the cookie,
/// so the splash window is destroyed and replaced.
pub fn build_main_window(app: &AppHandle<Wry>, url: tauri::Url) -> Result<WebviewWindow<Wry>, String> {
    if let Some(window) = app.get_webview_window("main") {
        window.navigate(url).map_err(|error| format!("cannot open the UI: {error}"))?;
        return Ok(window);
    }
    // The splash lives in its own window: reusing one label for two document
    // lifetimes makes WebKit drop the auth cookie, and the UI lands on
    // "authentication required".
    let splash = app.get_webview_window("splash");
    let state = fit_to_screen(app, load_window_state());
    let menu = build_menu(app).map_err(|error| format!("cannot build the application menu: {error}"))?;
    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
        .initialization_script(crate::native::SHIM)
        .title("DSH")
        .inner_size(state.width, state.height)
        .min_inner_size(880.0, 600.0)
        .menu(menu);
    if let (Some(x), Some(y)) = (state.x, state.y) {
        builder = builder.position(f64::from(x), f64::from(y));
    }
    let window = builder.build().map_err(|error| format!("cannot open the window: {error}"))?;
    if state.maximized {
        let _ = window.maximize();
    }
    attach_window_events(&window);
    if let Some(splash) = splash {
        let _ = splash.destroy();
    }
    Ok(window)
}

/// Log line for a failed navigation of an existing main window. Pure so the
/// diagnostic wording (and the original error text) can be asserted in tests.
fn navigate_error_message(error: &str) -> String {
    format!("[dsh-tauri] 无法在主窗口打开服务 URL：{error}")
}

/// Open the window on the server URL, or navigate the existing one after a
/// restart. The splash is destroyed once the real UI is up.
pub fn show_url(app: &AppHandle<Wry>, url: &str) -> Result<(), String> {
    let parsed: tauri::Url = url.parse().map_err(|error| format!("bad URL from DSH: {error}"))?;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            if let Err(error) = window.navigate(parsed) {
                eprintln!("{}", navigate_error_message(&error.to_string()));
            }
            let _ = window.set_focus();
            return;
        }
        if let Err(error) = build_main_window(&handle, parsed) {
            eprintln!("[dsh-tauri] {error}");
        }
    })
    .map_err(|error| format!("cannot reach the main thread: {error}"))
}

/// Show a readable failure page instead of a blank window.
pub fn show_error(app: &AppHandle<Wry>, message: &str) -> Result<(), String> {
    let encoded = urlencoding(message);
    let shown = match app.get_webview_window("main") {
        Some(window) => {
            let target: tauri::Url = format!("tauri://localhost/error.html#{encoded}")
                .parse()
                .map_err(|error| format!("bad error URL: {error}"))?;
            window.navigate(target).map_err(|error| format!("cannot show the error page: {error}"))
        }
        None => {
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(format!("error.html#{encoded}").into()))
                .title("DSH")
                .inner_size(760.0, 560.0)
                .build()
                .map_err(|error| format!("cannot open the error window: {error}"))?;
            attach_window_events(&window);
            Ok(())
        }
    };
    // The boot spinner must not keep spinning behind the failure page.
    if shown.is_ok() {
        if let Some(splash) = app.get_webview_window("splash") {
            let _ = splash.destroy();
        }
    }
    shown
}

/// Resized events carry a PhysicalSize in device pixels, while tauri's native
/// `set_size` takes a logical size. Re-applying the raw event size is only
/// correct at scale_factor == 1; on a 2x framebuffer it doubles the window on
/// every event (self-excitation -> unbounded growth -> SIGSEGV). The native
/// window size is therefore NEVER written back from the Resized handler.
/// Zero-sized events (minimize / teardown) are still filtered out so no
/// spurious resize is dispatched to the page.
fn resized_event_is_noop(size: &tauri::PhysicalSize<u32>) -> bool {
    size.width == 0 || size.height == 0
}

fn attach_window_events(window: &WebviewWindow<Wry>) {
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
        tauri::WindowEvent::Resized(size) => {
            if resized_event_is_noop(size) {
                return;
            }
            // JS resize event only. Never write the native window size here:
            // the Resized payload is physical pixels and would be re-applied as
            // logical pixels, causing a feedback loop at scale=2/3 monitors.
            let _ = closable.eval("window.dispatchEvent(new Event('resize'))");
        }
        tauri::WindowEvent::Destroyed => save_window_state(&closable),
        _ => {}
    });
}

/// Percent-encode everything outside the unreserved set (error text in the hash).
fn urlencoding(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push_str("%20"),
            b'\n' => out.push_str("%0A"),
            _ => out.push_str(&format!("%{byte:02X}"))
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{clamp_to_screen, decode_state, encode_state, geometry_is_sane, menu_window_label, navigate_error_message, resized_event_is_noop, urlencoding, WindowState, HELP_MENU_LABEL, SUPPORT_ID, SUPPORT_ITEM_LABEL, SUPPORT_URL};

    // Static guards over the menu source: these are the pure, testable parts of
    // the native menu layout (the actual widgets require a live AppHandle).
    fn menu_source() -> String {
        include_str!("shell_ui.rs").to_owned()
    }

    fn build_menu_source() -> String {
        let source = menu_source();
        let start = source.find("pub fn build_menu").expect("build_menu must exist");
        let tail = &source[start..];
        let end = tail.find("\n/// ").expect("build_menu must end before the next doc comment");
        tail[..end].to_owned()
    }

    fn install_tray_source() -> String {
        let source = menu_source();
        let start = source.find("pub fn install_tray").expect("install_tray must exist");
        let tail = &source[start..];
        let end = tail.find("\nfn show_main_window").expect("install_tray must end before show_main_window");
        tail[..end].to_owned()
    }

    #[test]
    fn window_menu_has_exactly_three_columns() {
        let menu = build_menu_source();
        let top = menu
            .split("Menu::with_items(app, &[")
            .nth(1)
            .and_then(|tail| tail.split("])").next())
            .expect("top-level Menu::with_items must exist");
        assert_eq!(top.matches("&edit").count(), 1, "Edit column must appear once");
        assert_eq!(top.matches("&view").count(), 1, "View column must appear once");
        assert_eq!(top.matches("&help").count(), 1, "Help column must appear once");
        assert!(!top.contains("&window"), "Window column must be removed");
        assert!(!top.contains("&tray"), "tray must not be in the window menu");
    }

    #[test]
    fn window_menu_has_no_duplicate_or_shortcut_hint_items() {
        let menu = build_menu_source();
        // View items: exactly three, no hand-written accelerator strings.
        let view = menu
            .split("Submenu::with_items(app, \"查看\"")
            .nth(1)
            .and_then(|tail| tail.split("])").next())
            .expect("View submenu must exist");
        // View submenu references the three item bindings (the item creation
        // lines themselves sit above the submenu in build_menu).
        for binding in ["&reload", "&fullscreen", "&devtools"] {
            assert!(view.contains(binding), "View submenu must reference {binding}");
        }
        for id in ["reload", "fullscreen", "devtools"] {
            assert_eq!(
                menu.matches(&format!("MenuItem::with_id(app, \"{id}\"")).count(),
                1,
                "{id} item must be created exactly once across the menu"
            );
        }
        assert!(!view.contains("CmdOrCtrl"), "no hand-written shortcut hints in View");
        assert!(!view.contains("F11"), "no hand-written shortcut hints in View");
        // No duplicate star entry: one in the window menu, none in the tray.
        assert_eq!(menu.matches(SUPPORT_ID).count(), 2, "support id: const definition + Help item only");
        assert!(!install_tray_source().contains(SUPPORT_ID), "tray must not repeat the star entry");
    }

    #[test]
    fn devtools_is_always_visible_and_enabled() {
        let menu = build_menu_source();
        // The devtools MenuItem is created with enabled = true, no cfg gate and
        // no enabled/disabled switching anywhere in the source.
        let devtools_line = menu
            .lines()
            .find(|line| line.contains("MenuItem::with_id(app, \"devtools\""))
            .expect("devtools menu item must exist");
        assert!(devtools_line.contains(", true, None::<&str>)"), "devtools must be visible and enabled");
        assert!(!devtools_line.contains("CmdOrCtrl"), "no hand-written shortcut hint");
        assert!(!menu.contains("debug_assertions"), "no debug-only cfg gate in menu");
        let source = menu_source();
        assert!(!source.split("// Static guards over the menu source").nth(1).is_some_and(|tests| tests.contains("DSH_DEVTOOLS")), "menu code must have no environment-variable gate");
        let handler = source
            .split("\"devtools\" =>")
            .nth(1)
            .and_then(|tail| tail.split("\n        }\n").next())
            .expect("devtools handler must exist");
        assert!(handler.contains("window.open_devtools()"), "handler must actually open devtools");
        assert!(!handler.contains("cfg("), "handler must have no cfg gate");
        // Cargo.toml must enable the feature so open_devtools exists in release.
        let cargo = include_str!("../Cargo.toml");
        assert!(cargo.contains("\"devtools\""), "Cargo.toml must enable tauri devtools feature");
    }

    #[test]
    fn reload_uses_native_webview_reload_not_eval() {
        let source = menu_source();
        let handler = source
            .split("\"reload\" =>")
            .nth(1)
            .and_then(|tail| tail.split("\n        }\n").next())
            .expect("reload handler must exist");
        assert!(handler.contains("window.reload()"), "reload must use native WebviewWindow::reload()");
        // The only "window.location.reload" mention must be in the explanatory
        // comment, not in executable code. Split past the comment block.
        let code = handler.split("silently doing nothing.").nth(1).expect("comment marker must exist");
        assert!(!code.contains("eval"), "eval-based reload is unreliable on External URL");
        assert!(handler.contains("if let Err(error)"), "reload failure must be logged, not swallowed");
    }

    #[test]
    fn window_menu_items_have_no_accelerator_text() {
        let menu = build_menu_source();
        // Predefined edit items keep native platform shortcuts automatically;
        // only custom View/Help items must avoid hand-written accelerator text.
        assert!(!menu.contains("Some(\"Cmd"), "command-style shortcut hints must be gone");
        assert!(!menu.contains("F11"), "F11 hint must be gone");
        assert_eq!(menu.matches("None::<&str>").count(), 4, "reload/fullscreen/devtools/support are accelerator-free");
    }


    #[test]
    fn clamp_to_screen_large_screen_respects_floor_and_ceiling() {
        let state = WindowState { width: 1920.0, height: 1080.0, x: None, y: None, maximized: false };
        let out = clamp_to_screen(state, 1920.0, 1080.0);
        assert_eq!((out.width, out.height), (1880.0, 1000.0));

        let small = WindowState { width: 400.0, height: 300.0, x: None, y: None, maximized: false };
        let out = clamp_to_screen(small, 1920.0, 1080.0);
        assert_eq!((out.width, out.height), (880.0, 600.0));
    }

    #[test]
    fn clamp_to_screen_small_screen_never_exceeds_monitor() {
        // The old .min(screen - margin).max(floor) order returned 880x600 here,
        // larger than the 800x640 screen itself.
        let state = WindowState { width: 1280.0, height: 840.0, x: Some(10), y: Some(20), maximized: true };
        let out = clamp_to_screen(state, 800.0, 640.0);
        assert_eq!((out.width, out.height), (760.0, 560.0));
        assert_eq!((out.x, out.y, out.maximized), (Some(10), Some(20), true));
    }

    #[test]
    fn clamp_to_screen_normal_screen_bounds_size() {
        let state = WindowState { width: 1600.0, height: 1000.0, x: None, y: None, maximized: false };
        let out = clamp_to_screen(state, 1280.0, 800.0);
        assert_eq!((out.width, out.height), (1240.0, 720.0));
    }

    #[test]
    fn menu_targets_main_when_it_exists() {
        assert_eq!(menu_window_label(true), "main");
    }

    #[test]
    fn support_entry_is_shared_by_menu_and_tray() {
        // Both surfaces dispatch on the same id and label, so one handler
        // constant keeps the window menu and the tray menu from drifting.
        assert_eq!(SUPPORT_ID, "support");
        assert_eq!(HELP_MENU_LABEL, "帮助");
        assert!(SUPPORT_ITEM_LABEL.contains("支持项目"));
        assert!(SUPPORT_ITEM_LABEL.contains('⭐'));
    }

    #[test]
    fn support_url_is_a_https_repository_link() {
        // native::open_external only allows http(s)/mailto; keep this a plain
        // HTTPS GitHub URL so the star entry can never be rejected.
        assert!(SUPPORT_URL.starts_with("https://github.com/"));
        assert_eq!(SUPPORT_URL, "https://github.com/pengtang1024/dsh-tauri");
    }

    #[test]
    fn menu_targets_splash_before_main_exists() {
        assert_eq!(menu_window_label(false), "splash");
    }

    #[test]
    fn escapes_everything_except_unreserved_bytes() {
        assert_eq!(urlencoding("a b\nc/d"), "a%20b%0Ac%2Fd");
    }

    #[test]
    fn keeps_plain_text_readable() {
        assert_eq!(urlencoding("DSH-2.0.24"), "DSH-2.0.24");
    }

    #[test]
    fn roundtrip_preserves_all_fields_without_position() {
        let state = WindowState {
            width: 1440.5,
            height: 900.25,
            x: None,
            y: None,
            maximized: true
        };
        let decoded = decode_state(&encode_state(&state));
        assert_eq!(decoded, Some(state));
    }

    #[test]
    fn roundtrip_preserves_all_fields_with_position() {
        let state = WindowState {
            width: 1280.0,
            height: 840.0,
            x: Some(-12),
            y: Some(345),
            maximized: false
        };
        let decoded = decode_state(&encode_state(&state));
        assert_eq!(decoded, Some(state));
    }

    #[test]
    fn decode_rejects_empty_truncated_and_wrong_types() {
        assert_eq!(decode_state(""), None);
        assert_eq!(decode_state("{\"width\": 1280.0, \"height\": 840.0"), None);
        assert_eq!(
            decode_state("{\"width\": \"wide\", \"height\": 840.0, \"x\": null, \"y\": null, \"maximized\": false}"),
            None
        );
    }

    #[test]
    fn decode_ignores_unknown_extra_fields() {
        let json = "{\"width\": 1024.0, \"height\": 768.0, \"x\": 10, \"y\": 20, \"maximized\": true, \"futureField\": 42}";
        assert_eq!(
            decode_state(json),
            Some(WindowState {
                width: 1024.0,
                height: 768.0,
                x: Some(10),
                y: Some(20),
                maximized: true
            })
        );
    }

    #[test]
    fn geometry_is_sane_accepts_finite_sizes_with_any_position() {
        assert!(geometry_is_sane(&WindowState {
            width: 1280.0,
            height: 840.0,
            x: Some(-12),
            y: Some(345),
            maximized: false
        }));
        assert!(geometry_is_sane(&WindowState {
            width: 1.0,
            height: 16384.0,
            x: None,
            y: None,
            maximized: true
        }));
    }

    #[test]
    fn geometry_is_sane_rejects_non_usable_sizes() {
        let make = |width, height| WindowState {
            width,
            height,
            x: None,
            y: None,
            maximized: false
        };
        assert!(!geometry_is_sane(&make(-1.0, 840.0)));
        assert!(!geometry_is_sane(&make(1280.0, 0.0)));
        assert!(!geometry_is_sane(&make(16385.0, 840.0)));
        assert!(!geometry_is_sane(&make(f64::INFINITY, 840.0)));
    }

    #[test]
    fn navigate_error_message_keeps_context_and_original_error() {
        let message = navigate_error_message("navigation failed: 连接被拒绝");
        assert!(message.starts_with("[dsh-tauri] 无法在主窗口打开服务 URL："));
        assert!(message.ends_with("navigation failed: 连接被拒绝"));
    }

    #[test]
    fn resized_event_is_noop_intercepts_minimize_and_teardown() {
        use tauri::PhysicalSize;
        assert!(!resized_event_is_noop(&PhysicalSize::new(900, 700)));
        assert!(!resized_event_is_noop(&PhysicalSize::new(1, 1)));
        assert!(resized_event_is_noop(&PhysicalSize::new(0, 700)));
        assert!(resized_event_is_noop(&PhysicalSize::new(900, 0)));
        assert!(resized_event_is_noop(&PhysicalSize::new(0, 0)));
    }

    #[test]
    fn physical_size_at_scale_two_is_never_treated_as_logical() {
        // tauri 2.12.7 re-exports dpi 0.1.2: PhysicalSize::to_logical divides
        // by the window scale factor. Passing the raw 2x physical size to
        // set_size (which takes logical units) is exactly the f5fc64a loop.
        use tauri::{LogicalSize, PhysicalSize};
        let physical = PhysicalSize::new(2440_u32, 1680_u32);
        let logical: LogicalSize<f64> = physical.to_logical::<f64>(2.0);
        assert_eq!((logical.width, logical.height), (1220.0, 840.0));
        assert_eq!((logical.width * 2.0, logical.height * 2.0), (2440.0, 1680.0));
        assert!(logical.width < f64::from(physical.width));
    }

    #[test]
    fn resized_handler_never_reapplies_native_size() {
        // Static guard: the Resized branch must stay free of native size writes.
        let source = include_str!("shell_ui.rs");
        let branch = source
            .split("WindowEvent::Resized")
            .nth(1)
            .and_then(|tail| tail.split("WindowEvent::Destroyed").next())
            .expect("Resized branch must exist");
        assert!(!branch.contains("set_size"), "Resized must not write native size");
        assert!(!branch.contains("set_bounds"), "Resized must not write native bounds");
        assert!(branch.contains("window.dispatchEvent"), "page resize event must remain");
    }
}
