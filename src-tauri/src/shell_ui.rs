//! Native shell furniture: the window, its menu, the tray and remembered
//! geometry — everything around the DSH web UI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry};

/// Native menu ids/labels. The window menu owns the star entry; the tray menu
/// deliberately has its own show/quit actions only, so it is not duplicated.
/// Both menu handlers call plain Rust helpers, so they work before any window
/// exists and need no extra frontend capability.
const SUPPORT_ID: &str = "support";
const ABOUT_ID: &str = "about";
const DSH_MENU_LABEL: &str = "DSH";
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

/// Single-column native menu, designed around this project's workflow rather
/// than a generic desktop template. dsh-tauri is a launcher/diagnostic shell:
/// the real session lives in the WebView, so the native menu exposes only what
/// the WebView cannot do for itself and the OS does not already provide.
///
/// - 重新加载: the main window uses WebviewUrl::External and has no browser
///   refresh button; reload after a service restart or a background update.
/// - 开发者工具: WebKitGTK diagnostics for the web UI and the bridge. Always
///   visible and enabled — the `devtools` Cargo feature makes the API available
///   in release too, so no debug-only cfg, no env gate, no disabled state.
/// - 关于 dsh-tauri: shows this shell's own shell_info (runtime / home /
///   profile / desktopVersion), answering "which runtime am I running".
/// - 支持项目 ⭐ lives inside the about submenu, not as a top-level column and
///   not in the tray.
///
/// No Edit column (the upstream web UI owns its own clipboard/undo and Linux
/// undo/redo is not native here), no View/Window column or fullscreen (GTK F11
/// and the title bar already cover those).
pub fn build_menu<R: tauri::Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let reload = MenuItem::with_id(app, "reload", "重新加载", true, None::<&str>)?;
    let devtools = MenuItem::with_id(app, "devtools", "开发者工具", true, None::<&str>)?;
    let about = MenuItem::with_id(app, ABOUT_ID, "关于 dsh-tauri", true, None::<&str>)?;
    let support = MenuItem::with_id(app, SUPPORT_ID, SUPPORT_ITEM_LABEL, true, None::<&str>)?;
    // The star sits inside the about cluster, never as a top-level column.
    let about_cluster = Submenu::with_items(app, "关于", true, &[&about, &support])?;
    // Exactly one top-level column: everything the shell owns lives under DSH.
    let dsh = Submenu::with_items(app, DSH_MENU_LABEL, true, &[&reload, &devtools, &about_cluster])?;

    Menu::with_items(app, &[&dsh])
}

/// Native helpers the desktop shell leans on. Detection is a read-only PATH
/// lookup: no helper is invoked, so a missing one can only affect the panel
/// text, never the running session.
const NATIVE_HELPERS: [(&str, &str); 5] = [
    ("zenity", "关于/选取面板"),
    ("wl-copy", "剪贴板写入"),
    ("wl-paste", "剪贴板读取"),
    ("notify-send", "系统通知"),
    ("xdg-open", "外链打开")
];

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

/// Check whether a helper name is an executable on PATH without spawning a
/// process: empty entries are skipped and missing files mean "not found".
fn helper_in_path(name: &str, path_value: &str) -> bool {
    path_value.split(':').any(|dir| {
        if dir.is_empty() {
            return false;
        }
        fs::metadata(Path::new(dir).join(name))
            .map(|metadata| metadata.is_file() && is_executable(&metadata))
            .unwrap_or(false)
    })
}

/// Version strings come from an installed package.json (outside our control).
/// Keep only a conservative token so a malformed value cannot inject newlines,
/// markup or a path into the panel.
fn display_version(raw: Option<&str>) -> String {
    match raw {
        Some(value) if !value.is_empty() && value.len() <= 64 && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_')
        }) => value.to_owned(),
        _ => "不可用".to_owned()
    }
}

/// Everything the status panel reports beyond shell_info, gathered read-only.
struct StatusFacts {
    source: Option<&'static str>,
    updatable: bool,
    version: Option<String>,
    auto_update: bool,
    close_to_tray: bool,
    helpers: Vec<(&'static str, bool)>
}

fn status_facts(app: Option<&AppHandle<Wry>>) -> StatusFacts {
    let (source, updatable, version) = match crate::runtime::locate_existing_with_source(app) {
        Some((dir, from)) => (Some(from.label()), from.updatable(), crate::runtime::version_at(&dir)),
        None => (None, false, None)
    };
    let path_value = std::env::var("PATH").unwrap_or_default();
    let helpers = NATIVE_HELPERS
        .iter()
        .map(|(name, _)| (*name, helper_in_path(name, &path_value)))
        .collect();
    StatusFacts {
        source,
        updatable,
        version,
        auto_update: crate::runtime::auto_update_enabled(),
        close_to_tray: close_to_tray(),
        helpers
    }
}

fn auto_update_label(enabled: bool, updatable: bool) -> &'static str {
    match (enabled, updatable) {
        (false, _) => "关闭（DSH_TAURI_OFFLINE / DSH_TAURI_AUTO_UPDATE）",
        (true, false) => "开启，但当前档位不自动写入（需手动更新）",
        (true, true) => "开启（更新下次启动生效）"
    }
}

fn close_label(keep_alive: bool) -> &'static str {
    if keep_alive {
        "关窗=托盘保活（服务与会话继续运行）"
    } else {
        "关窗=退出"
    }
}

/// Status-panel geometry. The panel keeps one fixed width and every line is
/// pre-wrapped to `PANEL_COLS` display columns, so zenity never breaks a value
/// mid-word (the old panel let a 110-column runtime path wrap across three
/// lines at arbitrary characters).
const PANEL_COLS: usize = 64;
const PANEL_WIDTH: u32 = 500;
/// Widest label ("运行时来源"); every value therefore starts in one column.
const LABEL_COLS: usize = 10;
const VALUE_COL: usize = LABEL_COLS + 2;

const BRIDGE_SUMMARY: &str = "SHIM 协议 1；桌面对象未实现";
const BRIDGE_DOC: &str = "docs/DESKTOP_BRIDGE.zh.md";

/// Display columns of one character: CJK and fullwidth forms take two cells so
/// the budget matches what GTK actually renders for mixed CJK/ASCII lines.
fn char_cols(c: char) -> usize {
    let code = c as u32;
    let wide = matches!(
        code,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
    );
    if wide { 2 } else { 1 }
}

fn display_cols(text: &str) -> usize {
    text.chars().map(char_cols).sum()
}

/// Marker for a value whose head was dropped: a path keeps its tail, so the
/// readable part (module directory, file name) survives instead of a mid-word
/// break. The "/" is part of the marker so no "…//" can appear.
const HEAD_ELLIPSIS: &str = "…/";

/// Keep the tail of a too-long value, starting it on a path boundary.
fn elide_head(value: &str, budget: usize) -> String {
    if display_cols(value) <= budget {
        return value.to_owned();
    }
    let room = budget.saturating_sub(display_cols(HEAD_ELLIPSIS));
    let mut tail: Vec<char> = Vec::new();
    let mut used = 0;
    for c in value.chars().rev() {
        let cols = char_cols(c);
        if used + cols > room {
            break;
        }
        used += cols;
        tail.push(c);
    }
    tail.reverse();
    let tail: String = tail.into_iter().collect();
    // Drop the partially captured leading component so the tail reads as a
    // path rather than as a half-eaten directory name.
    let tail = match tail.find('/') {
        Some(index) if index + 1 < tail.len() => &tail[index + 1..],
        _ => tail.as_str()
    };
    format!("{HEAD_ELLIPSIS}{tail}")
}

/// Keep the head of a too-long prose value; paths use `elide_head` instead.
fn elide_tail(value: &str, budget: usize) -> String {
    if display_cols(value) <= budget {
        return value.to_owned();
    }
    let mut head = String::new();
    let mut used = 0;
    for c in value.chars() {
        let cols = char_cols(c);
        if used + cols + 1 > budget {
            break;
        }
        used += cols;
        head.push(c);
    }
    format!("{}…", head.trim_end())
}

/// Fit one value into a column budget: paths lose their head (the tail is what
/// identifies them), prose loses its tail.
fn fit_value(value: &str, budget: usize) -> String {
    if display_cols(value) <= budget {
        return value.to_owned();
    }
    if value.contains('/') { elide_head(value, budget) } else { elide_tail(value, budget) }
}

/// One aligned `label: value` fact line, never wider than `PANEL_COLS`.
fn fact_line(label: &str, value: &str) -> String {
    let pad = LABEL_COLS.saturating_sub(display_cols(label));
    let prefix = format!("{label}{}: ", " ".repeat(pad));
    let budget = PANEL_COLS.saturating_sub(display_cols(&prefix));
    format!("{prefix}{}", fit_value(value, budget))
}

/// Continuation lines sit under the value column.
fn value_indent() -> String {
    " ".repeat(VALUE_COL)
}

/// Pack comma-separated items under one label line, breaking between items so
/// no item is ever split mid-word. A wrap keeps the comma on the previous line
/// and indents the continuation one level deeper, so the list still reads as a
/// list instead of a bare orphaned word.
fn pack_items(prefix: &str, items: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = prefix.to_owned();
    let mut filled = false;
    for item in items {
        let token = if filled { format!(", {item}") } else { item.clone() };
        if !filled {
            current.push_str(&token);
            filled = true;
            continue;
        }
        if display_cols(&current) + display_cols(&token) <= PANEL_COLS {
            current.push_str(&token);
            continue;
        }
        let hung = format!("{current},");
        if display_cols(&hung) <= PANEL_COLS {
            lines.push(hung);
        } else {
            lines.push(current);
        }
        current = format!("{}  {item}", value_indent());
    }
    lines.push(current);
    lines
}

/// Native-helper summary: an "n/5 可用" line, the installed names, and the
/// missing ones with their purpose. The old panel crammed five
/// `name（purpose）=state` pairs onto one ~150-column line which zenity then
/// wrapped at arbitrary characters.
fn helper_lines(facts: &StatusFacts) -> Vec<String> {
    let installed: Vec<&str> = NATIVE_HELPERS
        .iter()
        .filter(|(name, _)| facts.helpers.iter().any(|(found, ok)| found == name && *ok))
        .map(|(name, _)| *name)
        .collect();
    let missing: Vec<String> = NATIVE_HELPERS
        .iter()
        .filter(|(name, _)| !installed.contains(name))
        .map(|(name, purpose)| format!("{name}（{purpose}）"))
        .collect();
    let mut lines = vec![fact_line(
        "原生助手",
        &format!("{}/{} 可用", installed.len(), NATIVE_HELPERS.len())
    )];
    if !installed.is_empty() {
        let names: Vec<String> = installed.iter().map(|name| (*name).to_owned()).collect();
        lines.extend(pack_items(&format!("{}可用: ", value_indent()), &names));
    }
    if !missing.is_empty() {
        lines.extend(pack_items(&format!("{}缺失: ", value_indent()), &missing));
    }
    lines
}

/// Build the status-panel text from shell_info plus read-only desktop facts.
/// Pure so the exact fields, alignment and per-line width can be tested without
/// a window.
fn about_text(info: &serde_json::Value, facts: &StatusFacts) -> String {
    let field = |key: &str| -> String {
        match info.get(key).and_then(serde_json::Value::as_str) {
            Some(value) if !value.is_empty() => value.to_owned(),
            _ => "未设置".to_owned()
        }
    };
    let mut lines = vec![format!("DSH Tauri {}", field("desktopVersion")), String::new()];
    lines.push(fact_line("运行时", &field("runtime")));
    lines.push(fact_line("运行时来源", facts.source.unwrap_or("未设置")));
    lines.push(fact_line("运行时版本", &display_version(facts.version.as_deref())));
    lines.push(fact_line("自动更新", auto_update_label(facts.auto_update, facts.updatable)));
    lines.push(fact_line("关闭窗口", close_label(facts.close_to_tray)));
    lines.extend(helper_lines(facts));
    lines.push(fact_line("桥接", BRIDGE_SUMMARY));
    lines.push(format!("{}文档: {BRIDGE_DOC}", value_indent()));
    lines.push(fact_line("主目录", &field("home")));
    lines.push(fact_line("配置", &field("profile")));
    lines.push(fact_line("桌面版本", &field("desktopVersion")));
    lines.join("\n")
}

/// Show shell_info plus this shell's desktop-specific state in a lightweight
/// native status panel. Tauri 2.12.7 itself has no dialog API (that lives in the
/// optional tauri-plugin-dialog crate, which is not a dependency here and must
/// not be added under this task's 0-invasion constraint). Use the system's
/// zenity --info helper, already present for the folder picker in native.rs,
/// with --no-markup so user-controlled paths are never parsed as Pango markup.
/// No external URL, no capability, no new dependency; failures are logged and
/// never block the menu.
fn show_about(app: &AppHandle<Wry>) {
    let info = crate::dsh::info(app);
    let text = about_text(&info, &status_facts(Some(app)));
    // spawn, not output(): menu events run on Tauri's main thread, so waiting
    // for the dialog to close would freeze the DSH window. zenity owns its own
    // GTK loop; stderr is inherited so dialog errors stay visible in logs.
    let width = format!("--width={PANEL_WIDTH}");
    if let Err(error) = Command::new("zenity")
        .args([
            "--info",
            "--title", "dsh-tauri 运行状态",
            "--no-markup",
            // No auto-wrap: every line is already wrapped to PANEL_COLS, and
            // GTK must never pick its own (mid-word) break points.
            "--no-wrap",
            &width,
            "--text", &text
        ])
        .spawn()
    {
        eprintln!("[dsh-tauri] 无法打开关于面板：{error}");
    }
}

/// Which window owns the shared menu: the real UI once it exists, otherwise the
/// boot splash. Pure so the choice can be tested without building an AppHandle.
fn menu_window_label(main_exists: bool) -> &'static str {
    if main_exists { "main" } else { "splash" }
}

/// Wire menu actions; unknown ids are ignored. Reload/devtools target the
/// owning window (main once it exists, otherwise the boot splash); about and
/// support are plain Rust helpers that work even before a window exists.
pub fn handle_menu_event(app: &AppHandle<Wry>, id: &str) {
    if id == SUPPORT_ID {
        if let Err(error) = crate::native::open_external(SUPPORT_URL.to_owned()) {
            eprintln!("[dsh-tauri] cannot open the support page: {error}");
        }
        return;
    }
    if id == ABOUT_ID {
        show_about(app);
        return;
    }
    // Remaining known ids are window actions; unknown ids are ignored.
    if id != "reload" && id != "devtools" {
        return;
    }
    let label = menu_window_label(app.get_webview_window("main").is_some());
    let Some(window) = app.get_webview_window(label) else { return };
    match id {
        "reload" => {
            // The main window is created at WebviewUrl::External(url); injecting
            // `window.location.reload()` via eval is not reliable on an external
            // origin. WebviewWindow::reload() reloads any URL through the native
            // WebView and re-runs the SHIM; log failures instead of swallowing.
            if let Err(error) = window.reload() {
                eprintln!("[dsh-tauri] 无法重新加载窗口：{error}");
            }
        }
        "devtools" => {
            // Always shown, always functional in release builds too: the
            // `devtools` Cargo feature makes this API available outside debug.
            window.open_devtools();
        }
        _ => {}
    }
}

/// Tray icon: left click restores the window, the menu offers show/quit only.
/// Session-keepalive lifecycle stays here; the star entry lives only in the
/// window's about cluster (no duplicate here).
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
    // Separate script on purpose: the SHIM injection above keeps its exact
    // contract, while this one is empty unless the DMABUF renderer is off. On
    // that software path backdrop-filter is ignored, so the menu surface has to
    // become near-opaque instead of a see-through block.
    let menu_fallback = crate::native::menu_fallback_injection(crate::dmabuf_renderer_disabled());
    if !menu_fallback.is_empty() {
        builder = builder.initialization_script(menu_fallback);
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

/// Readable error-page text shared by the navigate and build failure paths.
/// Kept pure so the wording and the original error are testable without a
/// live WebView.
fn show_url_failed_message(error: &str) -> String {
    format!("无法打开 DSH 界面：{error}\n\n请重试；如果仍然失败，在终端里手动运行 `dsh web` 查看完整日志。")
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
                // The splash must not keep spinning after a failed navigation.
                // Stay on this main thread: show_error builds a window and
                // Tauri only allows that from the main thread.
                let message = show_url_failed_message(&error.to_string());
                crate::progress::fail(message.clone());
                if let Err(error) = show_error(&handle, &message) {
                    eprintln!("[dsh-tauri] cannot show the error window: {error}");
                }
            }
            let _ = window.set_focus();
            return;
        }
        if let Err(error) = build_main_window(&handle, parsed) {
            eprintln!("[dsh-tauri] {error}");
            let message = show_url_failed_message(&error);
            crate::progress::fail(message.clone());
            if let Err(error) = show_error(&handle, &message) {
                eprintln!("[dsh-tauri] cannot show the error window: {error}");
            }
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
    use super::{about_text, auto_update_label, clamp_to_screen, close_label, decode_state, display_cols, display_version, elide_head, elide_tail, encode_state, fact_line, geometry_is_sane, helper_in_path, helper_lines, menu_window_label, navigate_error_message, resized_event_is_noop, show_url_failed_message, urlencoding, StatusFacts, WindowState, ABOUT_ID, DSH_MENU_LABEL, LABEL_COLS, PANEL_COLS, SUPPORT_ID, SUPPORT_ITEM_LABEL, SUPPORT_URL};

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

    /// Value of one `label: value` fact line; also asserts the label padding
    /// (the panel aligns every value in one column).
    fn fact_value<'a>(text: &'a str, label: &str) -> Option<&'a str> {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(label)?;
            rest.trim_start_matches(' ').strip_prefix(": ")
        })
    }

    /// The top-level menu row, i.e. the text inside Menu::with_items.
    fn top_as_binding(menu: &str) -> &str {
        menu.split("Menu::with_items(app, &[")
            .nth(1)
            .and_then(|tail| tail.split("])").next())
            .expect("top-level Menu::with_items must exist")
    }

    #[test]
    fn window_menu_is_one_workflow_driven_dsh_column() {
        let menu = build_menu_source();
        let top = top_as_binding(&menu);
        // One column named DSH: the menu is built from this project's workflow,
        // not from a generic Edit/View/Window/Help template.
        assert_eq!(top.trim(), "&dsh", "menu must have exactly the DSH column");
        assert!(!top.contains("&edit") && !top.contains("&view"), "no generic template columns");
        assert!(!top.contains("&window") && !top.contains("&tray"), "window/tray columns removed");
        assert!(menu.contains("Submenu::with_items(app, DSH_MENU_LABEL"),
            "the single column must be the DSH cluster");
        assert!(menu.contains("Submenu::with_items(app, \"关于\""), "about submenu must exist");
        for removed in ["全屏", "set_fullscreen", "PredefinedMenuItem"] {
            assert!(!menu.contains(removed), "{removed} must be gone (upstream/OS owns it)");
        }
    }

    #[test]
    fn menu_items_map_to_shell_workflows_without_duplicates() {
        let menu = build_menu_source();
        let column = menu
            .split("Submenu::with_items(app, DSH_MENU_LABEL")
            .nth(1)
            .and_then(|tail| tail.split("])").next())
            .expect("DSH submenu must exist");
        let cluster = menu
            .split("Submenu::with_items(app, \"关于\"")
            .nth(1)
            .and_then(|tail| tail.split("])").next())
            .expect("about submenu must exist");
        // Reload (external URL has no browser refresh) and devtools (WebKitGTK
        // bridge diagnosis) are window actions; about + star live in the cluster.
        for binding in ["&reload", "&devtools", "&about_cluster"] {
            assert!(column.contains(binding), "DSH column must reference {binding}");
        }
        for binding in ["&about", "&support"] {
            assert!(cluster.contains(binding), "about cluster must reference {binding}");
        }
        assert!(cluster.contains("&about") && cluster.contains("&support"),
            "about cluster must hold about + star");
        assert!(!cluster.contains("&reload") && !cluster.contains("&devtools"),
            "window actions belong directly in the DSH column");
        assert_eq!(ABOUT_ID, "about", "about id must match the handler literal");
        assert_eq!(SUPPORT_ID, "support", "support id must match the handler literal");
        // reload/devtools are plain literal ids; about/support use constants.
        for literal in ["reload", "devtools"] {
            assert_eq!(
                menu.matches(&format!("MenuItem::with_id(app, \"{literal}\"")).count(),
                1,
                "{literal} item must be created exactly once across the menu"
            );
        }
        for constant in ["ABOUT_ID", "SUPPORT_ID"] {
            assert_eq!(
                menu.matches(&format!("MenuItem::with_id(app, {constant}")).count(),
                1,
                "{constant} item must be created exactly once across the menu"
            );
        }
        assert_eq!(menu.matches(SUPPORT_ID).count(), 2, "support id: const definition + menu item only");
        // Star is in the about cluster only; tray keeps show/quit lifecycle.
        let tray = install_tray_source();
        assert!(!tray.contains(SUPPORT_ID), "tray must not repeat the star entry");
        assert!(tray.contains("\"show\"") && tray.contains("\"quit\""),
            "tray must keep show/quit untouched");
        assert!(!menu.contains("CmdOrCtrl") && !menu.contains("F11"),
            "no hand-written shortcut hints");
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
        let code = handler.split("instead of swallowing.").nth(1).expect("comment marker must exist");
        assert!(!code.contains("eval"), "eval-based reload is unreliable on External URL");
        assert!(handler.contains("if let Err(error)"), "reload failure must be logged, not swallowed");
    }

    #[test]
    fn window_menu_items_have_no_accelerator_text() {
        let menu = build_menu_source();
        // All four custom menu items are accelerator-free; GTK F11 covers
        // fullscreen, which is deliberately not a menu item here.
        assert!(!menu.contains("Some(\"Cmd"), "command-style shortcut hints must be gone");
        assert!(!menu.contains("F11"), "F11 hint must be gone");
        assert_eq!(menu.matches("None::<&str>").count(), 4, "reload/devtools/about/support are accelerator-free");
    }

    fn facts() -> StatusFacts {
        StatusFacts {
            source: Some("仓库 runtime/（checkout）"),
            updatable: false,
            version: Some("0.2.2-rc.5".to_owned()),
            auto_update: true,
            close_to_tray: true,
            helpers: vec![
                ("zenity", true), ("wl-copy", true), ("wl-paste", false),
                ("notify-send", true), ("xdg-open", true)
            ]
        }
    }

    #[test]
    fn about_panel_formats_shell_info_fields_with_fallback() {
        let info = serde_json::json!({
            "runtime": "/home/pt/.dsh/dsh-tauri/runtime",
            "home": "/home/pt/.dsh",
            "profile": "web",
            "desktopVersion": "0.5.0"
        });
        let text = about_text(&info, &facts());
        assert!(text.starts_with("DSH Tauri 0.5.0\n\n运行时"), "title, blank separator, then facts");
        assert_eq!(fact_value(&text, "运行时"), Some("/home/pt/.dsh/dsh-tauri/runtime"));
        assert_eq!(fact_value(&text, "运行时来源"), Some("仓库 runtime/（checkout）"));
        assert_eq!(fact_value(&text, "运行时版本"), Some("0.2.2-rc.5"));
        assert_eq!(fact_value(&text, "自动更新"), Some("开启，但当前档位不自动写入（需手动更新）"));
        assert_eq!(fact_value(&text, "关闭窗口"), Some("关窗=托盘保活（服务与会话继续运行）"));
        assert_eq!(fact_value(&text, "原生助手"), Some("4/5 可用"));
        assert!(text.contains("缺失: wl-paste（剪贴板读取）"), "missing helpers keep their purpose");
        assert_eq!(fact_value(&text, "桥接"), Some("SHIM 协议 1；桌面对象未实现"));
        assert!(text.contains("文档: docs/DESKTOP_BRIDGE.zh.md"));
        assert_eq!(fact_value(&text, "主目录"), Some("/home/pt/.dsh"));
        assert_eq!(fact_value(&text, "配置"), Some("web"));
        assert_eq!(fact_value(&text, "桌面版本"), Some("0.5.0"));
        assert!(text.lines().last().unwrap_or_default().starts_with("桌面版本"), "version closes the panel: {text:?}");
        // shell_info may carry JSON nulls; the panel must stay readable.
        let empty = about_text(&serde_json::json!({ "runtime": null, "home": "", "profile": null, "desktopVersion": null }), &facts());
        assert_eq!(empty.matches("未设置").count(), 5, "null/empty shell_info fields fall back (version shown twice)");
    }

    #[test]
    fn every_panel_line_fits_the_column_budget() {
        // The real path from the reported screenshot: one 110-column token that
        // zenity used to wrap across three lines at arbitrary characters.
        let info = serde_json::json!({
            "runtime": "/home/pt/Work/DshProjects/dsh-plugin/dsh-tauri/src-tauri/./runtime/node_modules/@deepseek-ai/dsh/lib/bin.js",
            "home": "/home/pt/Work/DshProjects/dsh-plugin/dsh-tauri/.tmp-profile",
            "profile": "web",
            "desktopVersion": "0.7.1"
        });
        let text = about_text(&info, &facts());
        for line in text.lines() {
            assert!(
                display_cols(line) <= PANEL_COLS,
                "line needs {} cols (budget {PANEL_COLS}): {line:?}",
                display_cols(line)
            );
        }
        assert_eq!(
            text.lines().filter(|line| line.contains("bin.js")).count(),
            1,
            "the runtime path must stay on one line, never wrap"
        );
        assert_eq!(
            text.lines().filter(|line| line.contains(".tmp-profile")).count(),
            1,
            "the home path must stay on one line too"
        );
    }

    #[test]
    fn long_paths_keep_their_identifiable_tail() {
        let real = "/home/pt/Work/DshProjects/dsh-plugin/dsh-tauri/src-tauri/./runtime/node_modules/@deepseek-ai/dsh/lib/bin.js";
        let elided = elide_head(real, 48);
        assert!(display_cols(&elided) <= 48, "elided value must fit: {elided}");
        assert!(elided.starts_with("…/"), "the dropped head is marked: {elided}");
        assert!(elided.ends_with("lib/bin.js"), "the file name survives: {elided}");
        assert!(!elided.contains("//"), "no doubled separator: {elided}");
        assert_eq!(elide_head("/home/pt/.dsh", 48), "/home/pt/.dsh", "short values pass through untouched");
        // Prose keeps its head, paths keep their tail.
        let long_prose = "开启（更新下次启动生效）且不会在本窗口重载";
        assert!(display_cols(&elide_tail(long_prose, 10)) <= 10);
        assert!(elide_tail(long_prose, 10).ends_with('…'));
    }

    #[test]
    fn fact_values_start_in_one_column() {
        let info = serde_json::json!({
            "runtime": "/home/pt/.dsh/dsh-tauri/runtime",
            "home": "/home/pt/.dsh",
            "profile": "web",
            "desktopVersion": "0.7.1"
        });
        let text = about_text(&info, &facts());
        let fact_lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.starts_with(' ') && line.contains(": "))
            .collect();
        assert_eq!(fact_lines.len(), 10, "ten labelled facts (title and continuations excluded)");
        for line in fact_lines.iter().skip(1) {
            let colon = line.find(": ").unwrap_or_else(|| panic!("label missing colon: {line:?}"));
            assert_eq!(display_cols(&line[..colon]), LABEL_COLS, "labels share one column: {line:?}");
        }
        assert_eq!(fact_line("配置", "web"), "配置      : web", "labels are padded, not ragged");
    }

    #[test]
    fn helper_summary_is_compressed_and_names_the_missing_purpose() {
        let lines = helper_lines(&facts());
        assert!(lines[0].contains("4/5 可用"), "count first: {lines:?}");
        assert!(lines.iter().any(|line| line.contains("可用: zenity, wl-copy")), "installed names listed: {lines:?}");
        assert!(lines.iter().any(|line| line.contains("缺失: wl-paste（剪贴板读取）")), "missing + purpose: {lines:?}");
        assert!(lines.iter().all(|line| !line.contains("=可用")), "old inline =state pairs are gone");
        assert!(lines.iter().all(|line| display_cols(line) <= PANEL_COLS), "helper block obeys the budget");
    }

    #[test]
    fn status_panel_falls_back_when_facts_are_empty() {
        let empty_facts = StatusFacts {
            source: None,
            updatable: false,
            version: None,
            auto_update: false,
            close_to_tray: false,
            helpers: vec![]
        };
        let text = about_text(&serde_json::Value::Null, &empty_facts);
        assert_eq!(fact_value(&text, "运行时来源"), Some("未设置"));
        assert_eq!(fact_value(&text, "运行时版本"), Some("不可用"));
        assert_eq!(fact_value(&text, "关闭窗口"), Some("关窗=退出"));
        assert_eq!(fact_value(&text, "原生助手"), Some("0/5 可用"));
        for (name, purpose) in [("zenity", "关于/选取面板"), ("wl-paste", "剪贴板读取"), ("xdg-open", "外链打开")] {
            assert!(text.contains(&format!("{name}（{purpose}）")), "missing helper {name} is reported");
        }
        assert!(!text.contains("data-platform"), "must never mention platform injection");
        assert!(!text.contains("dshDesktop ="), "must never inject the desktop global");
    }

    #[test]
    fn version_and_update_labels_are_conservative() {
        assert_eq!(display_version(Some("1.2.3")), "1.2.3");
        assert_eq!(display_version(Some("v2.0.0-rc.1+build.5")), "v2.0.0-rc.1+build.5");
        assert_eq!(display_version(Some("0.4.1\n[evil]")), "不可用");
        assert_eq!(display_version(Some("/home/pt/.ssh/id_rsa")), "不可用");
        assert_eq!(display_version(None), "不可用");
        assert_eq!(auto_update_label(false, true), "关闭（DSH_TAURI_OFFLINE / DSH_TAURI_AUTO_UPDATE）");
        assert_eq!(auto_update_label(true, false), "开启，但当前档位不自动写入（需手动更新）");
        assert_eq!(auto_update_label(true, true), "开启（更新下次启动生效）");
        assert_eq!(close_label(true), "关窗=托盘保活（服务与会话继续运行）");
        assert_eq!(close_label(false), "关窗=退出");
    }

    #[test]
    fn helper_detection_is_read_only_and_nonexistent_is_unavailable() {
        assert!(!helper_in_path("definitely-not-a-real-helper-xyz", "/usr/bin:/bin"));
        assert!(!helper_in_path("definitely-not-a-real-helper-xyz", ""));
        assert!(helper_in_path("sh", &std::env::var("PATH").unwrap_or_default()), "baseline shell must be on PATH");
        // No process is spawned by detection: the panel only reports presence.
        let source = menu_source();
        let block = source
            .split("fn helper_in_path")
            .nth(1)
            .and_then(|tail| tail.split("\n}\n").next())
            .expect("helper_in_path must exist");
        assert!(!block.contains("Command::new"), "detection must not invoke helpers");
        assert!(block.contains("fs::metadata"), "detection must only stat files");
    }

    #[test]
    fn about_uses_zenity_no_markup_and_no_new_plugin() {
        let source = menu_source();
        let show = source
            .split("fn show_about")
            .nth(1)
            .and_then(|tail| tail.split("\n}\n").next())
            .expect("show_about must exist");
        assert!(show.contains("crate::dsh::info(app)"), "about must show real shell_info");
        assert!(show.contains("Command::new(\"zenity\")"), "use the existing system helper");
        assert!(show.contains("--no-markup"), "untrusted paths must not be parsed as markup");
        assert!(show.contains("--no-wrap"), "GTK must never pick its own mid-word break points");
        assert!(show.contains("--width={PANEL_WIDTH}"), "the panel width comes from the shared constant");
        assert!(show.contains(".spawn()"), "dialog must not block the main thread");
        assert!(show.contains("if let Err(error)"), "dialog failure must be logged");
        let handler = source
            .split("ABOUT_ID =>")
            .nth(1)
            .and_then(|tail| tail.split("\n        }\n").next())
            .expect("about handler must exist");
        assert!(handler.contains("show_about(app)"), "about id must open the panel");
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
    fn support_lives_in_about_cluster_with_stable_id_and_label() {
        assert_eq!(SUPPORT_ID, "support");
        assert_eq!(ABOUT_ID, "about");
        assert_eq!(DSH_MENU_LABEL, "DSH");
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

    #[test]
    fn show_url_failed_message_keeps_error_and_manual_command() {
        let message = show_url_failed_message("cannot open the window: no display");
        assert!(message.contains("cannot open the window: no display"), "error lost: {message}");
        assert!(message.contains("dsh web"), "manual command missing: {message}");
    }

    #[test]
    fn show_url_failure_branches_replace_the_splash_with_an_error_page() {
        // Static guard: both failures inside show_url's main-thread closure
        // must mark the progress state as failed and call show_error there,
        // otherwise the splash keeps spinning with no visible explanation.
        let source = include_str!("shell_ui.rs");
        let start = source.find("pub fn show_url").expect("show_url must exist");
        let tail = &source[start..];
        let end = tail.find("\n/// Show a readable failure page").expect("show_url must end before show_error");
        let body = &tail[..end];

        let navigate = body
            .split("if let Err(error) = window.navigate(parsed)")
            .nth(1)
            .and_then(|tail| tail.split("let _ = window.set_focus()").next())
            .expect("navigate failure branch must exist");
        assert!(navigate.contains("show_error(&handle"), "navigate failure must show the error page");
        assert!(navigate.contains("progress::fail"), "navigate failure must end boot progress");

        let build = body
            .split("if let Err(error) = build_main_window(&handle, parsed)")
            .nth(1)
            .expect("build failure branch must exist");
        assert!(build.contains("show_error(&handle"), "build failure must show the error page");
        assert!(build.contains("progress::fail"), "build failure must end boot progress");
    }
}
