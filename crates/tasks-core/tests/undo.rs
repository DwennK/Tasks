use serde_json::{json, Value};
use tasks_core::TaskService;

fn setup() -> (tempfile::TempDir, TaskService) {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("undo.sqlite3")).unwrap();
    (dir, service)
}
fn call(s: &TaskService, action: &str, input: Value) -> Value {
    s.execute(action, input)
        .unwrap_or_else(|e| panic!("{action}: {e}"))
}
fn record(s: &TaskService, action: &str, input: Value) -> Value {
    call(s, "undo.execute", json!({"action":action,"input":input}))
}
fn undo(s: &TaskService, receipts: &[Value]) -> Value {
    call(
        s,
        "undo.apply",
        json!({"tokens": receipts.iter().map(|r| &r["undo_token"]).collect::<Vec<_>>()}),
    )
}
fn get(s: &TaskService, task: &Value) -> Value {
    call(s, "tasks.get", json!({"id":task["id"]}))
}
fn create(s: &TaskService, title: &str) -> Value {
    call(s, "tasks.create", json!({"title":title}))
}

#[test]
fn metadata_undo_preserves_reminder_ack_and_can_walk_back_several_actions() {
    let (_dir, s) = setup();
    let task = call(
        &s,
        "tasks.create",
        json!({"title":"Before", "due_date":"2026-12-01", "reminders":["2026-12-01T10:00:00Z"], "progress":40}),
    );
    let date = record(
        &s,
        "tasks.update",
        json!({"id":task["id"],"due_date":"2026-12-02"}),
    );
    let priority = record(&s, "tasks.update", json!({"id":task["id"],"priority":3}));
    let due = call(&s, "reminders.due", json!({"now":"2026-12-03T10:00:00Z"}));
    call(
        &s,
        "reminders.mark",
        json!({"id":task["id"],"reminder_at":due[0]["reminder_at"],"generation":due[0]["reminder_generation"]}),
    );
    undo(&s, &[priority]);
    assert_eq!(get(&s, &task)["priority"], 0);
    undo(&s, &[date]);
    let restored = get(&s, &task);
    assert_eq!(restored["due_date"], task["due_date"]);
    assert_eq!(restored["progress"], 40);
    assert!(restored["version"].as_i64().unwrap() > task["version"].as_i64().unwrap());
    assert_eq!(
        call(&s, "reminders.due", json!({"now":"2026-12-03T10:00:00Z"})),
        json!([])
    );
}

#[test]
fn delete_and_completion_restore_exact_descendant_states() {
    let (_dir, s) = setup();
    let root = create(&s, "Root");
    let child = call(
        &s,
        "tasks.create",
        json!({"title":"Done child","parent_id":root["id"]}),
    );
    call(&s, "tasks.complete", json!({"id":child["id"]}));
    let trashed = call(
        &s,
        "tasks.create",
        json!({"title":"Already deleted","parent_id":root["id"]}),
    );
    call(&s, "tasks.delete", json!({"id":trashed["id"]}));
    let completed_before = get(&s, &child)["completed_at"].clone();
    let deleted_before = get(&s, &trashed)["deleted_at"].clone();
    let completed = record(&s, "tasks.complete", json!({"id":root["id"]}));
    let deleted = record(&s, "tasks.delete", json!({"id":root["id"]}));
    undo(&s, &[deleted]);
    assert_eq!(get(&s, &trashed)["deleted_at"], deleted_before);
    undo(&s, &[completed]);
    assert!(get(&s, &root)["completed_at"].is_null());
    assert_eq!(get(&s, &child)["completed_at"], completed_before);
}

#[test]
fn recurring_completion_undo_removes_generated_tree_and_rearms_next_completion() {
    let (_dir, s) = setup();
    let root = call(
        &s,
        "tasks.create",
        json!({"title":"Daily","due_date":"2026-12-01","recurrence":{"frequency":"daily","interval":1}}),
    );
    let child = call(
        &s,
        "tasks.create",
        json!({"title":"Child","parent_id":root["id"]}),
    );
    call(
        &s,
        "attachments.add",
        json!({"id":child["id"],"name":"file.txt","content_base64":"aGk="}),
    );
    let receipt = record(&s, "tasks.complete", json!({"id":root["id"]}));
    let next = receipt["result"]["recurrence_next_id"].clone();
    assert!(next.is_string());
    undo(&s, &[receipt]);
    assert!(get(&s, &root)["recurrence_next_id"].is_null());
    assert!(get(&s, &child)["completed_at"].is_null());
    assert!(s.execute("tasks.get", json!({"id":next})).is_err());
    let again = call(&s, "tasks.complete", json!({"id":root["id"]}));
    assert!(again["recurrence_next_id"].is_string());
    assert_ne!(again["recurrence_next_id"], next);
}

#[test]
fn move_and_reorder_restore_parent_list_section_and_position() {
    let (_dir, s) = setup();
    let list = call(&s, "lists.create", json!({"name":"Destination"}));
    let section = call(
        &s,
        "sections.create",
        json!({"name":"Original","list_id":"inbox"}),
    );
    let root = call(
        &s,
        "tasks.create",
        json!({"title":"Root","section_id":section["id"]}),
    );
    let child = call(
        &s,
        "tasks.create",
        json!({"title":"Child","parent_id":root["id"],"section_id":section["id"]}),
    );
    let other = create(&s, "Other");
    let order = record(&s, "tasks.reorder", json!({"ids":[root["id"],other["id"]]}));
    let moved = record(
        &s,
        "tasks.update",
        json!({"id":root["id"],"list_id":list["id"]}),
    );
    undo(&s, &[moved]);
    assert_eq!(get(&s, &child)["list_id"], "inbox");
    assert_eq!(get(&s, &child)["section_id"], section["id"]);
    assert_eq!(get(&s, &root)["section_id"], section["id"]);
    undo(&s, &[order]);
    assert_eq!(get(&s, &root)["position"], root["position"]);
}

#[test]
fn group_undo_is_atomic_and_conflicts_keep_receipts_and_external_changes() {
    let (_dir, s) = setup();
    let a = create(&s, "A");
    let b = create(&s, "B");
    let first = record(&s, "tasks.update", json!({"id":a["id"],"priority":1}));
    let second = record(&s, "tasks.update", json!({"id":b["id"],"priority":2}));
    call(
        &s,
        "tasks.update",
        json!({"id":a["id"],"notes":"External edit"}),
    );
    let revision = call(&s, "revision", json!({}));
    assert!(s
        .execute(
            "undo.apply",
            json!({"tokens":[first["undo_token"],second["undo_token"]]})
        )
        .is_err());
    assert_eq!(call(&s, "revision", json!({})), revision);
    assert_eq!(get(&s, &a)["notes"], "External edit");
    assert_eq!(get(&s, &b)["priority"], 2);
    // Second receipt was not consumed by the rolled-back attempt.
    undo(&s, &[second]);
    assert_eq!(get(&s, &b)["priority"], 0);
}

#[test]
fn creating_then_undoing_child_does_not_block_earlier_parent_undo() {
    let (_dir, s) = setup();
    let parent = record(&s, "tasks.create", json!({"title":"Parent"}));
    let child = record(
        &s,
        "tasks.create",
        json!({"title":"Child","parent_id":parent["result"]["id"]}),
    );
    undo(&s, &[child]);
    undo(&s, &[parent]);
    assert_eq!(call(&s, "snapshot", json!({}))["tasks"], json!([]));
}

#[test]
fn external_new_child_blocks_undo_of_creation_and_failed_writes_make_no_receipt() {
    let (dir, s) = setup();
    let parent = record(&s, "tasks.create", json!({"title":"Parent"}));
    let child = call(
        &s,
        "tasks.create",
        json!({"title":"External child","parent_id":parent["result"]["id"]}),
    );
    assert!(s
        .execute("undo.apply", json!({"tokens":[parent["undo_token"]]}))
        .is_err());
    assert_eq!(get(&s, &child)["title"], "External child");
    assert!(s
        .execute(
            "undo.execute",
            json!({"action":"tasks.update","input":{"id":child["id"],"priority":99}})
        )
        .is_err());
    let conn = rusqlite::Connection::open(dir.path().join("undo.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM undo_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn missing_original_section_rejects_undo_without_partial_changes() {
    let (_dir, s) = setup();
    let section = call(
        &s,
        "sections.create",
        json!({"name":"Original","list_id":"inbox"}),
    );
    let task = call(
        &s,
        "tasks.create",
        json!({"title":"Task","section_id":section["id"]}),
    );
    let receipt = record(
        &s,
        "tasks.update",
        json!({"id":task["id"],"section_id":null}),
    );
    call(&s, "sections.delete", json!({"id":section["id"]}));
    assert!(s
        .execute("undo.apply", json!({"tokens":[receipt["undo_token"]]}))
        .is_err());
    assert!(get(&s, &task)["section_id"].is_null());
}
