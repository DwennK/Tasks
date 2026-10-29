//! Local login startup and opt-in global Quick Add / Show-Hide shortcuts.
//! Platform calls are serialized off the main thread; the plugins dispatch their
//! own AppKit work. Defaults never register a shortcut or enable launch at login.
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tasks_core::{validate_desktop_integration_setting, TaskService};
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub const AUTOSTART_ARG: &str = "--tasks-autostart";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Preferences {
    launch_at_login: bool,
    launch_minimized: bool,
    global_quick_add_shortcut: String,
    global_show_hide_shortcut: String,
}
impl Preferences {
    fn read(service: &TaskService) -> Result<Self, String> {
        let settings = service
            .execute("settings.get", json!({}))
            .map_err(|e| e.to_string())?;
        Self::from_settings(&settings)
    }
    fn from_settings(settings: &Value) -> Result<Self, String> {
        let prefs = Self {
            launch_at_login: settings["launch_at_login"].as_bool().unwrap_or(false),
            launch_minimized: settings["launch_minimized"].as_bool().unwrap_or(false),
            global_quick_add_shortcut: settings["global_quick_add_shortcut"]
                .as_str()
                .unwrap_or("")
                .into(),
            global_show_hide_shortcut: settings["global_show_hide_shortcut"]
                .as_str()
                .unwrap_or("")
                .into(),
        };
        Shortcuts::parse(&prefs)?;
        Ok(prefs)
    }
    fn with_setting(&self, key: &str, value: &Value) -> Result<Self, String> {
        validate_desktop_integration_setting(key, value).map_err(|e| e.to_string())?;
        let mut next = self.clone();
        match key {
            "launch_at_login" => next.launch_at_login = value.as_bool().unwrap(),
            "launch_minimized" => next.launch_minimized = value.as_bool().unwrap(),
            "global_quick_add_shortcut" => {
                next.global_quick_add_shortcut = value.as_str().unwrap().into()
            }
            "global_show_hide_shortcut" => {
                next.global_show_hide_shortcut = value.as_str().unwrap().into()
            }
            _ => unreachable!("validated integration key"),
        }
        Shortcuts::parse(&next)?;
        Ok(next)
    }
    fn response(&self) -> Value {
        json!({"launch_at_login":self.launch_at_login,"launch_minimized":self.launch_minimized,"global_quick_add_shortcut":self.global_quick_add_shortcut,"global_show_hide_shortcut":self.global_show_hide_shortcut})
    }
}

fn parse_shortcut(chord: &str) -> Result<Option<Shortcut>, String> {
    validate_desktop_integration_setting("global_quick_add_shortcut", &json!(chord))
        .map_err(|e| e.to_string())?;
    if chord.is_empty() {
        return Ok(None);
    }
    let native = chord
        .split('+')
        .map(|part| match part {
            "mod" => "CommandOrControl",
            "ctrl" => "Control",
            "alt" => "Alt",
            "shift" => "Shift",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+");
    native
        .parse()
        .map(Some)
        .map_err(|e| format!("Unsupported global shortcut: {e}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlobalAction {
    QuickAdd,
    ShowHide,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Shortcuts {
    quick_add: Option<Shortcut>,
    show_hide: Option<Shortcut>,
}
impl Shortcuts {
    fn parse(preferences: &Preferences) -> Result<Self, String> {
        let shortcuts = Self {
            quick_add: parse_shortcut(&preferences.global_quick_add_shortcut)?,
            show_hide: parse_shortcut(&preferences.global_show_hide_shortcut)?,
        };
        if shortcuts.quick_add.is_some() && shortcuts.quick_add == shortcuts.show_hide {
            return Err(
                "Quick Add and Show/Hide Main Window must use different global shortcuts.".into(),
            );
        }
        Ok(shortcuts)
    }
    fn registered(self) -> Vec<Shortcut> {
        [self.quick_add, self.show_hide]
            .into_iter()
            .flatten()
            .collect()
    }
    fn packed(self) -> u64 {
        u64::from(self.quick_add.map_or(0, |shortcut| shortcut.id()))
            | (u64::from(self.show_hide.map_or(0, |shortcut| shortcut.id())) << 32)
    }
    fn available(self, registered: &[Shortcut]) -> Self {
        Self {
            quick_add: self
                .quick_add
                .filter(|shortcut| registered.contains(shortcut)),
            show_hide: self
                .show_hide
                .filter(|shortcut| registered.contains(shortcut)),
        }
    }
}

fn active_action(packed: u64, shortcut_id: u32) -> Option<GlobalAction> {
    if shortcut_id != 0 && packed as u32 == shortcut_id {
        Some(GlobalAction::QuickAdd)
    } else if shortcut_id != 0 && (packed >> 32) as u32 == shortcut_id {
        Some(GlobalAction::ShowHide)
    } else {
        None
    }
}

#[derive(Default)]
struct Applied {
    shortcuts: Shortcuts,
    // Track actual successful OS calls, including inactive leftovers if cleanup
    // failed. The next synchronization retries cleanup or missing registration.
    registered: Vec<Shortcut>,
    preferences: Option<Preferences>,
}

#[derive(Default)]
pub struct IntegrationState {
    gate: Mutex<Applied>,
    active_shortcuts: AtomicU64,
    pending_quick_add: AtomicBool,
}

trait Platform {
    fn autostart_enabled(&self) -> Result<bool, String>;
    fn set_autostart(&self, enabled: bool) -> Result<(), String>;
    fn register(&self, shortcut: Shortcut) -> Result<(), String>;
    fn unregister(&self, shortcut: Shortcut) -> Result<(), String>;
    fn activate(&self, shortcuts: Shortcuts);
}

fn reconcile_registrations(
    platform: &impl Platform,
    applied: &mut Applied,
    desired: Shortcuts,
) -> Result<(), String> {
    let desired = desired.registered();
    // Register replacements first so an OS collision preserves existing keys.
    // Comparing physical keys also permits switching the two action bindings.
    for shortcut in &desired {
        if !applied.registered.contains(shortcut) {
            platform.register(*shortcut)?;
            applied.registered.push(*shortcut);
        }
    }
    for shortcut in applied.registered.clone() {
        if !desired.contains(&shortcut) {
            platform.unregister(shortcut)?;
            applied
                .registered
                .retain(|registered| *registered != shortcut);
        }
    }
    Ok(())
}

fn replace_shortcuts(
    platform: &impl Platform,
    applied: &mut Applied,
    next: Shortcuts,
) -> Result<(), String> {
    let previous = applied.shortcuts;
    if let Err(error) = reconcile_registrations(platform, applied, next) {
        let rollback = reconcile_registrations(platform, applied, previous);
        // A failed rollback must never activate an unused replacement, and a
        // subsequent synchronization must retry instead of considering it done.
        applied.shortcuts = previous.available(&applied.registered);
        platform.activate(applied.shortcuts);
        return Err(match rollback {
            Ok(()) => error,
            Err(restore) => {
                applied.preferences = None;
                format!("{error} Could not restore the previous global shortcuts: {restore}")
            }
        });
    }
    applied.shortcuts = next;
    platform.activate(next);
    Ok(())
}

fn restore_shortcuts(
    platform: &impl Platform,
    applied: &mut Applied,
    previous: Shortcuts,
) -> Result<(), String> {
    let restored = reconcile_registrations(platform, applied, previous);
    // After persistence fails, a replacement must never remain active even if
    // the OS cannot restore an old registration. Retain actual leftovers for
    // cleanup and dispatch only the old shortcuts that could be restored.
    applied.shortcuts = previous.available(&applied.registered);
    platform.activate(applied.shortcuts);
    applied.preferences = None;
    restored
}

fn needs_synchronization(applied: &Applied, last_revision: Option<i64>, revision: i64) -> bool {
    last_revision != Some(revision) || applied.preferences.is_none()
}

fn apply_and_save(
    platform: &impl Platform,
    applied: &mut Applied,
    preferences: &Preferences,
    refresh_login_path: bool,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let next = Shortcuts::parse(preferences)?;
    let old_shortcuts = applied.shortcuts;
    let old_autostart = platform.autostart_enabled()?;
    replace_shortcuts(platform, applied, next)?;
    let changed_autostart = old_autostart != preferences.launch_at_login;
    let needs_login_write =
        changed_autostart || (refresh_login_path && preferences.launch_at_login);
    let result = (|| {
        if needs_login_write {
            platform.set_autostart(preferences.launch_at_login)?;
        }
        save()
    })();
    if let Err(error) = result {
        let mut errors = vec![error];
        if needs_login_write {
            if let Err(restore) = platform.set_autostart(old_autostart) {
                errors.push(format!("Could not restore launch at login: {restore}"));
            }
        }
        if let Err(restore) = restore_shortcuts(platform, applied, old_shortcuts) {
            errors.push(format!(
                "Could not restore the previous global shortcuts: {restore}"
            ));
        }
        applied.preferences = None;
        return Err(errors.join(" "));
    }
    applied.preferences = Some(preferences.clone());
    Ok(())
}

trait MainWindow {
    fn visible(&self) -> Result<bool, String>;
    fn focused(&self) -> Result<bool, String>;
    fn minimized(&self) -> Result<bool, String>;
    fn hide(&self) -> Result<(), String>;
    fn unminimize(&self) -> Result<(), String>;
    fn show(&self) -> Result<(), String>;
    fn focus(&self) -> Result<(), String>;
}
impl MainWindow for tauri::WebviewWindow {
    fn visible(&self) -> Result<bool, String> {
        self.is_visible().map_err(|e| e.to_string())
    }
    fn focused(&self) -> Result<bool, String> {
        self.is_focused().map_err(|e| e.to_string())
    }
    fn minimized(&self) -> Result<bool, String> {
        self.is_minimized().map_err(|e| e.to_string())
    }
    fn hide(&self) -> Result<(), String> {
        tauri::WebviewWindow::hide(self).map_err(|e| e.to_string())
    }
    fn unminimize(&self) -> Result<(), String> {
        tauri::WebviewWindow::unminimize(self).map_err(|e| e.to_string())
    }
    fn show(&self) -> Result<(), String> {
        tauri::WebviewWindow::show(self).map_err(|e| e.to_string())
    }
    fn focus(&self) -> Result<(), String> {
        self.set_focus().map_err(|e| e.to_string())
    }
}
fn show_main_window(window: &impl MainWindow) -> Result<(), String> {
    window.unminimize()?;
    window.show()?;
    window.focus()
}
fn toggle_main_window(window: &impl MainWindow) -> Result<(), String> {
    if window.visible()? && window.focused()? && !window.minimized()? {
        window.hide()
    } else {
        show_main_window(window)
    }
}

struct NativePlatform<'a>(&'a tauri::AppHandle);
fn login_item_allowed(
    executable: &std::path::Path,
    custom_database: bool,
    development: bool,
) -> bool {
    !custom_database
        && !development
        && executable
            .to_string_lossy()
            .contains(".app/Contents/MacOS/")
}
fn can_manage_login_item() -> bool {
    std::env::current_exe().is_ok_and(|path| {
        login_item_allowed(
            &path,
            std::env::var_os("TASKS_DB").is_some(),
            cfg!(debug_assertions),
        )
    })
}
impl Platform for NativePlatform<'_> {
    fn autostart_enabled(&self) -> Result<bool, String> {
        // Dev/custom-DB runs must never reconcile the production login item.
        if !can_manage_login_item() {
            return Ok(false);
        }
        self.0.autolaunch().is_enabled().map_err(|e| e.to_string())
    }
    fn set_autostart(&self, enabled: bool) -> Result<(), String> {
        if !can_manage_login_item() {
            return if enabled {
                Err("Launch at Login is available in the packaged release app using its default database.".into())
            } else {
                // Failed isolated enable operations also cannot disable the
                // production item while restoring their previous false value.
                Ok(())
            };
        }
        if enabled {
            self.0.autolaunch().enable()
        } else {
            self.0.autolaunch().disable()
        }
        .map_err(|e| format!("Could not change Launch at Login: {e}"))
    }
    fn register(&self, shortcut: Shortcut) -> Result<(), String> {
        self.0
            .global_shortcut()
            .on_shortcut(shortcut, |app, shortcut, event| {
                let state = app.state::<IntegrationState>();
                let action = active_action(
                    state.active_shortcuts.load(Ordering::Acquire),
                    shortcut.id(),
                );
                if event.state != ShortcutState::Pressed
                    || action.is_none()
                    || crate::quit::is_pending(app)
                {
                    return;
                }
                let action = action.unwrap();
                let id = shortcut.id();
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    let state = handle.state::<IntegrationState>();
                    if crate::quit::is_pending(&handle)
                        || active_action(state.active_shortcuts.load(Ordering::Acquire), id)
                            != Some(action)
                    {
                        return;
                    }
                    if let Some(window) = handle.get_webview_window("main") {
                        let result = match action {
                            GlobalAction::QuickAdd => {
                                state.pending_quick_add.store(true, Ordering::Release);
                                let result = show_main_window(&window);
                                let _ = window.emit("global-quick-add", ());
                                result
                            }
                            GlobalAction::ShowHide => toggle_main_window(&window),
                        };
                        if let Err(error) = result {
                            let _ = handle.emit("desktop-integration-error", error);
                        }
                    }
                });
            })
            .map_err(|e| {
                format!(
                    "Could not register the global shortcut. It may be used by another app: {e}"
                )
            })
    }
    fn unregister(&self, shortcut: Shortcut) -> Result<(), String> {
        self.0
            .global_shortcut()
            .unregister(shortcut)
            .map_err(|e| format!("Could not unregister the global shortcut: {e}"))
    }
    fn activate(&self, shortcuts: Shortcuts) {
        self.0
            .state::<IntegrationState>()
            .active_shortcuts
            .store(shortcuts.packed(), Ordering::Release);
    }
}

fn synchronize(app: &tauri::AppHandle, service: &TaskService) -> Result<Value, String> {
    let state = app.state::<IntegrationState>();
    let mut applied = state.gate.lock().map_err(|e| e.to_string())?;
    if crate::quit::is_pending(app) {
        return Err("Tasks is saving before quitting.".into());
    }
    let preferences = Preferences::read(service)?;
    if applied.preferences.as_ref() != Some(&preferences) {
        let refresh_login_path = applied.preferences.is_none();
        apply_and_save(
            &NativePlatform(app),
            &mut applied,
            &preferences,
            refresh_login_path,
            || Ok(()),
        )?;
    }
    Ok(preferences.response())
}

#[tauri::command]
pub async fn sync_desktop_integrations(
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

#[tauri::command]
pub async fn set_desktop_integration_setting(
    app: tauri::AppHandle,
    service: tauri::State<'_, TaskService>,
    activities: tauri::State<'_, crate::quit::Activities>,
    key: String,
    value: Value,
) -> Result<Value, String> {
    let service = service.inner().clone();
    let activity = activities.start();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let state = app.state::<IntegrationState>();
        let mut applied = state.gate.lock().map_err(|e| e.to_string())?;
        if crate::quit::is_pending(&app) {
            return Err("Tasks is saving before quitting.".into());
        }
        let preferences = Preferences::read(&service)?.with_setting(&key, &value)?;
        apply_and_save(
            &NativePlatform(&app),
            &mut applied,
            &preferences,
            false,
            || {
                service
                    .execute("settings.set", json!({"key":key,"value":value}))
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        )?;
        Ok(preferences.response())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Drain after installing the global-quick-add listener, then whenever it fires.
/// This keeps a keypress during webview startup from being lost or replayed.
#[tauri::command]
pub fn take_global_quick_add(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, IntegrationState>,
) -> bool {
    window.label() == "main" && state.pending_quick_add.swap(false, Ordering::AcqRel)
}

pub fn is_autostart(args: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    args.into_iter().any(|arg| arg.as_ref() == AUTOSTART_ARG)
}

fn configure_startup_windows(
    windows: &mut [tauri::utils::config::WindowConfig],
    autostart: bool,
    minimize: bool,
) {
    if autostart && minimize {
        if let Some(window) = windows.iter_mut().find(|window| window.label == "main") {
            window.visible = false;
            window.focus = false;
        }
    }
}

/// Call before Builder::build so a login launch never flashes the main window.
pub fn configure_startup(
    context: &mut tauri::Context<tauri::Wry>,
    service: &TaskService,
) -> Result<(), String> {
    let preferences = Preferences::read(service)?;
    configure_startup_windows(
        &mut context.config_mut().app.windows,
        is_autostart(std::env::args()),
        preferences.launch_minimized,
    );
    Ok(())
}

pub fn start(app: tauri::AppHandle, stopping: Arc<AtomicBool>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("tasks-native-integrations".into())
        .spawn(move || {
            let service = app.state::<TaskService>().inner().clone();
            let mut last_revision = None;
            let mut last_error = None;
            while !stopping.load(Ordering::Relaxed) {
                if !crate::quit::is_pending(&app) {
                    let result = service
                        .execute("revision", json!({}))
                        .map_err(|e| e.to_string())
                        .and_then(|value| {
                            let revision = value["revision"].as_i64().unwrap_or(-1);
                            let retry = {
                                let state = app.state::<IntegrationState>();
                                let applied = state.gate.lock().map_err(|e| e.to_string())?;
                                needs_synchronization(&applied, last_revision, revision)
                            };
                            if retry {
                                let _activity = app.state::<crate::quit::Activities>().start();
                                synchronize(&app, &service)?;
                                last_revision = Some(revision);
                            }
                            Ok(())
                        });
                    match result {
                        Ok(()) => last_error = None,
                        Err(error) if last_error.as_ref() != Some(&error) => {
                            eprintln!("Could not apply desktop integration: {error}");
                            let _ = app.emit("desktop-integration-error", &error);
                            last_error = Some(error);
                        }
                        Err(_) => {}
                    }
                }
                for _ in 0..20 {
                    if stopping.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Fake {
        login: RefCell<bool>,
        registered: RefCell<Vec<Shortcut>>,
        active: RefCell<Shortcuts>,
        fail_register: RefCell<Option<Shortcut>>,
        fail_unregister: RefCell<Vec<Shortcut>>,
        fail_login: RefCell<bool>,
        calls: RefCell<Vec<String>>,
    }
    impl Platform for Fake {
        fn autostart_enabled(&self) -> Result<bool, String> {
            Ok(*self.login.borrow())
        }
        fn set_autostart(&self, enabled: bool) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("login:{enabled}"));
            if *self.fail_login.borrow() && enabled {
                return Err("login failed".into());
            }
            *self.login.borrow_mut() = enabled;
            Ok(())
        }
        fn register(&self, shortcut: Shortcut) -> Result<(), String> {
            self.calls.borrow_mut().push("register".into());
            if *self.fail_register.borrow() == Some(shortcut) {
                return Err("shortcut taken".into());
            }
            assert!(!self.registered.borrow().contains(&shortcut));
            self.registered.borrow_mut().push(shortcut);
            Ok(())
        }
        fn unregister(&self, shortcut: Shortcut) -> Result<(), String> {
            self.calls.borrow_mut().push("unregister".into());
            if self.fail_unregister.borrow().contains(&shortcut) {
                return Err("unregister failed".into());
            }
            self.registered.borrow_mut().retain(|s| *s != shortcut);
            Ok(())
        }
        fn activate(&self, shortcuts: Shortcuts) {
            *self.active.borrow_mut() = shortcuts;
        }
    }
    fn preferences(shortcut: &str) -> Preferences {
        Preferences {
            global_quick_add_shortcut: shortcut.into(),
            ..Default::default()
        }
    }

    #[test]
    fn defaults_do_not_mutate_os_registration() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        apply_and_save(
            &backend,
            &mut applied,
            &Preferences::default(),
            true,
            || Ok(()),
        )
        .unwrap();
        assert!(backend.calls.borrow().is_empty());
        assert_eq!(parse_shortcut("").unwrap(), None);
        assert!(!is_autostart(["Tasks", "--minimized"]));
        assert!(is_autostart(["Tasks", AUTOSTART_ARG]));
    }
    #[test]
    fn development_and_isolated_databases_cannot_manage_production_login_item() {
        let app = std::path::Path::new("/Applications/Tasks.app/Contents/MacOS/tasks-desktop");
        assert!(login_item_allowed(app, false, false));
        assert!(!login_item_allowed(app, true, false));
        assert!(!login_item_allowed(app, false, true));
        assert!(!login_item_allowed(app, true, true));
        assert!(!login_item_allowed(
            std::path::Path::new("/workspace/target/debug/tasks-desktop"),
            false,
            false
        ));
    }
    #[test]
    fn minimizes_only_the_main_window_on_an_opted_in_login_launch() {
        let windows = vec![
            tauri::utils::config::WindowConfig {
                label: "main".into(),
                visible: true,
                focus: true,
                ..Default::default()
            },
            tauri::utils::config::WindowConfig {
                label: "settings".into(),
                visible: true,
                focus: true,
                ..Default::default()
            },
        ];
        for (autostart, minimize) in [(false, false), (false, true), (true, false)] {
            let mut configured = windows.clone();
            configure_startup_windows(&mut configured, autostart, minimize);
            assert!(configured[0].visible && configured[0].focus);
        }
        let mut configured = windows;
        configure_startup_windows(&mut configured, true, true);
        assert!(!configured[0].visible && !configured[0].focus);
        assert!(configured[1].visible && configured[1].focus);
    }
    #[test]
    fn supported_shared_chords_parse_to_native_keys() {
        for chord in [
            "ctrl+shift+a",
            "mod+alt+space",
            "mod+ctrl+alt+shift+f24",
            "alt+arrowleft",
            "ctrl+9",
            "ctrl+enter",
            "alt+pageup",
        ] {
            assert!(parse_shortcut(chord).unwrap().is_some(), "{chord}");
        }
        assert!(parse_shortcut("g t").is_err());
        assert!(parse_shortcut("mod+q").is_err());
    }
    #[test]
    fn conflict_preserves_previous_shortcut_and_skips_persistence() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = preferences("ctrl+shift+a");
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        let desired = preferences("ctrl+shift+b");
        *backend.fail_register.borrow_mut() =
            parse_shortcut(&desired.global_quick_add_shortcut).unwrap();
        assert!(
            apply_and_save(&backend, &mut applied, &desired, false, || panic!(
                "must not persist a failed registration"
            ))
            .is_err()
        );
        assert_eq!(
            backend.registered.borrow().as_slice(),
            &[parse_shortcut(&old.global_quick_add_shortcut)
                .unwrap()
                .unwrap()]
        );
        assert_eq!(applied.preferences, Some(old));
    }
    #[test]
    fn sqlite_failure_restores_both_os_settings() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = preferences("ctrl+shift+a");
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        let desired = Preferences {
            launch_at_login: true,
            ..preferences("ctrl+shift+b")
        };
        assert!(
            apply_and_save(&backend, &mut applied, &desired, false, || Err(
                "database read only".into()
            ))
            .is_err()
        );
        assert!(!*backend.login.borrow());
        assert_eq!(*backend.active.borrow(), Shortcuts::parse(&old).unwrap());
        assert_eq!(backend.registered.borrow().len(), 1);
    }
    #[test]
    fn platform_login_failure_is_not_saved_and_clear_unregisters() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        *backend.fail_login.borrow_mut() = true;
        let desired = Preferences {
            launch_at_login: true,
            ..preferences("ctrl+shift+a")
        };
        assert!(
            apply_and_save(&backend, &mut applied, &desired, false, || panic!(
                "must not save"
            ))
            .is_err()
        );
        assert!(backend.registered.borrow().is_empty());
        *backend.fail_login.borrow_mut() = false;
        apply_and_save(
            &backend,
            &mut applied,
            &preferences("ctrl+shift+a"),
            false,
            || Ok(()),
        )
        .unwrap();
        apply_and_save(
            &backend,
            &mut applied,
            &Preferences::default(),
            false,
            || Ok(()),
        )
        .unwrap();
        assert!(backend.registered.borrow().is_empty());
        assert_eq!(*backend.active.borrow(), Shortcuts::default());
    }

    fn two_shortcuts(quick_add: &str, show_hide: &str) -> Preferences {
        Preferences {
            global_quick_add_shortcut: quick_add.into(),
            global_show_hide_shortcut: show_hide.into(),
            ..Default::default()
        }
    }
    #[test]
    fn both_shortcuts_register_and_dispatch_independently_and_disable_separately() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = two_shortcuts("ctrl+shift+a", "ctrl+shift+m");
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        let bindings = Shortcuts::parse(&old).unwrap();
        assert_eq!(backend.registered.borrow().len(), 2);
        assert_eq!(
            active_action(bindings.packed(), bindings.quick_add.unwrap().id()),
            Some(GlobalAction::QuickAdd)
        );
        assert_eq!(
            active_action(bindings.packed(), bindings.show_hide.unwrap().id()),
            Some(GlobalAction::ShowHide)
        );
        assert_eq!(active_action(0, bindings.show_hide.unwrap().id()), None);
        let disabled = two_shortcuts("ctrl+shift+a", "");
        apply_and_save(&backend, &mut applied, &disabled, false, || Ok(())).unwrap();
        assert_eq!(
            *backend.active.borrow(),
            Shortcuts::parse(&disabled).unwrap()
        );
        assert_eq!(
            backend.registered.borrow().as_slice(),
            &[bindings.quick_add.unwrap()]
        );
        assert_eq!(
            active_action(
                backend.active.borrow().packed(),
                bindings.show_hide.unwrap().id()
            ),
            None
        );
    }
    #[test]
    fn duplicate_actions_are_rejected_before_os_calls_and_persistence() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = two_shortcuts("ctrl+shift+a", "ctrl+shift+m");
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        backend.calls.borrow_mut().clear();
        for desired in [
            two_shortcuts("ctrl+shift+a", "ctrl+shift+a"),
            two_shortcuts("ctrl+shift+m", "ctrl+shift+m"),
        ] {
            assert!(
                apply_and_save(&backend, &mut applied, &desired, false, || panic!(
                    "must not save collision"
                ))
                .is_err()
            );
            assert_eq!(applied.preferences.as_ref(), Some(&old));
            assert_eq!(*backend.active.borrow(), Shortcuts::parse(&old).unwrap());
            assert!(backend.calls.borrow().is_empty());
        }
    }
    #[test]
    fn second_registration_failure_rolls_back_first_replacement_and_preserves_old_pair() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = two_shortcuts("ctrl+shift+a", "ctrl+shift+m");
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        let desired = two_shortcuts("ctrl+shift+b", "ctrl+shift+n");
        *backend.fail_register.borrow_mut() = Shortcuts::parse(&desired).unwrap().show_hide;
        assert!(
            apply_and_save(&backend, &mut applied, &desired, false, || panic!(
                "must not save failed registration"
            ))
            .is_err()
        );
        assert_eq!(
            backend.registered.borrow().as_slice(),
            Shortcuts::parse(&old).unwrap().registered()
        );
        assert_eq!(*backend.active.borrow(), Shortcuts::parse(&old).unwrap());
        assert_eq!(applied.preferences, Some(old));
    }
    #[test]
    fn database_failure_restores_both_shortcuts_and_login_preference() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = two_shortcuts("ctrl+shift+a", "ctrl+shift+m");
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        let desired = Preferences {
            launch_at_login: true,
            ..two_shortcuts("ctrl+shift+b", "ctrl+shift+n")
        };
        assert!(
            apply_and_save(&backend, &mut applied, &desired, false, || Err(
                "database busy".into()
            ))
            .is_err()
        );
        assert!(!*backend.login.borrow());
        assert_eq!(*backend.active.borrow(), Shortcuts::parse(&old).unwrap());
        assert_eq!(
            backend.registered.borrow().as_slice(),
            Shortcuts::parse(&old).unwrap().registered()
        );
        assert!(applied.preferences.is_none());
    }
    #[test]
    fn failed_sqlite_rollback_never_activates_unsaved_bindings_and_retries_without_revision_change()
    {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = two_shortcuts("ctrl+shift+a", "ctrl+shift+m");
        let desired = two_shortcuts("ctrl+shift+b", "ctrl+shift+n");
        let old_bindings = Shortcuts::parse(&old).unwrap();
        let new_bindings = Shortcuts::parse(&desired).unwrap();
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        assert!(!needs_synchronization(&applied, Some(7), 7));
        *backend.fail_register.borrow_mut() = old_bindings.quick_add;
        let error = apply_and_save(&backend, &mut applied, &desired, false, || {
            Err("database busy".into())
        })
        .unwrap_err();
        assert!(error.contains("Could not restore"));
        for replacement in new_bindings.registered() {
            assert_eq!(
                active_action(backend.active.borrow().packed(), replacement.id()),
                None
            );
        }
        assert!(needs_synchronization(&applied, Some(7), 7));
        *backend.fail_register.borrow_mut() = None;
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        assert_eq!(*backend.active.borrow(), old_bindings);
        assert_eq!(backend.registered.borrow().len(), 2);
        assert!(!needs_synchronization(&applied, Some(7), 7));
        assert!(needs_synchronization(&applied, Some(7), 8));
    }

    #[test]
    fn unregister_failure_restores_removed_old_key_and_deactivates_failed_cleanup() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = two_shortcuts("ctrl+shift+a", "ctrl+shift+m");
        let desired = two_shortcuts("ctrl+shift+b", "ctrl+shift+n");
        let old_bindings = Shortcuts::parse(&old).unwrap();
        let new_bindings = Shortcuts::parse(&desired).unwrap();
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        *backend.fail_unregister.borrow_mut() = vec![
            old_bindings.show_hide.unwrap(),
            new_bindings.quick_add.unwrap(),
        ];
        let error = apply_and_save(&backend, &mut applied, &desired, false, || {
            panic!("must not save")
        })
        .unwrap_err();
        assert!(error.contains("Could not restore"));
        assert_eq!(*backend.active.borrow(), old_bindings);
        assert!(applied.preferences.is_none());
        for shortcut in old_bindings.registered() {
            assert!(backend.registered.borrow().contains(&shortcut));
        }
        assert_eq!(
            active_action(
                backend.active.borrow().packed(),
                new_bindings.quick_add.unwrap().id()
            ),
            None
        );
        backend.fail_unregister.borrow_mut().clear();
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        assert_eq!(backend.registered.borrow().len(), 2);
        assert_eq!(applied.preferences, Some(old));
    }
    #[test]
    fn swapping_actions_keeps_os_registrations_and_switches_dispatch_atomically() {
        let backend = Fake::default();
        let mut applied = Applied::default();
        let old = two_shortcuts("ctrl+shift+a", "ctrl+shift+m");
        apply_and_save(&backend, &mut applied, &old, false, || Ok(())).unwrap();
        backend.calls.borrow_mut().clear();
        let swapped = two_shortcuts("ctrl+shift+m", "ctrl+shift+a");
        apply_and_save(&backend, &mut applied, &swapped, false, || Ok(())).unwrap();
        assert!(backend.calls.borrow().is_empty());
        let old_bindings = Shortcuts::parse(&old).unwrap();
        assert_eq!(
            active_action(
                backend.active.borrow().packed(),
                old_bindings.quick_add.unwrap().id()
            ),
            Some(GlobalAction::ShowHide)
        );
        assert_eq!(
            active_action(
                backend.active.borrow().packed(),
                old_bindings.show_hide.unwrap().id()
            ),
            Some(GlobalAction::QuickAdd)
        );
    }

    struct FakeWindow {
        visible: bool,
        focused: bool,
        minimized: bool,
        calls: RefCell<Vec<&'static str>>,
    }
    impl MainWindow for FakeWindow {
        fn visible(&self) -> Result<bool, String> {
            Ok(self.visible)
        }
        fn focused(&self) -> Result<bool, String> {
            Ok(self.focused)
        }
        fn minimized(&self) -> Result<bool, String> {
            Ok(self.minimized)
        }
        fn hide(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("hide");
            Ok(())
        }
        fn unminimize(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("unminimize");
            Ok(())
        }
        fn show(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("show");
            Ok(())
        }
        fn focus(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("focus");
            Ok(())
        }
    }
    #[test]
    fn show_hide_hides_only_visible_focused_unminimized_window_otherwise_restores_it() {
        for visible in [false, true] {
            for focused in [false, true] {
                for minimized in [false, true] {
                    let window = FakeWindow {
                        visible,
                        focused,
                        minimized,
                        calls: RefCell::new(vec![]),
                    };
                    toggle_main_window(&window).unwrap();
                    let expected = if visible && focused && !minimized {
                        vec!["hide"]
                    } else {
                        vec!["unminimize", "show", "focus"]
                    };
                    assert_eq!(
                        *window.calls.borrow(),
                        expected,
                        "visible={visible}, focused={focused}, minimized={minimized}"
                    );
                }
            }
        }
    }
}
