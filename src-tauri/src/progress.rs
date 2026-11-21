//! Boot progress shared by the supervisor thread and the splash page.
//!
//! The splash has no event channel of its own: it simply polls the
//! `runtime_status` command, so one mutex-protected state is enough and no
//! script injection into the boot page is needed.

use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

/// What the shell is doing right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Locating,
    Installing,
    Starting,
    Ready,
    Updating,
    Error
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Locating => "locating",
            Stage::Installing => "installing",
            Stage::Starting => "starting",
            Stage::Ready => "ready",
            Stage::Updating => "updating",
            Stage::Error => "error"
        }
    }
}

struct State {
    stage: Stage,
    message: String,
    error: Option<String>,
    started: Instant
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn lock() -> MutexGuard<'static, State> {
    STATE
        .get_or_init(|| {
            Mutex::new(State {
                stage: Stage::Locating,
                message: "正在查找 DSH 运行时…".to_owned(),
                error: None,
                started: Instant::now()
            })
        })
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Enter a new stage with the text the splash should show.
pub fn set(stage: Stage, message: impl Into<String>) {
    let mut state = lock();
    state.stage = stage;
    state.message = message.into();
}

/// Record a fatal failure; `dsh::launch` then opens the error page.
pub fn fail(message: impl Into<String>) {
    let message = message.into();
    let mut state = lock();
    state.stage = Stage::Error;
    state.message = message.clone();
    state.error = Some(message);
}

/// Snapshot for the splash: stage, text, total elapsed seconds, last error.
pub fn snapshot() -> serde_json::Value {
    let state = lock();
    serde_json::json!({
        "stage": state.stage.as_str(),
        "message": state.message,
        "elapsedSecs": state.started.elapsed().as_secs(),
        "error": state.error
    })
}

/// Polled by `boot.html` (`window.__TAURI__.core.invoke('runtime_status')`).
#[tauri::command]
pub fn runtime_status() -> serde_json::Value {
    snapshot()
}

#[cfg(test)]
mod tests {
    use super::{runtime_status, set, Stage};

    #[test]
    fn reports_the_current_stage_and_message() {
        set(Stage::Installing, "正在安装");
        let status = runtime_status();
        assert_eq!(status["stage"], "installing");
        assert_eq!(status["message"], "正在安装");
        assert!(status["elapsedSecs"].is_u64());
    }
}
