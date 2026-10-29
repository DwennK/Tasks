use crate::quit;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tasks_core::TaskService;
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

fn key(task: &Value) -> String {
    format!(
        "{}:{}:{}",
        task["id"], task["reminder_at"], task["reminder_generation"]
    )
}

fn show_popup(app: &tauri::AppHandle) -> Result<(), String> {
    if quit::is_pending(app) {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window("reminders") {
        return window.show().map_err(|e| e.to_string());
    }
    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        "reminders",
        tauri::WebviewUrl::App("index.html?reminders".into()),
    )
    .title("Task Reminders")
    .inner_size(420.0, 380.0)
    .min_inner_size(360.0, 280.0)
    .resizable(true)
    .maximizable(false)
    .minimizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false);
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let origin = monitor.position();
        let width = monitor.size().width as f64 / scale;
        builder = builder.position(
            origin.x as f64 / scale + (width - 444.0).max(0.0),
            origin.y as f64 / scale + 60.0,
        );
    }
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

fn publish(app: &tauri::AppHandle, pending: &Value) {
    let _ = app.emit("task-reminders-changed", pending);
    if pending.as_array().is_some_and(|items| items.is_empty()) {
        if let Some(window) = app.get_webview_window("reminders") {
            let _ = window.hide();
        }
    }
}

#[tauri::command]
pub async fn pending_task_reminders(
    service: tauri::State<'_, TaskService>,
) -> Result<Value, String> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service
            .execute("reminders.pending", json!({}))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn resolve_task_reminder(
    app: tauri::AppHandle,
    service: tauri::State<'_, TaskService>,
    activities: tauri::State<'_, quit::Activities>,
    input: Value,
) -> Result<Value, String> {
    if quit::is_pending(&app) {
        return Err("Tasks is saving before quitting.".into());
    }
    let service = service.inner().clone();
    let activity = activities.start();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let result = service
            .execute("reminders.resolve", input)
            .map_err(|e| e.to_string())?;
        let pending = service
            .execute("reminders.pending", json!({}))
            .map_err(|e| e.to_string())?;
        publish(&app, &pending);
        Ok(json!({"result":result,"pending":pending}))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn start(
    app: tauri::AppHandle,
    service: TaskService,
    stopping: Arc<AtomicBool>,
) -> Result<(), std::io::Error> {
    std::thread::Builder::new()
        .name("task-reminders".into())
        .spawn(move || {
            let mut visible_keys = HashSet::new();
            let mut last_payload = Value::Null;
            while !stopping.load(Ordering::Relaxed) {
                if !quit::is_pending(&app) {
                    let enabled = service
                        .execute("settings.get", json!({}))
                        .ok()
                        .and_then(|s| s["notifications"].as_bool())
                        .unwrap_or(false);
                    if enabled {
                        // Read first: idle polling does not write a revision or fill task history.
                        let due = service.execute("reminders.due", json!({}));
                        if matches!(&due, Ok(Value::Array(items)) if !items.is_empty()) {
                            let _activity = app.state::<quit::Activities>().start();
                            if quit::is_pending(&app) {
                                continue;
                            }
                            if let Ok(capture) = service.execute("reminders.capture", json!({})) {
                                for task in capture["captured"].as_array().into_iter().flatten() {
                                    // The durable popup remains available even if macOS denies banners.
                                    let title = task["title"].as_str().unwrap_or("Task reminder");
                                    let _ = app
                                        .notification()
                                        .builder()
                                        .title("Tasks")
                                        .body(title)
                                        .show();
                                }
                            }
                        }
                        if let Ok(pending) = service.execute("reminders.pending", json!({})) {
                            let keys: HashSet<String> =
                                pending.as_array().into_iter().flatten().map(key).collect();
                            let has_new = keys.iter().any(|key| !visible_keys.contains(key));
                            if pending != last_payload {
                                publish(&app, &pending);
                            }
                            if has_new {
                                if let Err(error) = show_popup(&app) {
                                    eprintln!("Could not open reminder controls: {error}");
                                } else {
                                    visible_keys = keys;
                                }
                            } else {
                                visible_keys = keys;
                            }
                            last_payload = pending;
                        }
                    } else {
                        visible_keys.clear();
                        last_payload = Value::Null;
                        if let Some(window) = app.get_webview_window("reminders") {
                            let _ = window.hide();
                        }
                    }
                }
                for _ in 0..15 {
                    if stopping.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        })?;
    Ok(())
}
