//! Native directory chooser for `__DSH_DIRECTORY_PICKER__`.
//!
//! Why this is not zenity: zenity 4.2.1's file-selection mode only exposes
//! `--filename/--multiple/--directory/--save/--separator/--file-filter`
//! (verified with `zenity --help-file-selection` on this machine): it has
//! no `--show-hidden` and no path entry, so the two things users need most
//! — seeing dot-directories and pasting a path — cannot be expressed at all. This
//! module builds a GTK3 folder dialog ourselves: a `GtkFileChooserWidget` with
//! `show_hidden = true` plus a permanent path entry that accepts
//! `/some/dir`, `~/dir` or a relative path, jumps on Enter and can be
//! accepted directly.
//!
//! Thread model (no deadlock): Tauri's main thread *is* the GTK main thread, so
//! the dialog must be created and run there. The `pick_directory` command is
//! async and runs on the async runtime's worker, never on the GTK thread; it
//! posts the dialog through `AppHandle::run_on_main_thread` and waits on a
//! `std::sync::mpsc` channel from a *blocking pool* task, so no runtime worker
//! is parked either. `GtkDialog::run` spins a nested GLib main loop, which keeps
//! dispatching queued main-thread jobs while the dialog is open, and the dialog
//! closure never waits on the command side — the send happens after `run()`
//! returns. Nothing on the GTK thread blocks on a value produced by the waiting
//! task, which is exactly what would be needed to deadlock.
//!
//! The zenity path stays as the documented fallback: when the GTK dialog
//! cannot even be queued or returns an error, `native::pick_directory` logs it
//! and runs zenity instead of failing the pick.

use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;

use gtk::prelude::*;
use tauri::{AppHandle, Manager};

/// Environment switch that selects the implementation. `zenity` skips the GTK
/// dialog and exercises the fallback branch (used by the acceptance run and as
/// an escape hatch if a compositor breaks our dialog); anything else, including
/// unset, means "try GTK first".
pub(crate) const PICKER_ENV: &str = "DSH_TAURI_PICKER";

/// Which implementation a pick request is served from.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Gtk,
    Zenity,
}

/// Map `DSH_TAURI_PICKER` to an implementation. Pure so the fallback
/// decision is testable without touching the environment.
pub(crate) fn source_from_env(value: Option<&str>) -> Source {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("zenity") => Source::Zenity,
        _ => Source::Gtk,
    }
}

/// Collapse `//`, `/./` and `..` without touching the filesystem, so a
/// pasted path is returned in the same shape the user expects to see stored.
fn lexical_absolute(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            // At the root there is nothing left to pop: dropping the ".." keeps
            // the result absolute instead of inventing "/..".
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Turn one typed path into an absolute path: `~` and `~/x` expand against
/// `home`, relative paths join `cwd`. No filesystem access, so this is
/// unit-tested directly. Another user's `~name` is refused rather than guessed.
fn expand_input(input: &str, home: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("请输入目录路径".to_owned());
    }
    if trimmed.chars().any(char::is_control) {
        return Err("路径包含控制字符".to_owned());
    }
    let expanded = if trimmed == "~" {
        home.to_path_buf()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home.join(rest)
    } else if trimmed.starts_with('~') {
        return Err("只支持 ~ 与 ~/ 开头的路径，不支持其他用户的家目录".to_owned());
    } else {
        PathBuf::from(trimmed)
    };
    let absolute = if expanded.is_absolute() { expanded } else { cwd.join(expanded) };
    Ok(lexical_absolute(&absolute))
}

/// Resolve a typed path to an existing directory, or explain why it is not one.
fn resolve_dir(input: &str, home: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let path = expand_input(input, home, cwd)?;
    if path.is_dir() {
        Ok(path)
    } else if path.exists() {
        Err(format!("不是目录：{}", path.display()))
    } else {
        Err(format!("目录不存在：{}", path.display()))
    }
}

/// Hand a path back to the page as a string; a non-UTF-8 path is reported
/// instead of being silently mangled into a different directory.
fn path_to_string(path: PathBuf) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "目录路径不是有效 UTF-8".to_owned())
}

/// What `确定` returns. The typed path wins when present (that is the point of
/// the entry), then the chooser's selected folder, then the folder it shows.
fn selection_from(
    typed: &str,
    chosen: Option<&Path>,
    folder: Option<&Path>,
    home: &Path,
    cwd: &Path,
) -> Result<Option<String>, String> {
    let typed = typed.trim();
    if !typed.is_empty() {
        return resolve_dir(typed, home, cwd).and_then(|path| path_to_string(path).map(Some));
    }
    for candidate in [chosen, folder].into_iter().flatten() {
        if candidate.is_dir() {
            return path_to_string(candidate.to_path_buf()).map(Some);
        }
    }
    Ok(None)
}

/// Build and run the dialog on the GTK main thread, returning the picked path.
fn run_dialog(app: &AppHandle, title: &str) -> Result<Option<String>, String> {
    let parent = app
        .get_webview_window("main")
        .or_else(|| app.get_webview_window("splash"))
        .and_then(|window| window.gtk_window().ok());
    let dialog = gtk::Dialog::with_buttons(
        Some(title),
        parent.as_ref(),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[("取消", gtk::ResponseType::Cancel), ("确定", gtk::ResponseType::Accept)],
    );
    dialog.set_default_size(920, 620);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("粘贴或输入目录路径（支持 ~ 与相对路径），回车跳转"));
    entry.set_hexpand(true);
    // Enter in the entry must jump, never activate the default "确定" and close
    // the dialog behind the user's back.
    entry.set_activates_default(false);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.pack_start(&gtk::Label::new(Some("路径")), false, false, 0);
    row.pack_start(&entry, true, true, 0);
    let jump_button = gtk::Button::with_label("转到");
    row.pack_start(&jump_button, false, false, 0);
    let hint = gtk::Label::new(None);
    hint.set_xalign(0.0);
    content.pack_start(&row, false, false, 0);
    content.pack_start(&hint, false, false, 0);

    let chooser = gtk::FileChooserWidget::new(gtk::FileChooserAction::SelectFolder);
    // Requirement 1: dot-files/dot-directories are visible from the start.
    chooser.set_show_hidden(true);
    let start = crate::runtime::home_dir();
    let _ = chooser.set_current_folder(&start);
    content.pack_start(&chooser, true, true, 0);

    let jump: Rc<dyn Fn()> = {
        let chooser = chooser.clone();
        let entry = entry.clone();
        let hint = hint.clone();
        Rc::new(move || {
            let home = crate::runtime::home_dir();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            match resolve_dir(&entry.text(), &home, &cwd) {
                Ok(path) => {
                    if chooser.set_current_folder(&path) {
                        hint.set_text(&format!("当前目录：{}", path.display()));
                    } else {
                        hint.set_text(&format!("无法打开目录：{}", path.display()));
                    }
                }
                Err(message) => hint.set_text(&message),
            }
        })
    };
    {
        let jump = jump.clone();
        entry.connect_activate(move |_| jump());
    }
    {
        let jump = jump.clone();
        jump_button.connect_clicked(move |_| jump());
    }
    {
        // A typed-but-invalid path must not close the dialog: keep it open and
        // say why. Everything else lets the response through normally.
        let entry = entry.clone();
        let hint = hint.clone();
        dialog.connect_response(move |dialog, response| {
            if response != gtk::ResponseType::Accept || entry.text().trim().is_empty() {
                return;
            }
            let home = crate::runtime::home_dir();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            if let Err(message) = resolve_dir(&entry.text(), &home, &cwd) {
                hint.set_text(&message);
                dialog.stop_signal_emission_by_name("response");
            }
        });
    }

    dialog.show_all();
    // The path entry is the primary affordance: ready for paste/typing on open.
    entry.grab_focus();
    let response = dialog.run();
    let result = if response == gtk::ResponseType::Accept {
        let home = crate::runtime::home_dir();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        selection_from(
            &entry.text(),
            chooser.filename().as_deref(),
            chooser.current_folder().as_deref(),
            &home,
            &cwd,
        )
    } else {
        Ok(None)
    };
    // close() (not the unsafe destroy()) tears the dialog down after run().
    dialog.close();
    result
}

/// Queue the dialog on the GTK main thread and await its result.
///
/// `mpsc` + blocking-pool wait (rather than blocking the async worker)
/// keeps both the GTK loop and the runtime responsive; see the module docs for
/// why this cannot deadlock.
pub(crate) async fn choose(app: &AppHandle, title: String) -> Result<Option<String>, String> {
    let (sender, receiver) = mpsc::channel::<Result<Option<String>, String>>();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = run_dialog(&handle, &title);
        let _ = sender.send(result);
    })
    .map_err(|error| format!("无法把目录对话框排进主线程：{error}"))?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv())
        .await
        .map_err(|error| format!("目录对话框任务失败：{error}"))?
        .map_err(|error| format!("目录对话框结果通道断开：{error}"))?
}

#[cfg(test)]
mod tests {
    use super::{expand_input, resolve_dir, selection_from, source_from_env, Source};
    use std::path::PathBuf;

    fn home() -> PathBuf {
        PathBuf::from("/home/tester")
    }

    fn cwd() -> PathBuf {
        PathBuf::from("/work/repo")
    }

    #[test]
    fn source_defaults_to_gtk() {
        assert_eq!(source_from_env(None), Source::Gtk);
        assert_eq!(source_from_env(Some("")), Source::Gtk);
        assert_eq!(source_from_env(Some("gtk")), Source::Gtk);
    }

    #[test]
    fn source_honours_the_zenity_escape_hatch() {
        assert_eq!(source_from_env(Some("zenity")), Source::Zenity);
        assert_eq!(source_from_env(Some("  ZENITY ")), Source::Zenity);
        assert_eq!(source_from_env(Some("1")), Source::Gtk);
    }

    #[test]
    fn expands_a_bare_tilde_to_home() {
        assert_eq!(expand_input("~", &home(), &cwd()).unwrap(), home());
        assert_eq!(expand_input("  ~  ", &home(), &cwd()).unwrap(), home());
    }

    #[test]
    fn expands_a_tilde_prefix() {
        assert_eq!(
            expand_input("~/projects/dsh", &home(), &cwd()).unwrap(),
            PathBuf::from("/home/tester/projects/dsh")
        );
    }

    #[test]
    fn refuses_another_users_tilde() {
        let error = expand_input("~root/secrets", &home(), &cwd()).unwrap_err();
        assert!(error.contains('~'), "unexpected message: {error}");
    }

    #[test]
    fn joins_relative_paths_with_the_working_directory() {
        assert_eq!(
            expand_input("sub/dir", &home(), &cwd()).unwrap(),
            PathBuf::from("/work/repo/sub/dir")
        );
        assert_eq!(
            expand_input("./sub/../other", &home(), &cwd()).unwrap(),
            PathBuf::from("/work/repo/other")
        );
    }

    #[test]
    fn keeps_absolute_paths_as_written() {
        assert_eq!(
            expand_input("/srv/data", &home(), &cwd()).unwrap(),
            PathBuf::from("/srv/data")
        );
    }

    #[test]
    fn collapses_duplicate_and_dot_segments() {
        assert_eq!(
            expand_input("/srv//data/./logs/", &home(), &cwd()).unwrap(),
            PathBuf::from("/srv/data/logs")
        );
        assert_eq!(
            expand_input("/srv/data/../logs", &home(), &cwd()).unwrap(),
            PathBuf::from("/srv/logs")
        );
    }

    #[test]
    fn parent_dir_never_escapes_the_root() {
        assert_eq!(expand_input("/../..", &home(), &cwd()).unwrap(), PathBuf::from("/"));
        assert_eq!(expand_input("/a/../..", &home(), &cwd()).unwrap(), PathBuf::from("/"));
    }

    #[test]
    fn rejects_empty_and_control_character_input() {
        assert_eq!(expand_input("   ", &home(), &cwd()).unwrap_err(), "请输入目录路径");
        assert!(expand_input("/srv/a\nb", &home(), &cwd()).is_err());
        assert!(expand_input("/srv/a\u{7f}b", &home(), &cwd()).is_err());
    }

    #[test]
    fn resolve_dir_reports_missing_paths() {
        let error = resolve_dir("/definitely/not/here", &home(), &cwd()).unwrap_err();
        assert!(error.contains("目录不存在"), "unexpected message: {error}");
    }

    /// Unique directory under the system temp dir, removed by the caller.
    fn scratch(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "dsh-picker-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before the unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).expect("cannot create the test directory");
        base
    }

    #[test]
    fn resolve_dir_accepts_a_real_directory_and_refuses_a_file() {
        let base = scratch("resolve");
        let dir = base.join("real-dir");
        std::fs::create_dir_all(&dir).expect("cannot create the test directory");
        let file = base.join("plain-file");
        std::fs::write(&file, b"x").expect("cannot create the test file");

        let resolved = resolve_dir(dir.to_str().expect("test path is UTF-8"), &home(), &cwd());
        assert_eq!(resolved.unwrap(), dir);

        let file_error = resolve_dir(file.to_str().expect("test path is UTF-8"), &home(), &cwd())
            .expect_err("a file is not a directory");
        assert!(file_error.contains("不是目录"), "unexpected message: {file_error}");

        std::fs::remove_dir_all(&base).expect("cannot clean up the test directory");
    }

    #[test]
    fn selection_prefers_the_typed_path() {
        let base = scratch("typed");
        let typed = base.join("typed-dir");
        let chosen = base.join("chosen-dir");
        std::fs::create_dir_all(&typed).expect("cannot create the typed directory");
        std::fs::create_dir_all(&chosen).expect("cannot create the chosen directory");

        let selected = selection_from(
            typed.to_str().expect("test path is UTF-8"),
            Some(&chosen),
            None,
            &home(),
            &cwd(),
        );
        assert_eq!(selected.unwrap(), Some(typed.to_str().unwrap().to_owned()));

        std::fs::remove_dir_all(&base).expect("cannot clean up the test directory");
    }

    #[test]
    fn selection_refuses_an_invalid_typed_path() {
        let error = selection_from("/nope/nothing", None, None, &home(), &cwd()).unwrap_err();
        assert!(error.contains("目录不存在"), "unexpected message: {error}");
    }

    #[test]
    fn selection_skips_candidates_that_are_not_directories() {
        let base = scratch("skip");
        let file = base.join("plain-file");
        std::fs::write(&file, b"x").expect("cannot create the test file");

        let selected =
            selection_from("  ", Some(file.as_path()), Some(base.as_path()), &home(), &cwd());
        assert_eq!(
            selected.unwrap(),
            Some(base.to_str().expect("test path is UTF-8").to_owned())
        );

        std::fs::remove_dir_all(&base).expect("cannot clean up the test directory");
    }

    #[test]
    fn selection_is_none_without_any_candidate() {
        assert_eq!(selection_from("", None, None, &home(), &cwd()).unwrap(), None);
    }
}
