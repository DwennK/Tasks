//! Application updates are owned by Rust. The webview cannot choose an endpoint,
//! signing key, artifact URL or bytes. Installation joins the existing save barrier.
use crate::quit;
use serde::Serialize;
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tasks_core::TaskService;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const RELEASE_PREFIX: &str = "https://github.com/DwennK/Tasks/releases/download/";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    current_version: String,
    phase: String,
    version: Option<String>,
    notes: Option<String>,
    downloaded: u64,
    total: Option<u64>,
    error: Option<String>,
    checked_at: Option<String>,
    supported: bool,
    disabled_reason: Option<String>,
    automatic: bool,
}
struct Pending {
    status: Status,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}
pub struct UpdateState(Mutex<Pending>);

fn disabled_reason() -> Option<String> {
    if cfg!(debug_assertions) {
        Some("Updates are available in installed release builds.".into())
    } else if std::env::var_os("TASKS_DB").is_some() {
        Some("Updates are disabled for isolated database sessions.".into())
    } else if !cfg!(target_os = "macos") {
        Some("Application updates are currently available on macOS.".into())
    } else if std::env::current_exe()
        .ok()
        .is_none_or(|path| !path.to_string_lossy().contains(".app/Contents/MacOS/"))
    {
        Some("Install Tasks in Applications before updating.".into())
    } else {
        None
    }
}
impl Default for UpdateState {
    fn default() -> Self {
        let disabled_reason = disabled_reason();
        Self(Mutex::new(Pending {
            status: Status {
                current_version: env!("CARGO_PKG_VERSION").into(),
                phase: "idle".into(),
                version: None,
                notes: None,
                downloaded: 0,
                total: None,
                error: None,
                checked_at: None,
                supported: disabled_reason.is_none(),
                disabled_reason,
                automatic: true,
            },
            update: None,
            bytes: None,
        }))
    }
}
fn busy(phase: &str) -> bool {
    matches!(phase, "checking" | "downloading" | "saving" | "installing")
}
fn automatic(app: &tauri::AppHandle) -> bool {
    app.state::<TaskService>()
        .execute("settings.get", json!({}))
        .ok()
        .and_then(|settings| settings["automatic_update_checks"].as_bool())
        .unwrap_or(true)
}
fn publish(app: &tauri::AppHandle, pending: &Pending) {
    let _ = app.emit("tasks-update-status", &pending.status);
}
fn fail(app: &tauri::AppHandle, error: impl ToString) {
    if let Ok(mut pending) = app.state::<UpdateState>().0.lock() {
        pending.status.phase = "error".into();
        pending.status.error = Some(error.to_string().chars().take(600).collect());
        pending.bytes = None;
        publish(app, &pending);
    }
}
#[tauri::command]
pub fn update_status(app: tauri::AppHandle) -> Result<Status, String> {
    let mut status = app
        .state::<UpdateState>()
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .status
        .clone();
    status.automatic = automatic(&app);
    Ok(status)
}
#[tauri::command]
pub async fn update_set_automatic(app: tauri::AppHandle, enabled: bool) -> Result<Status, String> {
    if quit::is_pending(&app) {
        return Err("Tasks is saving before restarting.".into());
    }
    let activity = app.state::<quit::Activities>().start();
    let service = app.state::<TaskService>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        service
            .execute(
                "settings.set",
                json!({"key":"automatic_update_checks","value":enabled}),
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    update_status(app)
}
fn allowed_artifact(url: &str) -> bool {
    url.starts_with(RELEASE_PREFIX) && url.ends_with(".app.tar.gz")
}
#[tauri::command]
pub async fn check_for_updates(app: tauri::AppHandle) -> Result<Status, String> {
    {
        let state = app.state::<UpdateState>();
        let mut pending = state.0.lock().map_err(|e| e.to_string())?;
        if !pending.status.supported {
            return Err(pending.status.disabled_reason.clone().unwrap_or_default());
        }
        if quit::is_pending(&app) || busy(&pending.status.phase) || pending.status.phase == "ready"
        {
            return Err("Another update or save operation is in progress.".into());
        }
        pending.status.phase = "checking".into();
        pending.status.error = None;
        pending.update = None;
        pending.bytes = None;
        publish(&app, &pending);
    }
    let result = async {
        app.updater_builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?
            .check()
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    match result {
        Ok(update) => {
            if update
                .as_ref()
                .is_some_and(|u| !allowed_artifact(u.download_url.as_str()))
            {
                fail(
                    &app,
                    "The release points to an unexpected download location.",
                );
            } else {
                let state = app.state::<UpdateState>();
                let mut pending = state.0.lock().map_err(|e| e.to_string())?;
                pending.status.checked_at = Some(chrono::Utc::now().to_rfc3339());
                pending.status.phase = if update.is_some() {
                    "available"
                } else {
                    "current"
                }
                .into();
                pending.status.version = update.as_ref().map(|u| u.version.clone());
                pending.status.notes = update.as_ref().and_then(|u| u.body.clone());
                pending.update = update;
                publish(&app, &pending);
            }
        }
        Err(error) => fail(&app, format!("Could not check for updates. {error}")),
    }
    update_status(app)
}
#[tauri::command]
pub async fn download_update(app: tauri::AppHandle) -> Result<Status, String> {
    let update = {
        let state = app.state::<UpdateState>();
        let mut pending = state.0.lock().map_err(|e| e.to_string())?;
        if quit::is_pending(&app) || pending.status.phase != "available" {
            return Err("Check for an available update first.".into());
        }
        let mut update = pending.update.clone().ok_or("No update is available.")?;
        update.timeout = Some(Duration::from_secs(300));
        pending.status.phase = "downloading".into();
        pending.status.downloaded = 0;
        pending.status.total = None;
        publish(&app, &pending);
        update
    };
    let _activity = app.state::<quit::Activities>().start();
    let result = update
        .download(
            |chunk, total| {
                if let Ok(mut pending) = app.state::<UpdateState>().0.lock() {
                    pending.status.downloaded += chunk as u64;
                    pending.status.total = total;
                    publish(&app, &pending);
                }
            },
            || {},
        )
        .await;
    match result {
        Ok(bytes) => {
            let state = app.state::<UpdateState>();
            let mut pending = state.0.lock().map_err(|e| e.to_string())?;
            pending.bytes = Some(bytes);
            pending.status.phase = "ready".into();
            publish(&app, &pending);
        }
        Err(error) => fail(
            &app,
            format!("The update could not be downloaded or verified. {error}"),
        ),
    }
    update_status(app)
}
#[tauri::command]
pub fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    {
        let state = app.state::<UpdateState>();
        let mut pending = state.0.lock().map_err(|e| e.to_string())?;
        if quit::is_pending(&app) || pending.status.phase != "ready" || pending.bytes.is_none() {
            return Err("Download and verify an update before installing.".into());
        }
        pending.status.phase = "saving".into();
        publish(&app, &pending);
    }
    if !quit::request_update(&app) {
        cancelled(&app);
        return Err("Another save operation is already in progress.".into());
    }
    Ok(())
}
pub fn cancelled(app: &tauri::AppHandle) {
    if let Ok(mut pending) = app.state::<UpdateState>().0.lock() {
        if pending.status.phase == "saving" {
            pending.status.phase = "ready".into();
            publish(app, &pending);
        }
    }
}
/// Called only after every webview and native activity has finished saving.
pub async fn install_after_flush(app: tauri::AppHandle) -> Result<(), String> {
    let (update, bytes) = {
        let state = app.state::<UpdateState>();
        let mut pending = state.0.lock().map_err(|e| e.to_string())?;
        let update = pending
            .update
            .clone()
            .ok_or("No verified update is available.")?;
        let bytes = pending.bytes.take().ok_or("Download the update again.")?;
        pending.status.phase = "installing".into();
        publish(&app, &pending);
        (update, bytes)
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        update.install(bytes).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    if let Err(error) = &result {
        fail(&app, format!("Installation failed. {error}"));
    }
    result
}
pub fn start(app: tauri::AppHandle, stopping: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut next = Instant::now() + Duration::from_secs(30);
        while !stopping.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
            if Instant::now() < next {
                continue;
            }
            next = Instant::now() + Duration::from_secs(6 * 60 * 60);
            if !automatic(&app) || quit::is_pending(&app) {
                continue;
            }
            let can_check = app.state::<UpdateState>().0.lock().is_ok_and(|pending| {
                pending.status.supported
                    && matches!(pending.status.phase.as_str(), "idle" | "current" | "error")
            });
            if can_check {
                let _ = tauri::async_runtime::block_on(check_for_updates(app.clone()));
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifacts_are_limited_to_this_repository_and_update_format() {
        assert!(allowed_artifact(
            "https://github.com/DwennK/Tasks/releases/download/v0.2.1/Tasks_aarch64.app.tar.gz"
        ));
        for url in [
            "http://github.com/DwennK/Tasks/releases/download/v1/Tasks.app.tar.gz",
            "https://github.com/other/Tasks/releases/download/v1/Tasks.app.tar.gz",
            "https://github.com/DwennK/Tasks/releases/download/v1/Tasks.dmg",
            "https://github.com/DwennK/Tasks/releases/download/v1/Tasks.app.tar.gz?redirect=evil",
        ] {
            assert!(!allowed_artifact(url));
        }
    }
    #[test]
    fn busy_phases_cover_network_save_and_install_operations() {
        for phase in ["checking", "downloading", "saving", "installing"] {
            assert!(busy(phase));
        }
        for phase in ["idle", "current", "available", "ready", "error"] {
            assert!(!busy(phase));
        }
    }
    #[test]
    fn development_builds_never_offer_binary_updates() {
        assert!(disabled_reason().is_some());
        assert!(!UpdateState::default().0.lock().unwrap().status.supported);
    }
}
