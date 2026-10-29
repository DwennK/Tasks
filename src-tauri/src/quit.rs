use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

#[derive(Default)]
pub struct Activities(Arc<AtomicUsize>);

pub struct ActivityGuard(Arc<AtomicUsize>);

impl Activities {
    pub fn start(&self) -> ActivityGuard {
        self.0.fetch_add(1, Ordering::AcqRel);
        ActivityGuard(self.0.clone())
    }
}
impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Default)]
struct FlushRound {
    generation: u64,
    windows: Option<HashSet<String>>,
    waiting: bool,
}

#[derive(Default)]
pub struct QuitState {
    pub allowed: AtomicBool,
    round: Mutex<FlushRound>,
}

pub fn is_pending(app: &tauri::AppHandle) -> bool {
    let state = app.state::<QuitState>();
    state
        .round
        .lock()
        .map(|round| round.windows.is_some())
        .unwrap_or(true)
}

pub fn request(app: &tauri::AppHandle) {
    let windows = app.webview_windows();
    let state = app.state::<QuitState>();
    let generation = {
        let Ok(mut round) = state.round.lock() else {
            return;
        };
        if round.windows.is_some() {
            return;
        }
        round.generation += 1;
        round.windows = Some(windows.keys().cloned().collect());
        round.waiting = false;
        round.generation
    };
    // Each webview owns its own JavaScript save queue. Acknowledging only the
    // main editor would discard queued Settings changes in the other window.
    for (label, window) in windows {
        if label == "main" {
            let _ = window.show();
        }
        if window.emit("tasks-before-quit", generation).is_err() {
            cancel(
                app,
                generation,
                "Could not save all windows. Please try quitting again.",
            );
            break;
        }
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let pending = {
                let state = handle.state::<QuitState>();
                let Ok(round) = state.round.lock() else {
                    return;
                };
                if round.generation != generation {
                    return;
                }
                let Some(pending) = &round.windows else {
                    return;
                };
                if pending.is_empty() {
                    return;
                }
                pending.clone()
            };
            if Instant::now() >= deadline {
                cancel(
                    &handle,
                    generation,
                    "A window has not finished saving. Please try quitting again.",
                );
                return;
            }
            // A just-created webview may not yet have installed its listeners.
            // Repeat only for windows that have not acknowledged this round.
            for label in pending {
                if let Some(window) = handle.get_webview_window(&label) {
                    let _ = window.emit("tasks-before-quit", generation);
                }
            }
        }
    });
}

fn cancel(app: &tauri::AppHandle, generation: u64, reason: &str) {
    let state = app.state::<QuitState>();
    if let Ok(mut round) = state.round.lock() {
        if round.generation != generation || round.windows.is_none() {
            return;
        }
        round.generation += 1;
        round.windows = None;
        round.waiting = false;
    }
    let _ = app.emit("tasks-quit-cancelled", reason);
}

#[tauri::command]
pub fn cancel_quit(app: tauri::AppHandle, reason: String, round_id: u64) {
    cancel(
        &app,
        round_id,
        &reason.chars().take(500).collect::<String>(),
    );
}

#[tauri::command]
pub async fn finish_quit(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, QuitState>,
    activities: tauri::State<'_, Activities>,
    round_id: u64,
) -> Result<(), String> {
    let generation = {
        let mut round = state.round.lock().map_err(|e| e.to_string())?;
        if round.generation != round_id {
            return Err("Quit was cancelled".into());
        }
        let pending = round.windows.as_mut().ok_or("Quit was cancelled")?;
        pending.remove(window.label());
        if !pending.is_empty() || round.waiting {
            return Ok(());
        }
        round.waiting = true;
        round.generation
    };
    let active = activities.0.clone();
    // File pickers/imports and IPC operations can also be running independently
    // of the Vue task queue. Never terminate while a selected file is being saved.
    let drained = tauri::async_runtime::spawn_blocking(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while active.load(Ordering::Acquire) != 0 {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    })
    .await
    .map_err(|e| e.to_string())?;
    if !drained {
        cancel(
            &app,
            generation,
            "A file operation is still open. Finish or cancel it, then quit again.",
        );
        return Err("Pending file operation".into());
    }
    let round = state.round.lock().map_err(|e| e.to_string())?;
    if round.generation != generation || round.windows.is_none() {
        return Err("Quit was cancelled".into());
    }
    state.allowed.store(true, Ordering::Release);
    drop(round);
    app.exit(0);
    Ok(())
}
