use serde_json::json;
use tasks_core::TaskService;

#[test]
fn desktop_settings_default_to_no_registration_and_persist_across_clients() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.sqlite3");
    let service = TaskService::open(&path).unwrap();
    let initial = service.execute("snapshot", json!({})).unwrap();
    assert_eq!(initial["settings"]["launch_at_login"], false);
    assert_eq!(initial["settings"]["launch_minimized"], false);
    assert_eq!(initial["settings"]["global_quick_add_shortcut"], "");
    assert_eq!(initial["settings"]["global_show_hide_shortcut"], "");
    // The shared service stores preferences only; there is no OS registration
    // dependency in this crate, including when invoked by CLI or MCP.
    for (key, value) in [
        ("launch_at_login", json!(true)),
        ("launch_minimized", json!(true)),
        ("global_quick_add_shortcut", json!("ctrl+shift+a")),
        ("global_show_hide_shortcut", json!("ctrl+shift+m")),
    ] {
        service
            .execute("settings.set", json!({"key":key,"value":value}))
            .unwrap();
    }
    let other = TaskService::open(&path).unwrap();
    let saved = other.execute("snapshot", json!({})).unwrap();
    assert_eq!(
        other.execute("settings.get", json!({})).unwrap(),
        saved["settings"]
    );
    assert_eq!(saved["settings"]["launch_at_login"], true);
    assert_eq!(saved["settings"]["launch_minimized"], true);
    assert_eq!(
        saved["settings"]["global_quick_add_shortcut"],
        "ctrl+shift+a"
    );
    assert_eq!(
        saved["settings"]["global_show_hide_shortcut"],
        "ctrl+shift+m"
    );
    let revision = other.execute("revision", json!({})).unwrap();
    assert!(other
        .execute(
            "settings.set",
            json!({"key":"global_quick_add_shortcut","value":"g t"})
        )
        .is_err());
    assert_eq!(other.execute("revision", json!({})).unwrap(), revision);
    assert_eq!(
        service.execute("snapshot", json!({})).unwrap()["settings"]["global_quick_add_shortcut"],
        "ctrl+shift+a"
    );
}

#[test]
fn local_hotkeys_accept_reference_task_actions_and_punctuation() {
    let directory = tempfile::tempdir().unwrap();
    let service = TaskService::open(directory.path().join("settings.sqlite3")).unwrap();
    let mappings = json!({"print_task":"mod+p","set_date":"mod+d","settings":"mod+,","shortcuts":"?","list_view":"v l","kanban_view":"v k","pin_task":"","priority_high":"ctrl+3","toggle_all_groups":"","toggle_all_subtasks":"mod+alt+e"});
    service
        .execute("settings.set", json!({"key":"hotkeys","value":mappings}))
        .unwrap();
    assert_eq!(
        service.execute("snapshot", json!({})).unwrap()["settings"]["hotkeys"],
        mappings
    );
    assert!(service
        .execute(
            "settings.set",
            json!({"key":"hotkeys","value":{"unknown_action":"mod+a"}})
        )
        .is_err());
}

#[test]
fn global_shortcut_collisions_are_rejected_atomically_across_clients() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.sqlite3");
    let service = TaskService::open(&path).unwrap();
    let other = TaskService::open(&path).unwrap();
    service
        .execute(
            "settings.set",
            json!({"key":"global_quick_add_shortcut","value":"ctrl+shift+a"}),
        )
        .unwrap();
    other
        .execute(
            "settings.set",
            json!({"key":"global_show_hide_shortcut","value":"ctrl+shift+m"}),
        )
        .unwrap();
    let saved = service.execute("snapshot", json!({})).unwrap();
    for (key, chord) in [
        ("global_show_hide_shortcut", "ctrl+shift+a"),
        ("global_quick_add_shortcut", "ctrl+shift+m"),
    ] {
        assert!(other
            .execute("settings.set", json!({"key":key,"value":chord}))
            .is_err());
        assert_eq!(service.execute("snapshot", json!({})).unwrap(), saved);
    }
    other
        .execute(
            "settings.set",
            json!({"key":"global_quick_add_shortcut","value":""}),
        )
        .unwrap();
    service
        .execute(
            "settings.set",
            json!({"key":"global_show_hide_shortcut","value":"ctrl+shift+a"}),
        )
        .unwrap();
    let reopened = TaskService::open(path).unwrap();
    let settings = reopened.execute("settings.get", json!({})).unwrap();
    assert_eq!(settings["global_quick_add_shortcut"], "");
    assert_eq!(settings["global_show_hide_shortcut"], "ctrl+shift+a");
}

#[test]
fn simultaneous_global_shortcut_claims_have_one_winner() {
    use std::sync::{Arc, Barrier};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.sqlite3");
    let service = TaskService::open(&path).unwrap();
    let before = service.execute("revision", json!({})).unwrap()["revision"]
        .as_i64()
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let writers = ["global_quick_add_shortcut", "global_show_hide_shortcut"].map(|key| {
        let writer = TaskService::open(&path).unwrap();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            writer.execute("settings.set", json!({"key":key,"value":"ctrl+shift+a"}))
        })
    });
    let outcomes = writers.map(|writer| writer.join().unwrap());
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    let settings = service.execute("settings.get", json!({})).unwrap();
    assert_eq!(
        ["global_quick_add_shortcut", "global_show_hide_shortcut"]
            .iter()
            .filter(|key| settings[**key] == "ctrl+shift+a")
            .count(),
        1
    );
    assert_eq!(
        service.execute("revision", json!({})).unwrap()["revision"],
        before + 1
    );
}

#[test]
fn automatic_update_checks_persist_and_reject_non_boolean_values() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.sqlite3");
    let service = TaskService::open(&path).unwrap();
    service
        .execute(
            "settings.set",
            json!({"key":"automatic_update_checks","value":false}),
        )
        .unwrap();
    let revision = service.execute("revision", json!({})).unwrap();
    assert!(service
        .execute(
            "settings.set",
            json!({"key":"automatic_update_checks","value":"false"})
        )
        .is_err());
    assert_eq!(service.execute("revision", json!({})).unwrap(), revision);
    let reopened = TaskService::open(&path).unwrap();
    assert_eq!(
        reopened.execute("settings.get", json!({})).unwrap()["automatic_update_checks"],
        false
    );
}
