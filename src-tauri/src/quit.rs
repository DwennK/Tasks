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
    update: bool,
    committing: bool,
}

impl FlushRound {
    fn acknowledge(&mut self, generation: u64, label: &str) -> Result<bool, String> {
        if self.generation != generation {
            return Err("Quit was cancelled".into());
        }
        let pending = self.windows.as_mut().ok_or("Quit was cancelled")?;
        pending.remove(label);
        if !pending.is_empty() || self.waiting {
            return Ok(false);
        }
        self.waiting = true;
        Ok(true)
    }
    fn commit(&mut self, generation: u64) -> Result<bool, String> {
        if self.generation != generation
            || !self.waiting
            || self.committing
            || !self.windows.as_ref().is_some_and(HashSet::is_empty)
        {
            return Err("Save acknowledgements are incomplete or cancelled".into());
        }
        self.committing = true;
        Ok(self.update)
    }
    fn cancel(&mut self, generation: u64) -> bool {
        if self.generation != generation || self.windows.is_none() || self.committing {
            return false;
        }
        self.generation += 1;
        self.windows = None;
        self.waiting = false;
        true
    }
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
    request_action(app, false);
}

pub fn request_update(app: &tauri::AppHandle) -> bool {
    request_action(app, true)
}

fn request_action(app: &tauri::AppHandle, update: bool) -> bool {
    let windows = app.webview_windows();
    let state = app.state::<QuitState>();
    let generation = {
        let Ok(mut round) = state.round.lock() else {
            return false;
        };
        if round.windows.is_some() {
            return false;
        }
        round.generation += 1;
        round.windows = Some(windows.keys().cloned().collect());
        round.waiting = false;
        round.update = update;
        round.committing = false;
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
    true
}

fn cancel(app: &tauri::AppHandle, generation: u64, reason: &str) {
    let state = app.state::<QuitState>();
    if let Ok(mut round) = state.round.lock() {
        if !round.cancel(generation) {
            return;
        }
    } else {
        return;
    }

    crate::native_updates::cancelled(app);
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
        if !round.acknowledge(round_id, window.label())? {
            return Ok(());
        }
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
    let update = {
        let mut round = state.round.lock().map_err(|e| e.to_string())?;
        // Once commit starts, a late cancel cannot unlock editors.
        round.commit(generation)?
    };
    if update {
        let _ = app.emit("tasks-update-installing", ());
        if let Err(error) = crate::native_updates::install_after_flush(app.clone()).await {
            if let Ok(mut round) = state.round.lock() {
                round.committing = false;
            }
            cancel(&app, generation, &error);
            return Err(error);
        }
        state.allowed.store(true, Ordering::Release);
        app.restart();
    } else {
        state.allowed.store(true, Ordering::Release);
        app.exit(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn round() -> FlushRound {
        FlushRound {
            generation: 7,
            windows: Some(["main".into(), "settings".into()].into_iter().collect()),
            update: true,
            ..Default::default()
        }
    }
    #[test]
    fn installation_waits_for_every_window_and_commits_only_once() {
        let mut round = round();
        assert!(round.commit(7).is_err());
        assert!(!round.acknowledge(7, "main").unwrap());
        assert!(!round.acknowledge(7, "main").unwrap());
        assert!(round.commit(7).is_err());
        assert!(round.acknowledge(7, "settings").unwrap());
        assert!(round.commit(7).unwrap());
        assert!(!round.cancel(7));
        assert!(round.commit(7).is_err());
    }
    #[test]
    fn cancelled_or_stale_save_rounds_cannot_install() {
        let mut round = round();
        assert!(round.acknowledge(6, "main").is_err());
        assert!(!round.cancel(6));
        assert!(!round.acknowledge(7, "main").unwrap());
        assert!(round.cancel(7));
        assert!(round.acknowledge(7, "settings").is_err());
        assert!(round.commit(7).is_err());
    }
    #[test]
    fn native_activity_guards_remain_active_until_all_operations_finish() {
        let activities = Activities::default();
        let first = activities.start();
        let second = activities.start();
        assert_eq!(activities.0.load(Ordering::Acquire), 2);
        drop(first);
        assert_eq!(activities.0.load(Ordering::Acquire), 1);
        drop(second);
        assert_eq!(activities.0.load(Ordering::Acquire), 0);
    }
}
