//! Adversarial sync coverage using independent local databases only.
use serde_json::{json, Value};
use tasks_core::{ServiceError, TaskService};

struct Peer {
    _directory: tempfile::TempDir,
    service: TaskService,
}
impl Peer {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let service = TaskService::open(directory.path().join("sync.sqlite3")).unwrap();
        Self {
            _directory: directory,
            service,
        }
    }
    fn call(&self, action: &str, input: Value) -> Value {
        self.service
            .execute(action, input.clone())
            .unwrap_or_else(|error| panic!("{action} {input}: {error}"))
    }
    fn export(&self) -> Value {
        self.call("sync.export", json!({}))
    }
    fn merge(&self, packets: Vec<Value>) -> Value {
        self.call("sync.merge", json!({"packets":packets}))
    }
    fn task(&self, title: &str) -> Value {
        self.call("tasks.create", json!({"title":title}))
    }
    fn get(&self, id: &Value) -> Value {
        self.call("tasks.get", json!({"id":id}))
    }
}

fn exchange(first: &Peer, second: &Peer) {
    let a = first.export();
    let b = second.export();
    first.merge(vec![b]);
    second.merge(vec![a]);
}
fn field(packet: &mut Value, kind: &str, id: &str, field: &str, value: Value) {
    let entity = packet["entities"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entity| entity["kind"] == kind && entity["id"] == id)
        .unwrap();
    for version in entity["fields"][field].as_array_mut().unwrap() {
        version["value"] = value.clone();
    }
}

#[test]
fn disjoint_offline_edits_converge_and_replaying_packets_does_not_bump_task_version() {
    let a = Peer::new();
    let b = Peer::new();
    let task = a.task("Original");
    exchange(&a, &b);
    a.call(
        "tasks.update",
        json!({"id":task["id"],"title":"Edited on A"}),
    );
    b.call("tasks.update", json!({"id":task["id"],"priority":3}));
    exchange(&a, &b);
    for peer in [&a, &b] {
        let merged = peer.get(&task["id"]);
        assert_eq!(merged["title"], "Edited on A");
        assert_eq!(merged["priority"], 3);
    }
    let packet = b.export();
    let once = a.get(&task["id"]);
    a.merge(vec![packet.clone(), packet.clone()]);
    a.merge(vec![packet]);
    assert_eq!(
        a.get(&task["id"]),
        once,
        "Retrying an identical cloud packet changed a task"
    );
}

#[test]
fn concurrent_parent_deletion_keeps_an_offline_new_child_reachable() {
    let a = Peer::new();
    let b = Peer::new();
    let parent = a.task("Parent deleted on A");
    exchange(&a, &b);
    a.call("tasks.delete", json!({"id":parent["id"]}));
    let child = b.call(
        "tasks.create",
        json!({"title":"Offline child on B","parent_id":parent["id"]}),
    );
    exchange(&a, &b);
    for peer in [&a, &b] {
        let parent = peer.get(&parent["id"]);
        let child = peer.get(&child["id"]);
        assert!(parent["deleted_at"].is_string());
        assert!(
            child["deleted_at"].is_null(),
            "A newly created offline child was discarded"
        );
        assert!(
            child["parent_id"].is_null(),
            "Active child remains hidden under a parent in Trash"
        );
    }
}

#[test]
fn an_offline_active_child_reopens_a_parent_marked_wont_do_elsewhere() {
    let a = Peer::new();
    let b = Peer::new();
    let parent = a.task("Parent marked Won’t Do on A");
    exchange(&a, &b);
    a.call("tasks.wont_do", json!({"id":parent["id"],"wont_do":true}));
    let child = b.call(
        "tasks.create",
        json!({"title":"Offline active child","parent_id":parent["id"]}),
    );
    exchange(&a, &b);
    for peer in [&a, &b] {
        let parent = peer.get(&parent["id"]);
        let child = peer.get(&child["id"]);
        assert!(child["wont_do_at"].is_null());
        assert!(
            parent["wont_do_at"].is_null(),
            "Active child remains under a hidden Won’t Do parent"
        );
        assert!(parent["completed_at"].is_null());
    }
}

#[test]
fn concurrent_parent_moves_cannot_create_a_cycle_and_stabilize_after_exchange() {
    let a = Peer::new();
    let b = Peer::new();
    let first = a.task("First");
    let second = a.task("Second");
    exchange(&a, &b);
    a.call(
        "tasks.update",
        json!({"id":first["id"],"parent_id":second["id"]}),
    );
    b.call(
        "tasks.update",
        json!({"id":second["id"],"parent_id":first["id"]}),
    );
    exchange(&a, &b);
    let a_first = a.get(&first["id"]);
    let a_second = a.get(&second["id"]);
    assert!(a_first["parent_id"].is_null() || a_second["parent_id"].is_null());
    assert_eq!(a_first["parent_id"], b.get(&first["id"])["parent_id"]);
    assert_eq!(a_second["parent_id"], b.get(&second["id"])["parent_id"]);
    exchange(&a, &b);
    assert_eq!(a.get(&first["id"]), a_first);
    assert_eq!(a.get(&second["id"]), a_second);
}

#[test]
fn deleting_a_list_while_another_device_adds_a_task_preserves_the_new_task_in_inbox() {
    let a = Peer::new();
    let b = Peer::new();
    let list = a.call("lists.create", json!({"name":"Deleted offline"}));
    exchange(&a, &b);
    a.call("lists.delete", json!({"id":list["id"]}));
    let task = b.call(
        "tasks.create",
        json!({"title":"Created offline","list_id":list["id"]}),
    );
    exchange(&a, &b);
    for peer in [&a, &b] {
        let task = peer.get(&task["id"]);
        assert_eq!(task["list_id"], "inbox");
        assert!(task["deleted_at"].is_null());
    }
}

#[test]
fn stale_packets_cannot_resurrect_a_deleted_comment_even_after_a_concurrent_edit() {
    let a = Peer::new();
    let b = Peer::new();
    let task = a.task("Discussion");
    let comment = a.call(
        "comments.add",
        json!({"id":task["id"],"body":"Original comment"}),
    );
    exchange(&a, &b);
    let stale = b.export();
    a.call("comments.delete", json!({"id":comment["id"]}));
    b.call(
        "comments.update",
        json!({"id":comment["id"],"body":"Concurrent edit"}),
    );
    exchange(&a, &b);
    for peer in [&a, &b] {
        assert_eq!(peer.get(&task["id"])["comments"], json!([]));
        peer.merge(vec![stale.clone()]);
        assert_eq!(peer.get(&task["id"])["comments"], json!([]));
    }
}

#[test]
fn merged_edits_invalidate_task_and_comment_versions_without_erasing_local_history() {
    let a = Peer::new();
    let b = Peer::new();
    let task = a.task("Before sync");
    let comment = a.call(
        "comments.add",
        json!({"id":task["id"],"body":"Comment before sync"}),
    );
    exchange(&a, &b);
    let stale_task = a.get(&task["id"]);
    let stale_comment = stale_task["comments"][0].clone();
    let old_activities = a.call("tasks.activities", json!({"id":task["id"]}))["items"].clone();
    b.call(
        "tasks.update",
        json!({"id":task["id"],"notes":"A remote description"}),
    );
    b.call(
        "comments.update",
        json!({"id":comment["id"],"body":"A remote comment"}),
    );
    a.merge(vec![b.export()]);
    assert!(matches!(a.service.execute("tasks.update", json!({"id":task["id"],"title":"Stale overwrite","expected_version":stale_task["version"]})), Err(ServiceError::Conflict)));
    assert!(matches!(a.service.execute("comments.update", json!({"id":comment["id"],"body":"Stale overwrite","expected_version":stale_comment["version"]})), Err(ServiceError::Conflict)));
    let activities = a.call("tasks.activities", json!({"id":task["id"]}));
    for old in old_activities.as_array().unwrap() {
        assert!(
            activities["items"].as_array().unwrap().contains(old),
            "Sync erased a local task activity"
        );
    }
}

#[test]
fn relation_only_comment_changes_invalidate_stale_tokens_even_when_timestamps_match() {
    let a = Peer::new();
    let b = Peer::new();
    let task = a.task("Attachment discussion");
    let attachment = a.call("attachments.add", json!({"id":task["id"],"name":"note.txt","mime_type":"text/plain","content_base64":"SGVsbG8="}));
    let comment = a.call(
        "comments.add",
        json!({"id":task["id"],"body":"Unchanged body"}),
    );
    exchange(&a, &b);
    let old_task = a.get(&task["id"]);
    let old_comment = old_task["comments"][0].clone();
    b.call(
        "comments.update",
        json!({"id":comment["id"],"attachment_ids":[attachment["id"]]}),
    );
    // Simulate two writes within one millisecond. The version token, rather
    // than wall-clock precision, must protect a later editor's attachment list.
    let conn = rusqlite::Connection::open(b.service.path()).unwrap();
    conn.execute(
        "UPDATE comments SET updated_at=?2 WHERE id=?1",
        rusqlite::params![
            comment["id"].as_str().unwrap(),
            old_comment["updated_at"].as_str().unwrap()
        ],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET updated_at=?2 WHERE id=?1",
        rusqlite::params![
            task["id"].as_str().unwrap(),
            old_task["updated_at"].as_str().unwrap()
        ],
    )
    .unwrap();
    drop(conn);
    a.merge(vec![b.export()]);
    assert!(matches!(a.service.execute("comments.update", json!({"id":comment["id"],"attachment_ids":[],"expected_version":old_comment["version"]})), Err(ServiceError::Conflict)));
    assert!(matches!(a.service.execute("tasks.update", json!({"id":task["id"],"title":"Stale task editor","expected_version":old_task["version"]})), Err(ServiceError::Conflict)));
    assert_eq!(
        a.get(&task["id"])["comments"][0]["attachment_ids"],
        json!([attachment["id"]])
    );
}

#[test]
fn unrelated_title_edits_preserve_details_conflicts_until_explicit_resolution() {
    let a = Peer::new();
    let b = Peer::new();
    let task = a.task("Progress conflict");
    exchange(&a, &b);
    a.call("tasks.update", json!({"id":task["id"],"progress":20}));
    b.call("tasks.update", json!({"id":task["id"],"progress":40}));
    exchange(&a, &b);
    let conflicts = a.call("sync.conflicts", json!({}));
    let conflict = conflicts["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["kind"] == "tasks" && item["id"] == task["id"] && item["field"] == "details_json"
        })
        .expect("Concurrent progress values should be retained")
        .clone();
    a.call(
        "tasks.update",
        json!({"id":task["id"],"title":"Only the title changed"}),
    );
    let remaining = a.call("sync.conflicts", json!({}));
    let remaining = remaining["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["kind"] == "tasks" && item["id"] == task["id"] && item["field"] == "details_json"
        })
        .expect("Serializing details while changing title silently resolved a progress conflict");
    assert_eq!(remaining["versions"], conflict["versions"]);
    let previous_b = b.export();
    let selected = conflict["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|version| {
            serde_json::from_str::<Value>(version["value"].as_str().unwrap()).unwrap()["progress"]
                == 20
        })
        .unwrap();
    a.call(
        "sync.resolve",
        json!({"kind":"tasks","id":task["id"],"field":"details_json","version_id":selected["id"]}),
    );
    assert_eq!(a.get(&task["id"])["progress"], 20);
    a.merge(vec![previous_b]);
    assert_eq!(
        a.get(&task["id"])["progress"],
        20,
        "Old packet undid an explicit conflict resolution"
    );
    assert_eq!(a.get(&task["id"])["title"], "Only the title changed");
    exchange(&a, &b);
    assert_eq!(b.get(&task["id"])["progress"], 20);
}

#[test]
fn a_concurrent_title_edit_does_not_overwrite_the_notes_format() {
    let a = Peer::new();
    let b = Peer::new();
    let task = a.task("Original title");
    exchange(&a, &b);
    a.call(
        "tasks.update",
        json!({"id":task["id"],"notes":"**Formatted note**","notes_format":"markdown"}),
    );
    b.call(
        "tasks.update",
        json!({"id":task["id"],"title":"Changed title on B"}),
    );
    exchange(&a, &b);
    for peer in [&a, &b] {
        let current = peer.get(&task["id"]);
        assert_eq!(current["title"], "Changed title on B");
        assert_eq!(current["notes"], "**Formatted note**");
        assert_eq!(current["notes_format"], "markdown");
    }
}

#[test]
fn unknown_references_reject_the_entire_packet_before_changing_local_data() {
    let a = Peer::new();
    let b = Peer::new();
    let task = b.task("Malformed reference");
    let mut packet = b.export();
    field(
        &mut packet,
        "tasks",
        task["id"].as_str().unwrap(),
        "list_id",
        json!("unknown-list"),
    );
    let before = a.call("snapshot", json!({}));
    assert!(a
        .service
        .execute("sync.merge", json!({"packets":[packet]}))
        .is_err());
    assert_eq!(a.call("snapshot", json!({})), before);
}

#[test]
fn a_poisoned_maximum_clock_cannot_disable_subsequent_local_edits() {
    let a = Peer::new();
    let b = Peer::new();
    let mut packet = b.export();
    let device = packet["device_id"].as_str().unwrap().to_owned();
    packet["clock"] = json!(i64::MAX);
    packet["vector"] = json!({device.clone():i64::MAX});
    for entity in packet["entities"].as_array_mut().unwrap() {
        for versions in entity["fields"].as_object_mut().unwrap().values_mut() {
            for version in versions.as_array_mut().unwrap() {
                version["clock"] = json!(i64::MAX);
                version["context"] = json!({device.clone():i64::MAX});
            }
        }
    }
    assert!(a
        .service
        .execute("sync.merge", json!({"packets":[packet]}))
        .is_err());
    assert_eq!(a.task("Still editable")["title"], "Still editable");
}

#[test]
fn a_received_clock_at_the_protocol_ceiling_cannot_block_local_edits() {
    let a = Peer::new();
    let b = Peer::new();
    let mut packet = b.export();
    let device = packet["device_id"].as_str().unwrap().to_owned();
    let ceiling = 9_000_000_000_000i64;
    packet["clock"] = json!(ceiling);
    packet["vector"] = json!({device.clone():ceiling});
    for entity in packet["entities"].as_array_mut().unwrap() {
        for versions in entity["fields"].as_object_mut().unwrap().values_mut() {
            for version in versions.as_array_mut().unwrap() {
                version["clock"] = json!(ceiling);
                version["context"] = json!({device.clone():ceiling});
            }
        }
    }
    // Either refuse the impossible advance or keep enough local headroom. In
    // both cases a bad cloud file must not turn the local app read-only.
    let _ = a.service.execute("sync.merge", json!({"packets":[packet]}));
    assert_eq!(
        a.task("Local creation still works")["title"],
        "Local creation still works"
    );
}

#[test]
fn cross_task_comment_attachments_are_rejected_atomically() {
    let a = Peer::new();
    let b = Peer::new();
    let owner = b.task("Attachment owner");
    let other = b.task("Other task");
    let attachment = b.call("attachments.add", json!({"id":owner["id"],"name":"note.txt","mime_type":"text/plain","content_base64":"SGVsbG8="}));
    let attached = b.call(
        "comments.add",
        json!({"id":owner["id"],"body":"Owner comment","attachment_ids":[attachment["id"]]}),
    );
    let foreign = b.call(
        "comments.add",
        json!({"id":other["id"],"body":"Other comment"}),
    );
    let mut packet = b.export();
    let entity = packet["entities"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entity| entity["kind"] == "comment_attachments")
        .unwrap();
    assert_eq!(entity["fields"]["comment_id"][0]["value"], attached["id"]);
    entity["id"] = json!(serde_json::to_string(&json!([foreign["id"], attachment["id"]])).unwrap());
    for version in entity["fields"]["comment_id"].as_array_mut().unwrap() {
        version["value"] = foreign["id"].clone();
    }
    let before = a.call("snapshot", json!({}));
    assert!(a
        .service
        .execute("sync.merge", json!({"packets":[packet]}))
        .is_err());
    assert_eq!(a.call("snapshot", json!({})), before);
}

#[test]
fn reversed_task_links_are_rejected_instead_of_creating_unremovable_duplicates() {
    let a = Peer::new();
    let b = Peer::new();
    let first = b.task("First");
    let second = b.task("Second");
    b.call(
        "links.add",
        json!({"id":first["id"],"linked_task_id":second["id"]}),
    );
    let mut packet = b.export();
    let entity = packet["entities"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entity| entity["kind"] == "task_links")
        .unwrap();
    let original_first = entity["fields"]["task_id"][0]["value"].clone();
    let original_second = entity["fields"]["linked_task_id"][0]["value"].clone();
    entity["id"] = json!(serde_json::to_string(&json!([original_second, original_first])).unwrap());
    for version in entity["fields"]["task_id"].as_array_mut().unwrap() {
        version["value"] = original_second.clone();
    }
    for version in entity["fields"]["linked_task_id"].as_array_mut().unwrap() {
        version["value"] = original_first.clone();
    }
    let before = a.call("snapshot", json!({}));
    assert!(a
        .service
        .execute("sync.merge", json!({"packets":[packet]}))
        .is_err());
    assert_eq!(a.call("snapshot", json!({})), before);
}

#[test]
fn legitimate_attachment_only_comments_and_empty_files_can_be_synced_and_templated() {
    let a = Peer::new();
    let b = Peer::new();
    let task = a.task("Empty file with a comment");
    let attachment = a.call(
        "attachments.add",
        json!({"id":task["id"],"name":"empty.txt","mime_type":"text/plain","content_base64":""}),
    );
    let comment = a.call(
        "comments.add",
        json!({"id":task["id"],"body":"","attachment_ids":[attachment["id"]]}),
    );
    let template = a.call(
        "templates.create",
        json!({"task_id":task["id"],"name":"Empty attachment template"}),
    );
    exchange(&a, &b);
    let received = b.get(&task["id"]);
    assert_eq!(received["comments"][0]["id"], comment["id"]);
    assert_eq!(received["comments"][0]["body"], "");
    assert_eq!(
        received["comments"][0]["attachment_ids"],
        json!([attachment["id"]])
    );
    assert_eq!(received["attachments"][0]["size"], 0);
    let applied = b.call("templates.apply", json!({"id":template["id"]}));
    assert_eq!(applied["attachments"][0]["size"], 0);
    assert_eq!(
        b.call(
            "attachments.get",
            json!({"id":applied["attachments"][0]["id"]})
        )["content_base64"],
        ""
    );
    a.call("attachments.remove", json!({"id":attachment["id"]}));
    exchange(&a, &b);
    assert_eq!(b.get(&task["id"])["comments"][0]["body"], "");
}

#[test]
fn an_offline_filter_survives_a_concurrent_list_deletion_without_blocking_sync() {
    let a = Peer::new();
    let b = Peer::new();
    let list = a.call("lists.create", json!({"name":"Old project"}));
    exchange(&a, &b);
    a.call("lists.delete", json!({"id":list["id"]}));
    b.call("settings.set", json!({"key":"saved_filters","value":[{"id":"offline-filter","name":"Kept filter","list_ids":[list["id"],"inbox"],"tag_ids":[],"priorities":[3],"date":"today","match":"all"}]}));
    exchange(&a, &b);
    for peer in [&a, &b] {
        let filters = peer.call("settings.get", json!({}))["saved_filters"].clone();
        assert_eq!(filters[0]["name"], "Kept filter");
        assert_eq!(filters[0]["list_ids"], json!(["inbox"]));
        assert_eq!(filters[0]["priorities"], json!([3]));
    }
}
