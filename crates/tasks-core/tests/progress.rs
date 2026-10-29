//! Progress uses the same persisted task service as the editor, CLI and reports.
use serde_json::{json, Value};
use tasks_core::TaskService;

fn call(service: &TaskService, action: &str, data: Value) -> Value {
    service
        .execute(action, data.clone())
        .unwrap_or_else(|error| panic!("{action} {data}: {error}"))
}
fn get(service: &TaskService, task: &Value) -> Value {
    call(service, "tasks.get", json!({"id": task["id"]}))
}

#[test]
fn manual_one_hundred_completes_through_the_shared_api_and_repeats_normally() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let completed = call(
        &service,
        "tasks.create",
        json!({"title":"Already done","progress":100}),
    );
    assert!(completed["completed_at"].is_string());
    let repeating = call(
        &service,
        "tasks.create",
        json!({"title":"Repeat","due_date":"2026-11-01","recurrence":{"frequency":"daily"}}),
    );
    let finished = call(
        &service,
        "tasks.update",
        json!({"id":repeating["id"],"progress":100}),
    );
    assert!(finished["completed_at"].is_string());
    let next = call(
        &service,
        "tasks.get",
        json!({"id":finished["recurrence_next_id"]}),
    );
    assert_eq!(next["progress"], 0);
    assert!(next["completed_at"].is_null());
    let retried = call(
        &service,
        "tasks.update",
        json!({"id":repeating["id"],"progress":100}),
    );
    assert_eq!(
        retried["recurrence_next_id"],
        finished["recurrence_next_id"]
    );
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn manual_progress_is_validated_persisted_and_survives_unrelated_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.sqlite3");
    let service = TaskService::open(&path).unwrap();
    let task = call(&service, "tasks.create", json!({"title":"Write proposal"}));
    assert_eq!(task["progress"], 0);
    assert_eq!(task["progress_mode"], "manual");
    call(
        &service,
        "tasks.update",
        json!({"id":task["id"], "progress":42}),
    );
    call(
        &service,
        "tasks.update",
        json!({"id":task["id"], "notes":"Keep working"}),
    );
    assert_eq!(get(&service, &task)["progress"], 42);
    for invalid in [json!(-1), json!(101), json!(1.5), json!("42"), Value::Null] {
        let before = call(&service, "revision", json!({}));
        assert!(service
            .execute("tasks.update", json!({"id":task["id"], "progress":invalid}))
            .is_err());
        assert_eq!(call(&service, "revision", json!({})), before);
        assert_eq!(get(&service, &task)["progress"], 42);
    }
    drop(service);
    let reopened = TaskService::open(&path).unwrap();
    assert_eq!(get(&reopened, &task)["progress"], 42);
    // Databases created before progress was introduced require no destructive migration.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE tasks SET details_json='{}' WHERE id=?1",
            [task["id"].as_str().unwrap()],
        )
        .unwrap();
    assert_eq!(get(&reopened, &task)["progress"], 0);
}

#[test]
fn check_items_derive_progress_without_overwriting_manual_progress_or_completing_task() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let task = call(
        &service,
        "tasks.create",
        json!({"title":"Checklist", "progress":35, "checklist":[
            {"id":"a","text":"First","completed":true},
            {"id":"b","text":"Second","completed":false},
            {"id":"c","text":"Third","completed":false}
        ]}),
    );
    assert_eq!(task["progress"], 33);
    assert_eq!(task["progress_mode"], "checklist");
    let mut items = task["checklist"].clone();
    for item in items.as_array_mut().unwrap() {
        item["completed"] = json!(true);
    }
    let finished = call(
        &service,
        "tasks.update",
        json!({"id":task["id"], "checklist":items}),
    );
    assert_eq!(finished["progress"], 100);
    assert!(finished["completed_at"].is_null());
    call(
        &service,
        "tasks.update",
        json!({"id":task["id"], "title":"All check items done"}),
    );
    let manual = call(
        &service,
        "tasks.update",
        json!({"id":task["id"], "checklist":[]}),
    );
    assert_eq!(manual["progress"], 35);
    assert_eq!(manual["progress_mode"], "manual");
}

#[test]
fn note_checklists_count_only_actual_task_items_and_combine_with_native_checklist() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let html = call(
        &service,
        "tasks.create",
        json!({"title":"HTML checklist", "notes_format":"html", "notes":
        "<ul data-type='taskList'><li data-type='taskItem' data-checked='true'><label><input type='checkbox' checked></label>Done</li><li data-type=taskItem data-checked=false>Pending</li></ul><script>let fake = '<li data-type=taskItem data-checked=true>'</script><li data-checked=true>Ordinary list</li>",
        "checklist":[{"text":"Other step","completed":false}]}),
    );
    assert_eq!(html["progress"], 33);
    assert_eq!(html["progress_mode"], "checklist");
    let markdown = call(
        &service,
        "tasks.create",
        json!({"title":"Markdown checklist", "notes_format":"markdown", "notes":
        "- [x] Finished\n- [ ] Pending\n\n```markdown\n- [x] Code example\n```\n\n`- [x] Inline example`\n"}),
    );
    assert_eq!(markdown["progress"], 50);
    assert_eq!(markdown["progress_mode"], "checklist");
    let plain = call(
        &service,
        "tasks.create",
        json!({"title":"Plain notes", "progress":15,"notes":"- [x] Literal text"}),
    );
    assert_eq!(plain["progress"], 15);
    assert_eq!(plain["progress_mode"], "manual");
}

#[test]
fn completion_reopen_duplicates_templates_and_recurrence_have_independent_progress() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let task = call(
        &service,
        "tasks.create",
        json!({"title":"Project", "progress":70}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Step", "parent_id":task["id"], "progress":20}),
    );
    let complete = call(&service, "tasks.complete", json!({"id":task["id"]}));
    assert_eq!(complete["progress"], 100);
    assert_eq!(get(&service, &child)["progress"], 100);
    call(
        &service,
        "tasks.complete",
        json!({"id":child["id"], "completed":false}),
    );
    assert_eq!(get(&service, &task)["progress"], 0);
    assert_eq!(get(&service, &child)["progress"], 0);
    call(
        &service,
        "tasks.update",
        json!({"id":task["id"], "progress":65}),
    );
    // Adding a child to an already-open parent must not reset manual progress.
    call(
        &service,
        "tasks.create",
        json!({"title":"Another step", "parent_id":task["id"]}),
    );
    assert_eq!(get(&service, &task)["progress"], 65);
    let copy = call(&service, "tasks.duplicate", json!({"id":task["id"]}));
    assert_eq!(copy["progress"], 0);
    let template = call(
        &service,
        "templates.create",
        json!({"name":"Project template", "task_id":task["id"]}),
    );
    let applied = call(&service, "templates.apply", json!({"id":template["id"]}));
    assert_eq!(applied["progress"], 0);
    let repeating = call(
        &service,
        "tasks.create",
        json!({"title":"Repeat", "due_date":"2026-11-01", "progress":80, "recurrence":{"frequency":"daily"}}),
    );
    let finished = call(&service, "tasks.complete", json!({"id":repeating["id"]}));
    assert_eq!(finished["progress"], 100);
    let next = call(
        &service,
        "tasks.get",
        json!({"id":finished["recurrence_next_id"]}),
    );
    assert_eq!(next["progress"], 0);
    assert!(next["completed_at"].is_null());
}

#[test]
fn timeline_preferences_and_summary_templates_round_trip_and_reject_invalid_options() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let list = call(
        &service,
        "lists.create",
        json!({"name":"Timeline", "view_mode":"timeline"}),
    );
    assert_eq!(list["view_mode"], "timeline");
    call(
        &service,
        "settings.set",
        json!({"key":"view_preferences","value":{"today":{"view_mode":"timeline"}}}),
    );
    let template = json!({"id":"weekly", "name":"Weekly summary", "options":{
        "range":"this_week", "from":"2026-10-26", "to":"2026-11-01", "basis":"completed", "list_ids":["deleted-list"], "tag_ids":[],
        "statuses":["completed"], "priorities":[], "group_by":"list", "sort_by":"date", "include_subtasks":true, "fields":["list","progress","notes"]
    }});
    call(
        &service,
        "settings.set",
        json!({"key":"summary_templates", "value":[template.clone()]}),
    );
    let snapshot = call(&service, "snapshot", json!({}));
    assert_eq!(
        snapshot["settings"]["summary_templates"],
        json!([template.clone()])
    );
    for (key, value) in [
        ("from", json!("invalid")),
        ("to", json!("2026-01-01")),
        ("range", json!("all_time")),
        ("fields", json!(["private"])),
        ("priorities", json!([4])),
        ("include_subtasks", json!("yes")),
    ] {
        let mut invalid = template.clone();
        invalid["options"][key] = value;
        assert!(service
            .execute(
                "settings.set",
                json!({"key":"summary_templates", "value":[invalid]})
            )
            .is_err());
    }
    assert!(service
        .execute(
            "settings.set",
            json!({"key":"summary_templates", "value":[template.clone(),template]})
        )
        .is_err());
}
