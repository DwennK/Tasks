use serde_json::{json, Value};
use tasks_core::TaskService;
use tempfile::TempDir;

fn setup() -> (TempDir, TaskService) {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    (dir, service)
}

fn call(service: &TaskService, action: &str, input: Value) -> Value {
    service
        .execute(action, input)
        .unwrap_or_else(|error| panic!("{action}: {error}"))
}

fn id(value: &Value) -> &str {
    value["id"].as_str().expect("object ID")
}

fn task(service: &TaskService, title: &str) -> Value {
    call(service, "tasks.create", json!({"title": title}))
}

fn get(service: &TaskService, task: &Value) -> Value {
    call(service, "tasks.get", json!({"id": id(task)}))
}

fn revision(service: &TaskService) -> i64 {
    call(service, "revision", json!({}))["revision"]
        .as_i64()
        .unwrap()
}

fn ids(value: &Value) -> Vec<&str> {
    value.as_array().unwrap().iter().map(id).collect()
}

#[test]
fn empty_database_has_only_inbox_and_is_durable() {
    let (dir, service) = setup();
    let snapshot = call(&service, "snapshot", json!({}));
    assert!(snapshot["tasks"].as_array().unwrap().is_empty());
    assert!(snapshot["folders"].as_array().unwrap().is_empty());
    assert_eq!(snapshot["lists"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["lists"][0]["id"], "inbox");
    assert_eq!(snapshot["settings"]["theme"], "light");
    assert_eq!(snapshot["settings"]["notifications"], false);
    let created = task(&service, "Persist across connections");
    let previous_revision = revision(&service);
    drop(service);
    let reopened = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    assert_eq!(get(&reopened, &created), created);
    assert_eq!(revision(&reopened), previous_revision);
}

#[test]
fn nullable_patches_preserve_omitted_fields_and_detect_stale_versions() {
    let (_dir, service) = setup();
    let initial = call(
        &service,
        "tasks.create",
        json!({
            "title": "Review", "notes": "Keep me", "due_date": "2026-10-27",
            "due_time": "09:30", "reminder_at": "2026-10-27T07:30:00Z", "priority": 3
        }),
    );
    let updated = call(
        &service,
        "tasks.update",
        json!({
            "id": id(&initial), "title": "Reviewed", "expected_version": initial["version"]
        }),
    );
    assert_eq!(updated["notes"], "Keep me");
    assert_eq!(updated["due_date"], "2026-10-27");
    assert_eq!(updated["priority"], 3);
    assert!(updated["version"].as_i64().unwrap() > initial["version"].as_i64().unwrap());
    let current_revision = revision(&service);
    assert!(service
        .execute(
            "tasks.update",
            json!({
                "id": id(&initial), "title": "Stale", "expected_version": initial["version"]
            })
        )
        .is_err());
    assert_eq!(revision(&service), current_revision);
    assert_eq!(get(&service, &initial)["title"], "Reviewed");
    let cleared = call(
        &service,
        "tasks.update",
        json!({
            "id": id(&initial), "due_date": null, "due_time": null, "reminder_at": null
        }),
    );
    for field in ["due_date", "due_time", "reminder_at"] {
        assert!(cleared[field].is_null(), "{field} must be cleared");
    }
    assert_eq!(cleared["notes"], "Keep me");
}

#[test]
fn invalid_input_rolls_back_without_revision_or_partial_task_change() {
    let (_dir, service) = setup();
    let valid = task(&service, "Original");
    let previous_revision = revision(&service);
    for input in [
        json!({"title": ""}),
        json!({"title": "   "}),
        json!({"title": "Bad date", "due_date": "2026-02-30"}),
        json!({"title": "Bad date", "due_date": "27/10/2026"}),
        json!({"title": "Bad time", "due_time": "25:00"}),
        json!({"title": "Bad reminder", "reminder_at": "tomorrow"}),
        json!({"title": "Bad priority", "priority": 4}),
        json!({"title": "Bad priority", "priority": -1}),
        json!({"title": "Bad parent", "parent_id": "missing"}),
        json!({"title": "Bad list", "list_id": "missing"}),
    ] {
        assert!(
            service.execute("tasks.create", input.clone()).is_err(),
            "accepted {input}"
        );
    }
    assert!(service
        .execute(
            "tasks.update",
            json!({
                "id": id(&valid), "title": "Should roll back", "list_id": "missing"
            })
        )
        .is_err());
    assert_eq!(get(&service, &valid)["title"], "Original");
    assert_eq!(revision(&service), previous_revision);
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn subtree_moves_inherit_lists_and_reject_cycles_atomically() {
    let (_dir, service) = setup();
    let list = call(&service, "lists.create", json!({"name": "Work"}));
    let parent = call(
        &service,
        "tasks.create",
        json!({"title": "Parent", "list_id": id(&list)}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title": "Child", "parent_id": id(&parent)}),
    );
    let grandchild = call(
        &service,
        "tasks.create",
        json!({"title": "Grandchild", "parent_id": id(&child)}),
    );
    assert_eq!(child["list_id"], id(&list));
    assert_eq!(grandchild["list_id"], id(&list));
    let previous_revision = revision(&service);
    assert!(service
        .execute(
            "tasks.update",
            json!({"id": id(&parent), "parent_id": id(&grandchild)})
        )
        .is_err());
    assert!(service
        .execute(
            "tasks.update",
            json!({"id": id(&child), "parent_id": id(&child)})
        )
        .is_err());
    assert_eq!(revision(&service), previous_revision);
    assert!(get(&service, &parent)["parent_id"].is_null());
    call(
        &service,
        "tasks.update",
        json!({"id": id(&parent), "list_id": "inbox"}),
    );
    for value in [&parent, &child, &grandchild] {
        assert_eq!(get(&service, value)["list_id"], "inbox");
    }
    let other = call(
        &service,
        "tasks.create",
        json!({"title": "Other", "list_id": id(&list)}),
    );
    call(
        &service,
        "tasks.update",
        json!({"id": id(&child), "parent_id": id(&other)}),
    );
    assert_eq!(get(&service, &child)["list_id"], id(&list));
    assert_eq!(get(&service, &grandchild)["list_id"], id(&list));
}

#[test]
fn completing_subtree_and_reopening_child_maintains_ancestor_consistency() {
    let (_dir, service) = setup();
    let parent = task(&service, "Parent");
    let child = call(
        &service,
        "tasks.create",
        json!({"title": "Child", "parent_id": id(&parent)}),
    );
    let other = call(
        &service,
        "tasks.create",
        json!({"title": "Other child", "parent_id": id(&parent)}),
    );
    let grandchild = call(
        &service,
        "tasks.create",
        json!({"title": "Grandchild", "parent_id": id(&child)}),
    );
    call(&service, "tasks.complete", json!({"id": id(&parent)}));
    for value in [&parent, &child, &other, &grandchild] {
        assert!(get(&service, value)["completed_at"].is_string());
    }
    call(
        &service,
        "tasks.complete",
        json!({"id": id(&grandchild), "completed": false}),
    );
    for value in [&parent, &child, &grandchild] {
        assert!(get(&service, value)["completed_at"].is_null());
    }
    assert!(get(&service, &other)["completed_at"].is_string());
}

#[test]
fn soft_delete_and_restore_keep_content_and_restore_required_ancestors() {
    let (_dir, service) = setup();
    let parent = task(&service, "Parent");
    let child = call(
        &service,
        "tasks.create",
        json!({"title": "Child", "parent_id": id(&parent), "notes": "Retain notes"}),
    );
    let sibling = call(
        &service,
        "tasks.create",
        json!({"title": "Sibling", "parent_id": id(&parent)}),
    );
    call(&service, "tasks.delete", json!({"id": id(&parent)}));
    for value in [&parent, &child, &sibling] {
        assert!(get(&service, value)["deleted_at"].is_string());
    }
    assert!(call(&service, "tasks.list", json!({}))
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({"view": "trash"}))),
        vec![id(&parent)]
    );
    call(&service, "tasks.restore", json!({"id": id(&child)}));
    assert!(get(&service, &parent)["deleted_at"].is_null());
    assert!(get(&service, &child)["deleted_at"].is_null());
    assert_eq!(get(&service, &child)["notes"], "Retain notes");
    call(&service, "tasks.restore", json!({"id": id(&parent)}));
    assert!(get(&service, &sibling)["deleted_at"].is_null());
}

#[test]
fn date_views_search_and_child_filters_return_expected_tasks() {
    let (_dir, service) = setup();
    let overdue = call(
        &service,
        "tasks.create",
        json!({"title": "Overdue", "due_date": "2026-10-26"}),
    );
    let today = call(
        &service,
        "tasks.create",
        json!({"title": "Today", "due_date": "2026-10-27", "notes": "Needle in notes"}),
    );
    let upcoming = call(
        &service,
        "tasks.create",
        json!({"title": "Upcoming", "due_date": "2026-11-02"}),
    );
    let later = call(
        &service,
        "tasks.create",
        json!({"title": "Later", "due_date": "2026-11-03"}),
    );
    let undated = task(&service, "Undated");
    let child = call(
        &service,
        "tasks.create",
        json!({"title": "Nested", "parent_id": id(&undated)}),
    );
    let today_rows = call(
        &service,
        "tasks.list",
        json!({"view": "today", "date": "2026-10-27"}),
    );
    assert_eq!(today_rows.as_array().unwrap().len(), 2);
    assert!(ids(&today_rows).contains(&id(&overdue)));
    assert!(ids(&today_rows).contains(&id(&today)));
    let upcoming_rows = call(
        &service,
        "tasks.list",
        json!({"view": "upcoming", "date": "2026-10-27"}),
    );
    assert_eq!(upcoming_rows.as_array().unwrap().len(), 2);
    assert!(ids(&upcoming_rows).contains(&id(&today)));
    assert!(ids(&upcoming_rows).contains(&id(&upcoming)));
    assert!(!ids(&upcoming_rows).contains(&id(&later)));
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({"search": "needle"}))),
        vec![id(&today)]
    );
    assert_eq!(
        ids(&call(
            &service,
            "tasks.list",
            json!({"parent_id": id(&undated)})
        )),
        vec![id(&child)]
    );
    assert_eq!(
        call(&service, "tasks.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        call(&service, "tasks.list", json!({"include_subtasks": true}))
            .as_array()
            .unwrap()
            .len(),
        6
    );
}

#[test]
fn task_reorder_is_atomic_and_rejects_mixed_scopes() {
    let (_dir, service) = setup();
    let a = task(&service, "A");
    let b = task(&service, "B");
    let c = task(&service, "C");
    call(
        &service,
        "tasks.reorder",
        json!({"ids": [id(&c), id(&a), id(&b)]}),
    );
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({}))),
        vec![id(&c), id(&a), id(&b)]
    );
    let nested = call(
        &service,
        "tasks.create",
        json!({"title": "Nested", "parent_id": id(&a)}),
    );
    let previous_revision = revision(&service);
    for order in [
        json!([id(&a), id(&a)]),
        json!([id(&b), "missing"]),
        json!([id(&a), id(&nested)]),
    ] {
        assert!(service
            .execute("tasks.reorder", json!({"ids": order}))
            .is_err());
    }
    assert_eq!(revision(&service), previous_revision);
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({}))),
        vec![id(&c), id(&a), id(&b)]
    );
}

#[test]
fn folders_reject_cycles_and_deletion_preserves_children_and_lists() {
    let (_dir, service) = setup();
    let root = call(&service, "folders.create", json!({"name": "Root"}));
    let middle = call(
        &service,
        "folders.create",
        json!({"name": "Middle", "parent_id": id(&root)}),
    );
    let child = call(
        &service,
        "folders.create",
        json!({"name": "Child", "parent_id": id(&middle)}),
    );
    let list = call(
        &service,
        "lists.create",
        json!({"name": "Tasks", "folder_id": id(&middle)}),
    );
    let value = call(
        &service,
        "tasks.create",
        json!({"title": "Preserved", "list_id": id(&list)}),
    );
    let previous_revision = revision(&service);
    assert!(service
        .execute(
            "folders.update",
            json!({"id": id(&root), "parent_id": id(&child)})
        )
        .is_err());
    assert_eq!(revision(&service), previous_revision);
    call(&service, "folders.delete", json!({"id": id(&middle)}));
    let snapshot = call(&service, "snapshot", json!({}));
    let folders = snapshot["folders"].as_array().unwrap();
    assert!(!folders.iter().any(|item| id(item) == id(&middle)));
    assert_eq!(
        folders.iter().find(|item| id(item) == id(&child)).unwrap()["parent_id"],
        id(&root)
    );
    assert_eq!(
        snapshot["lists"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| id(item) == id(&list))
            .unwrap()["folder_id"],
        id(&root)
    );
    assert_eq!(get(&service, &value)["list_id"], id(&list));
}

#[test]
fn deleting_a_list_moves_all_tasks_to_inbox_and_inbox_is_protected() {
    let (_dir, service) = setup();
    let list = call(&service, "lists.create", json!({"name": "Temporary"}));
    let value = call(
        &service,
        "tasks.create",
        json!({"title": "Parent", "list_id": id(&list)}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title": "Child", "parent_id": id(&value)}),
    );
    call(&service, "tasks.delete", json!({"id": id(&child)}));
    call(&service, "lists.delete", json!({"id": id(&list)}));
    assert_eq!(get(&service, &value)["list_id"], "inbox");
    assert_eq!(get(&service, &child)["list_id"], "inbox");
    assert!(get(&service, &child)["deleted_at"].is_string());
    assert!(service
        .execute("lists.delete", json!({"id": "inbox"}))
        .is_err());
}

#[test]
fn reminders_are_persistent_and_only_mark_the_current_schedule() {
    let (dir, service) = setup();
    let due = call(
        &service,
        "tasks.create",
        json!({"title": "Due", "reminder_at": "2026-10-27T08:00:00Z"}),
    );
    let completed = call(
        &service,
        "tasks.create",
        json!({"title": "Done", "reminder_at": "2026-10-27T08:00:00Z"}),
    );
    call(&service, "tasks.complete", json!({"id": id(&completed)}));
    let trashed = call(
        &service,
        "tasks.create",
        json!({"title": "Trashed", "reminder_at": "2026-10-27T08:00:00Z"}),
    );
    call(&service, "tasks.delete", json!({"id": id(&trashed)}));
    call(
        &service,
        "tasks.create",
        json!({"title": "Later", "reminder_at": "2026-10-28T08:00:00Z"}),
    );
    assert_eq!(
        ids(&call(
            &service,
            "reminders.due",
            json!({"now": "2026-10-27T09:00:00Z"})
        )),
        vec![id(&due)]
    );
    let generation = call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-27T09:00:00Z"}),
    )[0]["reminder_generation"]
        .clone();
    call(
        &service,
        "reminders.mark",
        json!({"id": id(&due), "reminder_at": due["reminder_at"],"generation":generation}),
    );
    let reopened = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    assert!(call(
        &reopened,
        "reminders.due",
        json!({"now": "2026-10-27T09:00:00Z"})
    )
    .as_array()
    .unwrap()
    .is_empty());
    let updated = call(
        &reopened,
        "tasks.update",
        json!({"id": id(&due), "reminder_at": "2026-10-27T10:00:00Z"}),
    );
    // A delayed acknowledgment must not consume the replacement reminder.
    let _ = reopened.execute(
        "reminders.mark",
        json!({"id": id(&due), "reminder_at": due["reminder_at"],"generation":generation}),
    );
    assert_eq!(
        ids(&call(
            &reopened,
            "reminders.due",
            json!({"now": "2026-10-27T11:00:00Z"})
        )),
        vec![id(&updated)]
    );
}

#[test]
fn settings_are_validated_and_durable() {
    let (dir, service) = setup();
    assert_eq!(
        call(&service, "settings.get", json!({}))["show_list_color"],
        false
    );
    call(
        &service,
        "settings.set",
        json!({"key": "show_list_color", "value": true}),
    );
    call(
        &service,
        "settings.set",
        json!({"key": "theme", "value": "dark"}),
    );
    call(
        &service,
        "settings.set",
        json!({"key": "notifications", "value": true}),
    );
    let previous_revision = revision(&service);
    for invalid in [
        json!({"key": "theme", "value": "neon"}),
        json!({"key": "notifications", "value": "true"}),
        json!({"key": "unknown", "value": "anything"}),
    ] {
        assert!(service.execute("settings.set", invalid).is_err());
    }
    assert_eq!(revision(&service), previous_revision);
    let reopened = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let settings = call(&reopened, "snapshot", json!({}))["settings"].clone();
    assert_eq!(settings["theme"], "dark");
    assert_eq!(settings["notifications"], true);
    assert_eq!(settings["show_list_color"], true);
    assert_eq!(revision(&reopened), previous_revision);
}

#[test]
fn multiple_connections_write_without_lost_tasks_or_revisions() {
    let (dir, service) = setup();
    let previous_revision = revision(&service);
    let path = dir.path().join("tasks.sqlite3");
    let threads: Vec<_> = (0..6)
        .map(|worker| {
            let path = path.clone();
            std::thread::spawn(move || {
                let connection = TaskService::open(path).unwrap();
                for index in 0..12 {
                    task(&connection, &format!("Worker {worker}, task {index}"));
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let snapshot = call(&service, "snapshot", json!({}));
    let tasks = snapshot["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 72);
    let unique: std::collections::HashSet<_> = tasks.iter().map(id).collect();
    assert_eq!(unique.len(), 72);
    assert_eq!(
        snapshot["revision"].as_i64().unwrap(),
        previous_revision + 72
    );
}

#[test]
fn concurrent_stale_updates_allow_exactly_one_writer() {
    let (_dir, service) = setup();
    let original = task(&service, "Initial");
    let previous_revision = revision(&service);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|index| {
            let service = service.clone();
            let original = original.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service
                    .execute(
                        "tasks.update",
                        json!({
                            "id": id(&original), "expected_version": original["version"],
                            "title": format!("Writer {index}")
                        }),
                    )
                    .is_ok()
            })
        })
        .collect();
    let successes = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|success| *success)
        .count();
    assert_eq!(successes, 1);
    assert_eq!(revision(&service), previous_revision + 1);
}

#[test]
fn folder_and_list_reorders_validate_sibling_scope() {
    let (_dir, service) = setup();
    let folder_a = call(&service, "folders.create", json!({"name": "A"}));
    let folder_b = call(&service, "folders.create", json!({"name": "B"}));
    let nested = call(
        &service,
        "folders.create",
        json!({"name": "Nested", "parent_id": id(&folder_a)}),
    );
    call(
        &service,
        "folders.reorder",
        json!({"ids": [id(&folder_b), id(&folder_a)]}),
    );
    let list_a = call(
        &service,
        "lists.create",
        json!({"name": "A", "folder_id": id(&folder_a)}),
    );
    let list_b = call(
        &service,
        "lists.create",
        json!({"name": "B", "folder_id": id(&folder_a)}),
    );
    let other_list = call(
        &service,
        "lists.create",
        json!({"name": "Other", "folder_id": id(&folder_b)}),
    );
    call(
        &service,
        "lists.reorder",
        json!({"ids": [id(&list_b), id(&list_a)]}),
    );
    let snapshot = call(&service, "snapshot", json!({}));
    let folders = snapshot["folders"].as_array().unwrap();
    let position = |collection: &[Value], wanted: &Value| -> i64 {
        collection
            .iter()
            .find(|value| id(value) == id(wanted))
            .unwrap()["position"]
            .as_i64()
            .unwrap()
    };
    assert!(position(folders, &folder_b) < position(folders, &folder_a));
    let lists = snapshot["lists"].as_array().unwrap();
    assert!(position(lists, &list_b) < position(lists, &list_a));
    let previous_revision = revision(&service);
    assert!(service
        .execute(
            "folders.reorder",
            json!({"ids": [id(&folder_a), id(&nested)]})
        )
        .is_err());
    assert!(service
        .execute(
            "lists.reorder",
            json!({"ids": [id(&list_a), id(&other_list)]})
        )
        .is_err());
    assert!(service
        .execute(
            "folders.reorder",
            json!({"ids": [id(&folder_a), id(&folder_a)]})
        )
        .is_err());
    assert!(service
        .execute("lists.reorder", json!({"ids": [id(&list_a), "missing"]}))
        .is_err());
    assert_eq!(revision(&service), previous_revision);
}

#[test]
fn malformed_requests_fail_cleanly_without_writes() {
    let (_dir, service) = setup();
    let previous_revision = revision(&service);
    for (action, input) in [
        ("tasks.create", json!({})),
        ("tasks.create", json!({"title": 123})),
        ("tasks.create", json!([])),
        ("tasks.get", json!({"id": false})),
        ("tasks.update", json!({"id": "missing", "title": "Unknown"})),
        ("tasks.complete", json!({"id": "missing"})),
        ("folders.create", json!({"name": ""})),
        ("lists.create", json!({"name": ""})),
        ("tasks.reorder", json!({"ids": "not-an-array"})),
        ("unknown.operation", json!({})),
    ] {
        assert!(
            service.execute(action, input.clone()).is_err(),
            "accepted {action} {input}"
        );
    }
    assert_eq!(revision(&service), previous_revision);
}

#[test]
fn simultaneous_first_open_migrates_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.sqlite3");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|index| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let service = TaskService::open(path).unwrap();
                task(&service, &format!("First startup {index}"));
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let service = TaskService::open(path).unwrap();
    let snapshot = call(&service, "snapshot", json!({}));
    assert_eq!(snapshot["lists"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["tasks"].as_array().unwrap().len(), 8);
    assert_eq!(snapshot["revision"], 8);
}

#[test]
fn restoring_subtree_reopens_completed_parent_of_restored_active_child() {
    let (_dir, service) = setup();
    let parent = task(&service, "Parent");
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Child", "parent_id":id(&parent)}),
    );
    call(&service, "tasks.delete", json!({"id":id(&child)}));
    call(&service, "tasks.complete", json!({"id":id(&parent)}));
    call(&service, "tasks.delete", json!({"id":id(&parent)}));
    call(&service, "tasks.restore", json!({"id":id(&parent)}));
    assert!(get(&service, &parent)["completed_at"].is_null());
    assert!(get(&service, &child)["deleted_at"].is_null());
}

#[test]
fn date_smart_views_include_due_subtasks_with_undated_parents() {
    let (_dir, service) = setup();
    let parent = task(&service, "Undated parent");
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Due child", "parent_id":id(&parent), "due_date":"2026-10-27"}),
    );
    for view in ["today", "upcoming"] {
        let values = call(
            &service,
            "tasks.list",
            json!({"view":view,"date":"2026-10-27"}),
        );
        assert_eq!(ids(&values), vec![id(&child)]);
    }
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({"view":"all"}))),
        vec![id(&parent)]
    );
}

// Invoked by the process regression below, never by the ordinary test harness.
#[test]
#[ignore = "subprocess probe for cross-process cold-start regression"]
fn process_startup_probe() {
    let path = std::env::var_os("TASKS_CORE_STARTUP_PROBE_DB").expect("probe database path");
    let gate = std::env::var_os("TASKS_CORE_STARTUP_PROBE_GATE").expect("probe gate");
    let gate = std::path::PathBuf::from(gate);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !gate.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "startup gate timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let service = TaskService::open(path).unwrap();
    task(&service, &format!("Process {}", std::process::id()));
}

#[test]
fn concurrent_processes_initialize_fresh_database_without_lost_writes() {
    let dir = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();
    // Multiple genuinely fresh files reproduce the short WAL-negotiation race.
    for round in 0..8 {
        let database = dir.path().join(format!("cold-{round}.sqlite3"));
        let gate = dir.path().join(format!("gate-{round}"));
        let children: Vec<_> = (0..12)
            .map(|_| {
                std::process::Command::new(&executable)
                    .args([
                        "--exact",
                        "process_startup_probe",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("TASKS_CORE_STARTUP_PROBE_DB", &database)
                    .env("TASKS_CORE_STARTUP_PROBE_GATE", &gate)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        std::fs::write(&gate, b"start").unwrap();
        for child in children {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "round {round}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let service = TaskService::open(&database).unwrap();
        let snapshot = call(&service, "snapshot", json!({}));
        assert_eq!(snapshot["lists"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["tasks"].as_array().unwrap().len(), 12);
        assert_eq!(snapshot["revision"], 12);
    }
}

#[test]
fn completed_and_trash_views_expose_independent_children_without_subtree_duplicates() {
    let (_dir, service) = setup();
    let parent = task(&service, "Parent");
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Child", "parent_id":id(&parent)}),
    );
    call(&service, "tasks.complete", json!({"id":id(&child)}));
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({"view":"completed"}))),
        vec![id(&child)]
    );
    assert!(call(
        &service,
        "tasks.list",
        json!({"view":"completed","include_subtasks":false})
    )
    .as_array()
    .unwrap()
    .is_empty());
    call(&service, "tasks.complete", json!({"id":id(&parent)}));
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({"view":"completed"}))),
        vec![id(&parent)]
    );
    assert_eq!(
        call(
            &service,
            "tasks.list",
            json!({"view":"completed","include_subtasks":true})
        )
        .as_array()
        .unwrap()
        .len(),
        2
    );
    assert_eq!(
        ids(&call(
            &service,
            "tasks.list",
            json!({"view":"completed","parent_id":id(&parent)})
        )),
        vec![id(&child)]
    );
    call(
        &service,
        "tasks.complete",
        json!({"id":id(&child),"completed":false}),
    );
    call(&service, "tasks.delete", json!({"id":id(&child)}));
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({"view":"trash"}))),
        vec![id(&child)]
    );
    assert!(call(
        &service,
        "tasks.list",
        json!({"view":"trash","include_subtasks":false})
    )
    .as_array()
    .unwrap()
    .is_empty());
    call(&service, "tasks.delete", json!({"id":id(&parent)}));
    assert_eq!(
        ids(&call(&service, "tasks.list", json!({"view":"trash"}))),
        vec![id(&parent)]
    );
    assert_eq!(
        call(
            &service,
            "tasks.list",
            json!({"view":"trash","include_subtasks":true})
        )
        .as_array()
        .unwrap()
        .len(),
        2
    );
    assert_eq!(
        ids(&call(
            &service,
            "tasks.list",
            json!({"view":"trash","parent_id":id(&parent)})
        )),
        vec![id(&child)]
    );
}
