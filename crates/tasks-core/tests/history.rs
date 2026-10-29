use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use tasks_core::{ServiceError, TaskService};

fn setup() -> (tempfile::TempDir, TaskService) {
    let directory = tempfile::tempdir().unwrap();
    let service = TaskService::open(directory.path().join("history.sqlite3")).unwrap();
    (directory, service)
}
fn call(service: &TaskService, action: &str, input: Value) -> Value {
    service
        .execute(action, input.clone())
        .unwrap_or_else(|error| panic!("{action} {input}: {error}"))
}
fn task(service: &TaskService, title: &str) -> Value {
    call(service, "tasks.create", json!({"title":title}))
}
fn activities(service: &TaskService, task: &Value) -> Value {
    call(service, "tasks.activities", json!({"id":task["id"]}))
}
fn latest(service: &TaskService, task: &Value) -> Value {
    activities(service, task)["items"][0].clone()
}
fn get(service: &TaskService, task: &Value) -> Value {
    call(service, "tasks.get", json!({"id":task["id"]}))
}

#[test]
fn records_content_versions_and_only_changed_fields_without_noop_events() {
    let (_directory, service) = setup();
    let created = call(
        &service,
        "tasks.create",
        json!({"title":"Original", "notes":"Keep **this**", "notes_format":"markdown"}),
    );
    let creation = latest(&service, &created);
    assert!(creation["before"].is_null());
    assert_eq!(creation["after"]["content"]["title"], "Original");
    call(
        &service,
        "tasks.update",
        json!({"id":created["id"],"title":"Changed"}),
    );
    let edit = latest(&service, &created);
    assert_eq!(edit["changed_fields"], json!(["content"]));
    assert_eq!(
        edit["before"]["content"],
        json!({"title":"Original","notes":"Keep **this**","notes_format":"markdown"})
    );
    assert_eq!(edit["after"]["content"]["title"], "Changed");
    assert_eq!(edit["after"]["content"]["notes"], "Keep **this**");
    call(
        &service,
        "tasks.update",
        json!({"id":created["id"],"priority":3}),
    );
    let priority = latest(&service, &created);
    assert_eq!(priority["changed_fields"], json!(["priority"]));
    assert_eq!(priority["before"], json!({"priority":0}));
    assert_eq!(priority["after"], json!({"priority":3}));
    let count = activities(&service, &created)["items"]
        .as_array()
        .unwrap()
        .len();
    call(
        &service,
        "tasks.update",
        json!({"id":created["id"],"title":"Changed","priority":3}),
    );
    assert_eq!(
        activities(&service, &created)["items"]
            .as_array()
            .unwrap()
            .len(),
        count
    );
}

#[test]
fn captures_cascade_move_completion_trash_and_ancestor_reopening() {
    let (_directory, service) = setup();
    let parent = task(&service, "Parent");
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Child","parent_id":parent["id"]}),
    );
    let list = call(&service, "lists.create", json!({"name":"Project"}));
    call(
        &service,
        "tasks.update",
        json!({"id":parent["id"],"list_id":list["id"]}),
    );
    assert_eq!(latest(&service, &child)["after"]["list_id"], list["id"]);
    call(&service, "tasks.complete", json!({"id":parent["id"]}));
    for item in [&parent, &child] {
        let activity = latest(&service, item);
        assert_eq!(activity["action"], "tasks.complete");
        assert!(activity["after"]["completed_at"].is_string());
    }
    call(
        &service,
        "tasks.complete",
        json!({"id":child["id"],"completed":false}),
    );
    let reopened = latest(&service, &parent);
    assert!(reopened["before"]["completed_at"].is_string());
    assert!(reopened["after"]["completed_at"].is_null());
    call(&service, "tasks.delete", json!({"id":parent["id"]}));
    assert!(latest(&service, &child)["after"]["deleted_at"].is_string());
    call(&service, "tasks.restore", json!({"id":child["id"]}));
    assert!(latest(&service, &child)["after"]["deleted_at"].is_null());
    assert_eq!(latest(&service, &parent)["action"], "tasks.restore");
    call(&service, "lists.delete", json!({"id":list["id"]}));
    for item in [&parent, &child] {
        assert_eq!(latest(&service, item)["action"], "lists.delete");
        assert_eq!(latest(&service, item)["after"]["list_id"], "inbox");
    }
}

#[test]
fn recurring_completion_records_the_new_occurrence_and_its_children() {
    let (_directory, service) = setup();
    let parent = call(
        &service,
        "tasks.create",
        json!({"title":"Repeat me","due_date":"2031-06-01","recurrence":{"frequency":"daily","count":3}}),
    );
    call(
        &service,
        "tasks.create",
        json!({"title":"Repeat child","parent_id":parent["id"]}),
    );
    call(&service, "tasks.complete", json!({"id":parent["id"]}));
    let completed = get(&service, &parent);
    let next = get(&service, &json!({"id":completed["recurrence_next_id"]}));
    let creation = latest(&service, &next);
    assert_eq!(creation["action"], "tasks.complete");
    assert!(creation["before"].is_null());
    assert_eq!(creation["after"]["due_date"], "2031-06-02");
    assert_eq!(creation["after"]["content"]["title"], "Repeat me");
    let snapshot = call(&service, "snapshot", json!({}));
    let child = snapshot["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["parent_id"] == next["id"])
        .unwrap();
    let child_creation = latest(&service, child);
    assert!(child_creation["before"].is_null());
    assert_eq!(child_creation["after"]["content"]["title"], "Repeat child");
}

#[test]
fn restores_only_title_and_notes_with_conflict_detection_and_an_activity() {
    let (_directory, service) = setup();
    let original = call(
        &service,
        "tasks.create",
        json!({"title":"Original", "notes":"**Description**", "notes_format":"markdown"}),
    );
    call(
        &service,
        "tasks.update",
        json!({"id":original["id"],"title":"Replacement","notes":"Other description","notes_format":"plain"}),
    );
    let edit = latest(&service, &original);
    call(
        &service,
        "tasks.update",
        json!({"id":original["id"],"due_date":"2031-06-01","priority":3,"pinned":true}),
    );
    call(&service, "tasks.complete", json!({"id":original["id"]}));
    let before = get(&service, &original);
    let count = activities(&service, &original)["items"]
        .as_array()
        .unwrap()
        .len();
    assert!(matches!(service.execute("tasks.restore_content", json!({"id":original["id"],"activity_id":edit["id"],"side":"before","expected_version":original["version"]})), Err(ServiceError::Conflict)));
    assert_eq!(
        activities(&service, &original)["items"]
            .as_array()
            .unwrap()
            .len(),
        count
    );
    let restored = call(
        &service,
        "tasks.restore_content",
        json!({"id":original["id"],"activity_id":edit["id"],"side":"before","expected_version":before["version"]}),
    );
    for key in [
        "due_date",
        "priority",
        "pinned",
        "completed_at",
        "list_id",
        "parent_id",
        "deleted_at",
        "checklist",
        "tag_ids",
        "progress",
    ] {
        assert_eq!(restored[key], before[key], "Restoration changed {key}");
    }
    for key in ["title", "notes", "notes_format"] {
        assert_eq!(restored[key], original[key]);
    }
    assert_eq!(
        latest(&service, &original)["action"],
        "tasks.restore_content"
    );
    let reapplied = call(
        &service,
        "tasks.restore_content",
        json!({"id":original["id"],"activity_id":edit["id"],"side":"after","expected_version":restored["version"]}),
    );
    assert_eq!(reapplied["title"], "Replacement");
    assert_eq!(reapplied["notes_format"], "plain");
    assert_eq!(reapplied["completed_at"], before["completed_at"]);
}

#[test]
fn rejects_foreign_missing_and_deleted_content_versions_atomically() {
    let (_directory, service) = setup();
    let first = task(&service, "First");
    let foreign = task(&service, "Foreign");
    let foreign_creation = latest(&service, &foreign);
    let creation = activities(&service, &first)["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["before"].is_null())
        .unwrap()
        .clone();
    let current = get(&service, &first);
    for input in [
        json!({"id":first["id"],"activity_id":foreign_creation["id"],"side":"after","expected_version":current["version"]}),
        json!({"id":first["id"],"activity_id":creation["id"],"side":"before","expected_version":current["version"]}),
        json!({"id":first["id"],"activity_id":creation["id"],"side":"invalid","expected_version":current["version"]}),
        json!({"id":first["id"],"activity_id":creation["id"],"side":"after"}),
    ] {
        assert!(service.execute("tasks.restore_content", input).is_err());
    }
    assert_eq!(get(&service, &first), current);
    call(&service, "tasks.delete", json!({"id":first["id"]}));
    let deleted = get(&service, &first);
    let history = activities(&service, &first);
    assert_eq!(history["task_deleted"], true);
    assert!(service.execute("tasks.restore_content", json!({"id":first["id"],"activity_id":creation["id"],"side":"after","expected_version":deleted["version"]})).is_err());
    assert_eq!(get(&service, &first), deleted);
    assert_eq!(activities(&service, &first), history);
}

#[test]
fn records_both_link_ends_and_relations_without_attachment_bytes() {
    let (_directory, service) = setup();
    let first = task(&service, "First");
    let second = task(&service, "Second");
    call(
        &service,
        "links.add",
        json!({"id":first["id"],"linked_task_id":second["id"]}),
    );
    for (owner, linked) in [(&first, &second), (&second, &first)] {
        assert_eq!(
            latest(&service, owner)["after"]["linked_task_ids"],
            json!([linked["id"]])
        );
    }
    let before_noop = latest(&service, &first);
    call(
        &service,
        "links.add",
        json!({"id":first["id"],"linked_task_id":second["id"]}),
    );
    assert_eq!(latest(&service, &first), before_noop);
    let bytes = b"PRIVATE_BINARY_PAYLOAD_NEVER_IN_HISTORY";
    let attachment = call(
        &service,
        "attachments.add",
        json!({"id":first["id"],"name":"file.txt","mime_type":"text/plain","content_base64":STANDARD.encode(bytes)}),
    );
    let attached = latest(&service, &first);
    assert_eq!(attached["after"]["attachments"][0]["name"], "file.txt");
    assert!(!attached.to_string().contains("PRIVATE_BINARY"));
    assert!(!attached.to_string().contains(&STANDARD.encode(bytes)));
    assert!(!attached.to_string().contains("content_base64"));
    call(
        &service,
        "comments.add",
        json!({"id":first["id"],"body":"Keep this discussion","attachment_ids":[attachment["id"]]}),
    );
    assert_eq!(
        latest(&service, &first)["after"]["comments"][0]["body"],
        "Keep this discussion"
    );
    assert_eq!(
        latest(&service, &first)["after"]["comment_attachments"][0]["attachment_id"],
        attachment["id"]
    );
    let tag = call(&service, "tags.create", json!({"name":"Tagged"}));
    call(
        &service,
        "tasks.tags",
        json!({"id":first["id"],"tag_ids":[tag["id"]]}),
    );
    call(&service, "tags.delete", json!({"id":tag["id"]}));
    assert_eq!(
        latest(&service, &first)["before"]["tag_ids"],
        json!([tag["id"]])
    );
    assert_eq!(latest(&service, &first)["after"]["tag_ids"], json!([]));
    let database = rusqlite::Connection::open(service.path()).unwrap();
    let stored: String = database
        .query_row(
            "SELECT group_concat(before_json || after_json) FROM task_activities",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!stored.contains("PRIVATE_BINARY"));
    assert!(!stored.contains(&STANDARD.encode(bytes)));
}

#[test]
fn pages_stably_and_reading_does_not_change_revision() {
    let (_directory, service) = setup();
    let item = task(&service, "Page zero");
    for number in 1..6 {
        call(
            &service,
            "tasks.update",
            json!({"id":item["id"],"title":format!("Page {number}")}),
        );
    }
    let revision = call(&service, "revision", json!({}));
    let page = call(
        &service,
        "tasks.activities",
        json!({"id":item["id"],"limit":2}),
    );
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert_eq!(page["next_before_id"], page["items"][1]["id"]);
    let cursor = page["next_before_id"].clone();
    assert_eq!(call(&service, "revision", json!({})), revision);
    call(
        &service,
        "tasks.update",
        json!({"id":item["id"],"title":"New while paging"}),
    );
    let next = call(
        &service,
        "tasks.activities",
        json!({"id":item["id"],"limit":2,"before_id":cursor}),
    );
    assert_eq!(next["items"][0]["after"]["content"]["title"], "Page 3");
    assert_eq!(next["items"][1]["after"]["content"]["title"], "Page 2");
    let last = call(
        &service,
        "tasks.activities",
        json!({"id":item["id"],"limit":2,"before_id":next["next_before_id"]}),
    );
    assert!(last["next_before_id"].is_null());
    for invalid in [
        json!({"limit":0}),
        json!({"limit":101}),
        json!({"before_id":0}),
        json!({"limit":1.5}),
    ] {
        let mut input = invalid.as_object().unwrap().clone();
        input.insert("id".into(), item["id"].clone());
        assert!(service
            .execute("tasks.activities", Value::Object(input))
            .is_err());
    }
}

#[test]
fn migration_keeps_existing_data_without_inventing_past_activities() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old.sqlite3");
    let conn = rusqlite::Connection::open(&path).unwrap();
    for migration in [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_tasks_features.sql"),
        include_str!("../migrations/003_comment_attachments.sql"),
        include_str!("../migrations/004_template_previews.sql"),
        include_str!("../migrations/005_concurrency_tokens.sql"),
        include_str!("../migrations/006_list_metadata.sql"),
    ] {
        conn.execute_batch(migration).unwrap();
    }
    conn.execute_batch("INSERT INTO lists(id,name,color,position,created_at,updated_at) VALUES('inbox','Inbox','#4772fa',0,'2026-01-01','2026-01-01'); INSERT INTO tasks(id,title,notes,list_id,position,created_at,updated_at) VALUES('old','Before upgrade','Retained notes','inbox',0,'2026-01-01','2026-01-01'); PRAGMA user_version=6;").unwrap();
    drop(conn);
    let service = TaskService::open(&path).unwrap();
    let original = get(&service, &json!({"id":"old"}));
    assert_eq!(original["notes"], "Retained notes");
    assert_eq!(activities(&service, &original)["items"], json!([]));
    call(
        &service,
        "tasks.update",
        json!({"id":"old","title":"After upgrade"}),
    );
    assert_eq!(
        latest(&service, &original)["before"]["content"]["title"],
        "Before upgrade"
    );
    assert_eq!(
        latest(&service, &original)["before"]["content"]["notes"],
        "Retained notes"
    );
    let reopened = TaskService::open(&path).unwrap();
    assert_eq!(
        activities(&reopened, &original),
        activities(&service, &original)
    );
}

#[test]
fn failure_rolls_back_mutation_and_activity_together() {
    let (_directory, service) = setup();
    let item = task(&service, "Unchanged");
    let conn = rusqlite::Connection::open(service.path()).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_activity BEFORE INSERT ON task_activities BEGIN SELECT RAISE(ABORT,'history unavailable'); END;").unwrap();
    let revision = call(&service, "revision", json!({}));
    assert!(service
        .execute(
            "tasks.update",
            json!({"id":item["id"],"title":"Must roll back"})
        )
        .is_err());
    assert_eq!(get(&service, &item), item);
    assert_eq!(call(&service, "revision", json!({})), revision);
    assert_eq!(
        activities(&service, &item)["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
