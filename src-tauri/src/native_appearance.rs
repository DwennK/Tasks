//! Native macOS presentation backed by the same persisted settings as every client.
//! Call `start` after Tauri's Ready event, which initializes the development icon.
use chrono::{Local, NaiveDate};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tasks_core::TaskService;
use tauri::menu::{MenuBuilder, MenuItem};
use tauri::{Emitter, Manager};

const RECOVERY_TRAY_ID: &str = "tasks-dock-recovery";
// Tao suppresses DockHide for one second after DockShow (macOS process
// transforms are asynchronous). Keep a small margin beyond that cooldown.
const DOCK_HIDE_COOLDOWN: Duration = Duration::from_millis(1100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DockAction {
    None,
    Show,
    Hide,
    Deferred,
}

#[derive(Default)]
struct DockTransition {
    visible: Option<bool>,
    last_show: Option<Instant>,
    hide_deadline: Option<Instant>,
}
impl DockTransition {
    fn plan(&mut self, desired: bool, now: Instant) -> DockAction {
        if desired {
            // A newer show request cancels every earlier deferred hide.
            self.hide_deadline = None;
            return if self.visible == Some(true) {
                DockAction::None
            } else {
                DockAction::Show
            };
        }
        if self.visible == Some(false) {
            self.hide_deadline = None;
            return DockAction::None;
        }
        if let Some(deadline) = self.last_show.map(|time| time + DOCK_HIDE_COOLDOWN) {
            if now < deadline {
                // Repeated requests coalesce against the last actual Show;
                // they must not extend the cooldown indefinitely.
                self.hide_deadline = Some(deadline);
                return DockAction::Deferred;
            }
        }
        self.hide_deadline = None;
        DockAction::Hide
    }
    fn applied(&mut self, visible: bool, now: Instant) {
        self.visible = Some(visible);
        self.hide_deadline = None;
        if visible {
            self.last_show = Some(now);
        }
    }
    fn hide_due(&self, now: Instant) -> bool {
        self.hide_deadline.is_some_and(|deadline| now >= deadline)
    }
}

#[derive(Default)]
pub struct AppearanceState {
    // Serializes snapshot reads and native application, preventing an older
    // worker read from overwriting a just-saved Settings change.
    gate: Mutex<()>,
    applied: Mutex<Option<Appearance>>,
    recovery_menu_registered: AtomicBool,
    dock: Mutex<DockTransition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Appearance {
    app_icon: String,
    show_dock_icon: bool,
    dock_count: String,
    badge_count: Option<i64>,
}
impl Appearance {
    fn response(&self, recovery_tray: bool, dock: &DockTransition) -> Value {
        json!({"app_icon":self.app_icon,"show_dock_icon":self.show_dock_icon,
            "dock_count":self.dock_count,"badge_count":self.badge_count,
            "recovery_tray":recovery_tray,"dock_pending":dock.hide_deadline.is_some(),"dock_visible":dock.visible})
    }
}

/// Input rows have already passed `tasks.list`, so archived/hidden lists,
/// independently scheduled subtasks and current list hierarchy follow the core.
/// This final pure projection excludes notes and maps Today/Overdue dates.
fn badge_count(tasks: &[Value], scope: &str, today: NaiveDate) -> Option<i64> {
    if scope == "none" {
        return None;
    }
    let count = tasks
        .iter()
        .filter(|task| {
            if task["is_note"].as_bool().unwrap_or(false)
                || !task["completed_at"].is_null()
                || !task["deleted_at"].is_null()
                || !task["wont_do_at"].is_null()
            {
                return false;
            }
            let date = task["due_date"]
                .as_str()
                .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok());
            match scope {
                "today" => date.is_some_and(|date| date <= today),
                "overdue" => date.is_some_and(|date| date < today),
                "inbox" => task["list_id"] == "inbox",
                "all" => true,
                _ => false,
            }
        })
        .count();
    // macOS clears the badge for zero. Keep the IPC representation explicit.
    if count == 0 {
        None
    } else {
        Some(i64::try_from(count).unwrap_or(i64::MAX))
    }
}

fn read_appearance(service: &TaskService) -> Result<Appearance, String> {
    let settings = service
        .execute("settings.get", json!({}))
        .map_err(|e| e.to_string())?;
    let app_icon = match settings["app_icon"].as_str() {
        Some("rounded") => "rounded",
        _ => "default",
    }
    .to_string();
    let show_dock_icon = settings["show_dock_icon"].as_bool().unwrap_or(true);
    let dock_count = settings["dock_count"]
        .as_str()
        .filter(|scope| ["none", "today", "inbox", "all", "overdue"].contains(scope))
        .unwrap_or("today")
        .to_string();
    let today = Local::now().date_naive();
    let count = if dock_count == "none" {
        None
    } else {
        let view = if dock_count == "overdue" {
            "today"
        } else {
            &dock_count
        };
        let rows = service
            .execute("tasks.list", json!({"view":view,"date":today.to_string()}))
            .map_err(|e| e.to_string())?;
        badge_count(
            rows.as_array()
                .ok_or("The task service returned an invalid task list")?,
            &dock_count,
            today,
        )
    };
    Ok(Appearance {
        app_icon,
        show_dock_icon,
        dock_count,
        badge_count: count,
    })
}

fn show_main(app: &tauri::AppHandle, action: Option<&str>) {
    if crate::quit::is_pending(app) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(action) = action {
            let _ = window.emit("tasks-command", action);
        }
    }
}

fn ensure_recovery_tray(app: &tauri::AppHandle) -> Result<(), String> {
    if app.tray_by_id(RECOVERY_TRAY_ID).is_some() {
        return Ok(());
    }
    if crate::quit::is_pending(app) {
        return Err("Tasks is saving before quitting.".into());
    }
    let item =
        |id, text| MenuItem::with_id(app, id, text, true, None::<&str>).map_err(|e| e.to_string());
    let menu = MenuBuilder::new(app)
        .item(&item("tasks-recovery-show", "Show Tasks")?)
        .item(&item("tasks-recovery-new", "New Task")?)
        .item(&item("tasks-recovery-settings", "Settings…")?)
        .separator()
        .item(&item("tasks-recovery-quit", "Quit Tasks")?)
        .build()
        .map_err(|e| e.to_string())?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("Tasks has no recovery tray icon")?;
    // Tauri menu listeners are application-global and outlive a tray handle.
    // Register only once so repeated Dock hide/show cycles cannot accumulate
    // duplicate New Task or Settings actions.
    let state = app.state::<AppearanceState>();
    if !state.recovery_menu_registered.swap(true, Ordering::AcqRel) {
        app.on_menu_event(|app, event| {
            if crate::quit::is_pending(app) {
                return;
            }
            match event.id().as_ref() {
                "tasks-recovery-show" => show_main(app, None),
                "tasks-recovery-new" => show_main(app, Some("quick_add")),
                "tasks-recovery-settings" => show_main(app, Some("settings")),
                "tasks-recovery-quit" => crate::quit::request(app),
                _ => {}
            }
        });
    }
    tauri::tray::TrayIconBuilder::with_id(RECOVERY_TRAY_ID)
        .icon(icon)
        .icon_as_template(false)
        .tooltip("Tasks")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn apply_icon(icon: &str) -> Result<(), String> {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let marker =
        MainThreadMarker::new().ok_or("App icons must be updated on the macOS main thread")?;
    let bytes: &[u8] = match icon {
        "rounded" => include_bytes!("../icons/app-rounded.png"),
        _ => include_bytes!("../icons/app-icon.png"),
    };
    let application = NSApplication::sharedApplication(marker);
    let data = NSData::with_bytes(bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or("Could not decode the Tasks application icon")?;
    // AppKit requires its application icon setter on the main thread. The
    // checked marker above and run_on_main_thread caller enforce that contract.
    unsafe { application.setApplicationIconImage(Some(&image)) };
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn apply_icon(_icon: &str) -> Result<(), String> {
    Ok(())
}

fn apply_on_main(app: &tauri::AppHandle, appearance: &Appearance) -> Result<Value, String> {
    if crate::quit::is_pending(app) {
        return Err("Tasks is saving before quitting.".into());
    }
    let state = app.state::<AppearanceState>();
    let previous = state.applied.lock().map_err(|e| e.to_string())?.clone();
    if previous.as_ref() == Some(appearance) {
        let dock = state.dock.lock().map_err(|e| e.to_string())?;
        let waiting = dock.hide_deadline.is_some() && !dock.hide_due(Instant::now());
        if dock.visible == Some(appearance.show_dock_icon) || waiting {
            return Ok(appearance.response(app.tray_by_id(RECOVERY_TRAY_ID).is_some(), &dock));
        }
    }
    if previous
        .as_ref()
        .is_none_or(|p| p.app_icon != appearance.app_icon)
    {
        apply_icon(&appearance.app_icon)?;
    }
    if !appearance.show_dock_icon {
        // Recovery exists during the cooldown and before the actual hide.
        ensure_recovery_tray(app)?;
    }
    let dock_action = state
        .dock
        .lock()
        .map_err(|e| e.to_string())?
        .plan(appearance.show_dock_icon, Instant::now());
    match dock_action {
        DockAction::Show => {
            #[cfg(target_os = "macos")]
            app.set_dock_visibility(true).map_err(|e| e.to_string())?;
            // Record after the synchronous main-thread Tao call, making our
            // deadline conservative relative to Tao's internal timestamp.
            state
                .dock
                .lock()
                .map_err(|e| e.to_string())?
                .applied(true, Instant::now());
        }
        DockAction::Hide => {
            #[cfg(target_os = "macos")]
            app.set_dock_visibility(false).map_err(|e| e.to_string())?;
            state
                .dock
                .lock()
                .map_err(|e| e.to_string())?
                .applied(false, Instant::now());
        }
        DockAction::Deferred | DockAction::None => {}
    }
    if appearance.show_dock_icon {
        // Includes cancellation of a deferred hide while Dock stayed visible.
        // Only remove recovery after a successful Show or known-visible state.
        drop(app.remove_tray_by_id(RECOVERY_TRAY_ID));
    }
    if previous.as_ref().is_none_or(|p| {
        p.badge_count != appearance.badge_count || p.show_dock_icon != appearance.show_dock_icon
    }) {
        let window = app
            .get_webview_window("main")
            .ok_or("The Tasks main window is unavailable")?;
        window
            .set_badge_count(appearance.badge_count)
            .map_err(|e| e.to_string())?;
    }
    *state.applied.lock().map_err(|e| e.to_string())? = Some(appearance.clone());
    let dock = state.dock.lock().map_err(|e| e.to_string())?;
    Ok(appearance.response(app.tray_by_id(RECOVERY_TRAY_ID).is_some(), &dock))
}

fn synchronize(app: &tauri::AppHandle, service: &TaskService) -> Result<Value, String> {
    let state = app.state::<AppearanceState>();
    let _gate = state.gate.lock().map_err(|e| e.to_string())?;
    if crate::quit::is_pending(app) {
        return Err("Tasks is saving before quitting.".into());
    }
    let appearance = read_appearance(service)?;
    // All AppKit mutation lives on the main thread; SQLite stays off it.
    let handle = app.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = sender.send(apply_on_main(&handle, &appearance));
    })
    .map_err(|e| e.to_string())?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Timed out updating the macOS Dock".to_string())?
}

#[tauri::command]
pub async fn sync_desktop_appearance(
    app: tauri::AppHandle,
    service: tauri::State<'_, TaskService>,
    activities: tauri::State<'_, crate::quit::Activities>,
) -> Result<Value, String> {
    let service = service.inner().clone();
    let activity = activities.start();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        synchronize(&app, &service)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn pending_hide_due(app: &tauri::AppHandle) -> bool {
    app.state::<AppearanceState>()
        .dock
        .lock()
        .map(|dock| dock.hide_due(Instant::now()))
        .unwrap_or(false)
}

/// Start once from RunEvent::Ready, after Tauri's own development-icon setup.
/// Polls only the cheap revision/date until either changes; caches actual
/// appearance values so ordinary task edits never reload the icon or tray.
pub fn start(app: tauri::AppHandle, stopping: Arc<AtomicBool>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("tasks-native-appearance".into())
        .spawn(move || {
            let service = app.state::<TaskService>().inner().clone();
            let mut last_key = None;
            let mut last_error = None;
            while !stopping.load(Ordering::Relaxed) {
                if !crate::quit::is_pending(&app) {
                    let revision = service
                        .execute("revision", json!({}))
                        .map_err(|e| e.to_string());
                    let result = match revision {
                        Ok(value) => {
                            let key = (
                                value["revision"].as_i64().unwrap_or(-1),
                                Local::now().date_naive(),
                            );
                            if last_key.as_ref() != Some(&key) || pending_hide_due(&app) {
                                // Retry reads the latest persisted preference;
                                // no delayed callback captures an obsolete hide.
                                synchronize(&app, &service).map(|_| {
                                    last_key = Some(key);
                                })
                            } else {
                                Ok(())
                            }
                        }
                        Err(error) => Err(error),
                    };
                    match result {
                        Ok(()) => last_error = None,
                        Err(error) => {
                            // A deferred retry can fail without a new database
                            // revision. Ensure the next worker tick retries it.
                            last_key = None;
                            if last_error.as_ref() != Some(&error) {
                                eprintln!("Could not apply Tasks desktop appearance: {error}");
                                last_error = Some(error);
                            }
                        }
                    }
                }
                for _ in 0..20 {
                    if stopping.load(Ordering::Relaxed) {
                        return;
                    }
                    // Always yield, even if a due retry failed before it could
                    // consume the deadline (e.g. a temporary database error).
                    std::thread::sleep(Duration::from_millis(100));
                    if !crate::quit::is_pending(&app) && pending_hide_due(&app) {
                        break;
                    }
                }
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_only_active_tasks_and_local_date_scope() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();
        let tasks = vec![
            json!({"list_id":"inbox","due_date":"2026-10-26"}),
            json!({"list_id":"work","due_date":"2026-10-27"}),
            json!({"list_id":"inbox","due_date":"2026-10-28"}),
            json!({"list_id":"inbox","due_date":null}),
            json!({"list_id":"work","due_date":"2026-10-27","is_note":true}),
            json!({"list_id":"inbox","due_date":"2026-10-27","completed_at":"done"}),
            json!({"list_id":"inbox","due_date":"2026-10-27","deleted_at":"trash"}),
            json!({"list_id":"inbox","due_date":"2026-10-27","wont_do_at":"skipped"}),
        ];
        assert_eq!(badge_count(&tasks, "today", date), Some(2));
        assert_eq!(badge_count(&tasks, "overdue", date), Some(1));
        assert_eq!(badge_count(&tasks, "inbox", date), Some(3));
        assert_eq!(badge_count(&tasks, "all", date), Some(4));
        assert_eq!(badge_count(&tasks, "none", date), None);
    }
    #[test]
    fn clears_zero_badge_and_rolls_over_by_local_calendar_date() {
        let tasks = vec![
            json!({"list_id":"work","due_date":"2026-10-28"}),
            json!({"due_date":"invalid"}),
        ];
        assert_eq!(
            badge_count(
                &tasks,
                "today",
                NaiveDate::from_ymd_opt(2026, 10, 27).unwrap()
            ),
            None
        );
        assert_eq!(
            badge_count(
                &tasks,
                "today",
                NaiveDate::from_ymd_opt(2026, 10, 28).unwrap()
            ),
            Some(1)
        );
        assert_eq!(
            badge_count(
                &tasks,
                "overdue",
                NaiveDate::from_ymd_opt(2026, 10, 28).unwrap()
            ),
            None
        );
        assert_eq!(
            badge_count(
                &tasks,
                "overdue",
                NaiveDate::from_ymd_opt(2026, 10, 29).unwrap()
            ),
            Some(1)
        );
        assert_eq!(
            badge_count(&[], "all", NaiveDate::from_ymd_opt(2026, 10, 29).unwrap()),
            None
        );
    }
    #[test]
    fn rapid_show_hide_is_deferred_then_applied_after_tao_cooldown() {
        let now = Instant::now();
        let mut dock = DockTransition::default();
        assert_eq!(dock.plan(true, now), DockAction::Show);
        dock.applied(true, now);
        assert_eq!(
            dock.plan(false, now + Duration::from_millis(50)),
            DockAction::Deferred
        );
        assert_eq!(dock.visible, Some(true)); // Never cache an ignored Hide as applied.
        let deadline = dock.hide_deadline;
        assert_eq!(
            dock.plan(false, now + Duration::from_millis(900)),
            DockAction::Deferred
        );
        assert_eq!(dock.hide_deadline, deadline); // Repeated hides coalesce.
        assert!(!dock.hide_due(now + Duration::from_millis(1099)));
        assert!(dock.hide_due(now + DOCK_HIDE_COOLDOWN));
        assert_eq!(dock.plan(false, now + DOCK_HIDE_COOLDOWN), DockAction::Hide);
        // If the native setter fails, no applied(false) is recorded and the
        // next pass still requests Hide even though the deadline was consumed.
        assert_eq!(dock.visible, Some(true));
        assert_eq!(
            dock.plan(false, now + DOCK_HIDE_COOLDOWN + Duration::from_millis(1)),
            DockAction::Hide
        );
        dock.applied(false, now + DOCK_HIDE_COOLDOWN);
        assert_eq!(dock.visible, Some(false));
        assert!(dock.hide_deadline.is_none());
        assert_eq!(
            dock.plan(false, now + Duration::from_secs(2)),
            DockAction::None
        );
    }
    #[test]
    fn newer_show_cancels_deferred_hide_and_next_cycle_gets_a_new_deadline() {
        let now = Instant::now();
        let mut dock = DockTransition::default();
        dock.applied(false, now);
        assert_eq!(
            dock.plan(true, now + Duration::from_millis(10)),
            DockAction::Show
        );
        dock.applied(true, now + Duration::from_millis(10));
        assert_eq!(
            dock.plan(false, now + Duration::from_millis(20)),
            DockAction::Deferred
        );
        assert_eq!(
            dock.plan(true, now + Duration::from_millis(50)),
            DockAction::None
        );
        assert!(dock.hide_deadline.is_none());
        assert!(!dock.hide_due(now + Duration::from_secs(2)));
        // A fresh hide after cooldown can execute immediately.
        assert_eq!(
            dock.plan(false, now + Duration::from_secs(2)),
            DockAction::Hide
        );
        dock.applied(false, now + Duration::from_secs(2));
        assert_eq!(
            dock.plan(true, now + Duration::from_secs(3)),
            DockAction::Show
        );
        dock.applied(true, now + Duration::from_secs(3));
        assert_eq!(
            dock.plan(false, now + Duration::from_millis(3050)),
            DockAction::Deferred
        );
        assert!(!dock.hide_due(now + Duration::from_millis(4000)));
        assert!(dock.hide_due(now + Duration::from_millis(4100)));
    }

    #[test]
    fn latest_visible_preference_wins_when_deferred_hide_is_due() {
        let now = Instant::now();
        let mut dock = DockTransition::default();
        dock.applied(true, now);
        assert_eq!(
            dock.plan(false, now + Duration::from_millis(20)),
            DockAction::Deferred
        );
        assert!(dock.hide_due(now + DOCK_HIDE_COOLDOWN));
        // The worker reads SQLite again at the deadline. A newly persisted
        // show preference cancels the due hide even without an earlier sync.
        assert_eq!(dock.plan(true, now + DOCK_HIDE_COOLDOWN), DockAction::None);
        assert_eq!(dock.visible, Some(true));
        assert!(dock.hide_deadline.is_none());
    }
}
