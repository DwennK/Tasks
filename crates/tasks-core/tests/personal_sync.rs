use serde_json::{json, Value};
use tasks_core::TaskService;

struct Peer {
    directory: tempfile::TempDir,
    service: TaskService,
}
impl Peer {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let service = TaskService::open(directory.path().join("tasks.sqlite3")).unwrap();
        Self { directory, service }
    }
    fn call(&self, action: &str, input: Value) -> Value {
        self.service
            .execute(action, input.clone())
            .unwrap_or_else(|error| panic!("{action} {input}: {error}"))
    }
    fn export(&self) -> Value {
        self.call("sync.export", json!({}))
    }
    fn merge(&self, packet: Value) -> Value {
        self.call("sync.merge", json!({"packets":[packet]}))
    }
}
fn exchange(a: &Peer, b: &Peer) {
    let first = a.export();
    let second = b.export();
    a.merge(second);
    b.merge(first);
}

#[test]
fn sync_is_opt_in_and_checkpoint_is_local_provider_scoped_and_observational() {
    let a = Peer::new();
    let task = a.call("tasks.create", json!({"title":"Before enabling cloud"}));
    a.call("attachments.add",json!({"id":task["id"],"name":"file.txt","mime_type":"text/plain","content_base64":"SGVsbG8="}));
    let status = a.call("sync.status", json!({}));
    assert_eq!(status["enabled"], false);
    assert_eq!(status["entities"], 0);
    assert_eq!(status["revision"], 0);
    let connection = rusqlite::Connection::open(a.service.path()).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM sync_entities", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let packet = a.export();
    assert_eq!(packet, a.export(), "Export changed without a local edit");
    assert!(packet["entities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entity| entity["kind"] == "attachments"));
    let before = a.call("sync.status", json!({}));
    let revision = a.call("revision", json!({}));
    let token = "a".repeat(43);
    a.call(
        "sync.checkpoint.set",
        json!({"provider":"google_drive","token":token}),
    );
    assert_eq!(
        a.call("sync.checkpoint", json!({"provider":"google_drive"}))["token"],
        token
    );
    assert!(a.call("sync.checkpoint", json!({"provider":"dropbox"}))["token"].is_null());
    assert_eq!(a.call("sync.status", json!({})), before);
    assert_eq!(a.call("revision", json!({})), revision);
    assert_eq!(
        a.export(),
        packet,
        "A checkpoint leaked into the cloud packet"
    );
    let filter = json!({"id":"inbox","name":"Inbox filter","list_ids":["inbox"],"tag_ids":[],"priorities":[],"date":"all","match":"all"});
    a.call(
        "settings.set",
        json!({"key":"saved_filters","value":[filter]}),
    );
    let authored = a.export();
    assert!(authored["entities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entity| entity["kind"] == "settings" && entity["id"] == "saved_filters"));
    a.call("settings.set", json!({"key":"saved_filters","value":[]}));
    let cleared = a.export();
    let preset = cleared["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entity| entity["kind"] == "settings" && entity["id"] == "saved_filters")
        .unwrap();
    assert_eq!(
        preset["fields"]["value"][0]["value"], "[]",
        "An explicit clear must still sync after initial defaults were omitted"
    );
}

#[test]
fn conflicting_content_is_preserved_resolved_and_survives_stale_packets() {
    let a = Peer::new();
    let b = Peer::new();
    let task = a.call("tasks.create", json!({"title":"Original"}));
    exchange(&a, &b);
    a.call("tasks.update", json!({"id":task["id"],"title":"On laptop"}));
    b.call(
        "tasks.update",
        json!({"id":task["id"],"title":"On desktop"}),
    );
    let stale = b.export();
    exchange(&a, &b);
    assert_eq!(
        a.call("tasks.get", json!({"id":task["id"]}))["title"],
        b.call("tasks.get", json!({"id":task["id"]}))["title"]
    );
    let conflicts = a.call("sync.conflicts", json!({}));
    let conflict = conflicts["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == task["id"] && item["field"] == "title")
        .unwrap();
    assert_eq!(conflict["versions"].as_array().unwrap().len(), 2);
    let chosen = conflict["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|version| version["value"] == "On laptop")
        .unwrap();
    a.call(
        "sync.resolve",
        json!({"kind":"tasks","id":task["id"],"field":"title","version_id":chosen["id"]}),
    );
    exchange(&a, &b);
    a.merge(stale.clone());
    b.merge(stale);
    for peer in [&a, &b] {
        assert_eq!(
            peer.call("tasks.get", json!({"id":task["id"]}))["title"],
            "On laptop"
        );
        assert!(peer.call("sync.conflicts", json!({}))["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["field"] != "title"));
    }
}

#[test]
fn graph_attachments_templates_portable_preferences_sync_without_machine_settings_or_delivery_state(
) {
    let a = Peer::new();
    let b = Peer::new();
    let folder = a.call("folders.create", json!({"name":"Projects","icon":"🌍"}));
    let list = a.call(
        "lists.create",
        json!({"name":"Work","folder_id":folder["id"],"view_mode":"timeline"}),
    );
    let section = a.call(
        "sections.create",
        json!({"list_id":list["id"],"name":"Planning"}),
    );
    let tag = a.call("tags.create", json!({"name":"Important"}));
    let task = a.call("tasks.create",json!({"title":"Synced project","list_id":list["id"],"section_id":section["id"],"progress":40,"tag_ids":[tag["id"]],"due_date":"2026-11-01","reminders":["2026-11-01T09:00:00Z"]}));
    let child = a.call(
        "tasks.create",
        json!({"title":"Child","parent_id":task["id"]}),
    );
    a.call(
        "links.add",
        json!({"id":task["id"],"linked_task_id":child["id"]}),
    );
    let file = a.call("attachments.add",json!({"id":task["id"],"name":"notes.txt","mime_type":"text/plain","content_base64":"SGVsbG8="}));
    a.call(
        "comments.add",
        json!({"id":task["id"],"body":"Review this","attachment_ids":[file["id"]]}),
    );
    let template = a.call(
        "templates.create",
        json!({"name":"Project template","task_id":task["id"]}),
    );
    let filter = json!({"id":"work","name":"Work filter","list_ids":[list["id"]],"tag_ids":[tag["id"]],"priorities":[],"date":"all","match":"all"});
    a.call(
        "settings.set",
        json!({"key":"saved_filters","value":[filter.clone()]}),
    );
    let summary = json!({"id":"weekly","name":"Weekly","options":{"range":"this_week","from":"2026-10-26","to":"2026-11-01","basis":"completed","list_ids":[list["id"]],"tag_ids":[],"statuses":["completed"],"priorities":[],"group_by":"list","sort_by":"date","include_subtasks":true,"fields":["progress","notes"]}});
    a.call(
        "settings.set",
        json!({"key":"summary_templates","value":[summary.clone()]}),
    );
    a.call("settings.set", json!({"key":"theme","value":"dark"}));
    a.call(
        "settings.set",
        json!({"key":"global_show_hide_shortcut","value":"ctrl+shift+y"}),
    );
    let due = a.call("reminders.due", json!({"now":"2026-11-02T00:00:00Z"}));
    a.call("reminders.mark",json!({"id":task["id"],"reminder_at":due[0]["reminder_at"],"generation":due[0]["reminder_generation"]}));
    let packet = a.export();
    assert!(packet["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entity| entity["kind"] == "settings")
        .all(
            |entity| [json!("saved_filters"), json!("summary_templates")].contains(&entity["id"])
        ));
    b.merge(packet);
    let snapshot = b.call("snapshot", json!({}));
    assert_eq!(snapshot["settings"]["saved_filters"], json!([filter]));
    assert_eq!(snapshot["settings"]["summary_templates"], json!([summary]));
    assert_ne!(snapshot["settings"]["theme"], "dark");
    let copied = b.call("tasks.get", json!({"id":task["id"]}));
    assert_eq!(copied["progress"], 40);
    assert_eq!(copied["tag_ids"], json!([tag["id"]]));
    assert_eq!(copied["comments"][0]["attachment_ids"], json!([file["id"]]));
    assert_eq!(
        b.call("attachments.get", json!({"id":file["id"]}))["content_base64"],
        "SGVsbG8="
    );
    let applied = b.call("templates.apply", json!({"id":template["id"]}));
    assert_eq!(applied["attachments"].as_array().unwrap().len(), 1);
    assert_eq!(applied["progress"], 0);
    let remote_due = b.call("reminders.due", json!({"now":"2026-11-02T00:00:00Z"}));
    let remote = remote_due
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == task["id"])
        .unwrap();
    assert_ne!(remote["reminder_generation"], due[0]["reminder_generation"]);
    b.call("reminders.mark",json!({"id":task["id"],"reminder_at":remote["reminder_at"],"generation":remote["reminder_generation"]}));
    a.call("tasks.update", json!({"id":task["id"],"notes":"New note"}));
    b.merge(a.export());
    assert!(b
        .call("reminders.due", json!({"now":"2026-11-02T00:00:00Z"}))
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["id"] != task["id"]));
}

#[test]
fn database_restore_rebases_pending_edits_even_when_local_counter_is_ahead_of_cloud() {
    let a = Peer::new();
    let task = a.call("tasks.create", json!({"title":"Original"}));
    a.export();
    let old_token = "a".repeat(43);
    let new_token = "b".repeat(43);
    a.call(
        "sync.checkpoint.set",
        json!({"provider":"google_drive","token":old_token}),
    );
    let restored_path = a.directory.path().join("restored.sqlite3");
    rusqlite::Connection::open(a.service.path())
        .unwrap()
        .execute("VACUUM INTO ?1", [restored_path.to_str().unwrap()])
        .unwrap();
    a.call(
        "tasks.update",
        json!({"id":task["id"],"title":"Cloud edit"}),
    );
    let cloud = a.export();
    let restored = TaskService::open(restored_path).unwrap();
    for index in 0..5 {
        restored
            .execute(
                "tasks.update",
                json!({"id":task["id"],"title":format!("Restored edit {index}")}),
            )
            .unwrap();
    }
    let before = restored.execute("sync.status", json!({})).unwrap();
    assert!(before["clock"].as_i64().unwrap() > cloud["clock"].as_i64().unwrap());
    restored.execute("sync.merge",json!({"packets":[cloud],"checkpoint_provider":"google_drive","expected_checkpoint":new_token})).unwrap();
    let after = restored.execute("sync.status", json!({})).unwrap();
    assert_ne!(before["device_id"], after["device_id"]);
    let conflicts = restored.execute("sync.conflicts", json!({})).unwrap();
    let title = conflicts["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == task["id"] && item["field"] == "title")
        .unwrap();
    assert!(title["versions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|version| version["value"] == "Restored edit 4"));
    assert!(title["versions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|version| version["value"] == "Cloud edit"));
    let packet = restored.execute("sync.export", json!({})).unwrap();
    a.merge(packet);
}
