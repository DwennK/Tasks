#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod native_appearance;
mod native_files;
mod native_integrations;
mod native_window;
mod native_reminders;
mod native_sync;
mod quit;

use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tasks_core::TaskService;
use tauri::menu::{AboutMetadata, MenuBuilder, MenuItem, SubmenuBuilder};
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;

#[tauri::command]
async fn open_settings_window(
    app: tauri::AppHandle,
    section: Option<String>,
) -> Result<(), String> {
    if quit::is_pending(&app) {
        return Err("Tasks is saving before quitting.".into());
    }
    let section = section
        .as_deref()
        .filter(|s| {
            matches!(
                *s,
                "smart" | "notifications" | "date" | "appearance" | "more" | "hotkeys" | "about" | "sync"
            )
        })
        .unwrap_or("smart");
    if let Some(window) = app.get_webview_window("settings") {
        window
            .emit("settings-section", section)
            .map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
        return window.set_focus().map_err(|e| e.to_string());
    }
    let builder = tauri::WebviewWindowBuilder::new(
        &app,
        "settings",
        tauri::WebviewUrl::App(format!("index.html?settings&section={section}").into()),
    )
    .title("Settings")
    .inner_size(800.0, 680.0)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .center();
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(9.0, 9.0));
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn close_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        window.hide().map_err(|e| e.to_string())?;
    }
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn dispatch(
    service: tauri::State<'_, TaskService>,
    activities: tauri::State<'_, quit::Activities>,
    action: String,
    input: Value,
) -> Result<Value, String> {
    let service = service.inner().clone();
    let activity = activities.start();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        service
            .execute(&action, input)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn main() {
    let service = match TaskService::open(tasks_core::default_db_path()) {
        Ok(service) => service,
        Err(error) => {
            eprintln!("Could not open Tasks database: {error}");
            std::process::exit(1);
        }
    };
    let stopping = Arc::new(AtomicBool::new(false));
    let mut context = tauri::generate_context!();
    if let Err(error) = native_integrations::configure_startup(&mut context, &service) {
        eprintln!("Could not apply startup preferences: {error}");
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if native_integrations::is_autostart(args) { return; }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::Builder::new()
            .app_name("com.dwenn.tasks")
            .arg(native_integrations::AUTOSTART_ARG)
            .build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(service)
        .manage(quit::QuitState::default())
        .manage(quit::Activities::default())
        .manage(native_appearance::AppearanceState::default())
        .manage(native_integrations::IntegrationState::default())
        .manage(native_sync::CloudSyncState::default())
        .invoke_handler(tauri::generate_handler![
            dispatch, quit::finish_quit, quit::cancel_quit, open_settings_window, close_settings_window,
            native_reminders::pending_task_reminders,
            native_reminders::resolve_task_reminder,
            native_sync::cloud_sync_status,
            native_sync::cloud_sync_configure,
            native_sync::cloud_sync_connect,
            native_sync::cloud_sync_disconnect,
            native_sync::cloud_sync_run,
            native_sync::cloud_sync_cancel,
            native_files::open_notification_settings,
            native_files::open_external_link,
            native_files::import_attachments,
            native_files::save_attachment,
            native_files::open_attachment,
            native_files::export_text,
            native_files::copy_text,
            native_appearance::sync_desktop_appearance,
            native_integrations::set_desktop_integration_setting,
            native_integrations::sync_desktop_integrations,
            native_integrations::take_global_quick_add,
        ])
        .menu(|app| {
            // The predefined macOS Quit item calls NSApplication.terminate:
            // directly. Use an application-owned item so pending editor writes
            // are acknowledged before the process exits.
            let quit = MenuItem::with_id(app, "save-and-quit", "Quit Tasks", true, Some("CmdOrCtrl+Q"))?;
            let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let command = |id, title| MenuItem::with_id(app, id, title, true, None::<&str>);
            let file_menu = SubmenuBuilder::new(app, "File")
                .item(&command("quick_add", "Add Task")?)
                .item(&command("search", "Search")?)
                .build()?;
            let mut list_menu = SubmenuBuilder::new(app, "List");
            for (id, title) in [("all", "All"), ("today", "Today"), ("tomorrow", "Tomorrow"), ("upcoming", "Next 7 Days"), ("inbox", "Inbox"), ("completed", "Completed"), ("wont_do", "Won't Do"), ("trash", "Trash")] {
                list_menu = list_menu.item(&command(id, title)?);
            }
            let mut task_menu = SubmenuBuilder::new(app, "Task");
            for (id, title) in [("complete_task", "Complete Task"), ("set_date", "Set Date…"), ("clear_date", "Clear Date"), ("set_today", "Set Today"), ("set_tomorrow", "Set Tomorrow"), ("set_next_week", "Set Next Week"), ("priority_none", "Set No Priority"), ("priority_low", "Set Low Priority"), ("priority_medium", "Set Medium Priority"), ("priority_high", "Set High Priority"), ("pin_task", "Pin Task"), ("duplicate_task", "Duplicate Task"), ("print_task", "Print"), ("delete_task", "Delete Task")] {
                task_menu = task_menu.item(&command(id, title)?);
            }
            MenuBuilder::new(app)
                .item(&SubmenuBuilder::new(app, "Tasks")
                    .about(Some(AboutMetadata { name: Some("Tasks".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() }))
                    .separator().item(&settings).separator().services().separator().hide().hide_others().show_all().separator().item(&quit).build()?)
                .item(&file_menu)
                .item(&SubmenuBuilder::new(app, "Edit").undo().redo().separator().cut().copy().paste().select_all().build()?)
                .item(&list_menu.build()?)
                .item(&task_menu.build()?)
                .item(&SubmenuBuilder::new(app, "Window").minimize().maximize().fullscreen().separator().close_window().build()?)
                .build()
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "save-and-quit" {
                quit::request(app);
            } else if !quit::is_pending(app) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.emit("tasks-command", event.id().as_ref());
                }
            }
        })
        .setup(move |app| {
            let link_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                if event.urls().iter().any(|url| url.scheme() == "tasks" && url.host_str() == Some("task")) {
                    if let Some(window) = link_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Focused(true)) {
                native_window::layout_controls(window);
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                // macOS convention: closing a window keeps Tasks and reminders running.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(context)
        .expect("Could not start Tasks");
    app.run(move |handle, event| match event {
        tauri::RunEvent::Ready => {
            if let Some(window) = handle.get_webview_window("main") {
                native_window::layout_controls(&window.as_ref().window());
            }
            if let Err(error) = native_reminders::start(handle.clone(), handle.state::<TaskService>().inner().clone(), stopping.clone()) {
                eprintln!("Could not start task reminders: {error}");
            }
            if let Err(error) = native_appearance::start(handle.clone(), stopping.clone()) {
                eprintln!("Could not start desktop appearance updates: {error}");
            }
            if let Err(error) = native_integrations::start(handle.clone(), stopping.clone()) {
                eprintln!("Could not start desktop integration updates: {error}");
            }
            native_sync::start(handle.clone(), stopping.clone());
        }
        tauri::RunEvent::ExitRequested { api, .. } => {
            if !handle
                .state::<quit::QuitState>()
                .allowed
                .load(Ordering::Acquire)
            {
                api.prevent_exit();
                quit::request(handle);
            }
        }
        tauri::RunEvent::Exit => stopping.store(true, Ordering::Relaxed),
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        _ => {}
    });
}
