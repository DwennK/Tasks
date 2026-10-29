//! Integration coverage for the v2 Tasks feature contract.
//! Tests use only TaskService; the migration fixture deliberately constructs a v1 database.
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
        .execute(action, input.clone())
        .unwrap_or_else(|error| panic!("{action} {input}: {error}"))
}
fn id(value: &Value) -> &str {
    value["id"].as_str().expect("object ID")
}
fn get(service: &TaskService, value: &Value) -> Value {
    call(service, "tasks.get", json!({"id":id(value)}))
}
fn revision(service: &TaskService) -> i64 {
    call(service, "revision", json!({}))["revision"]
        .as_i64()
        .unwrap()
}

#[test]
fn search_uses_visible_note_text_and_preserves_literal_plain_and_markdown_code() {
    let (_dir, service) = setup();
    let html = call(
        &service,
        "tasks.create",
        json!({
            "title":"Rich note", "notes_format":"html",
            "notes":"<p>Meet AT&amp;T &lt;team&gt; for <strong>quarterly</strong> review.</p><p data-label=\"x > y\">Next&nbsp;block</p><script>secret_hidden()</script><style>.secret_style{}</style><p>Uni&#x1F680;code</p>"
        }),
    );
    assert_eq!(
        html["notes_text"],
        "Meet AT&T <team> for quarterly review. Next block Uni🚀code"
    );
    for search in [
        "AT&T",
        "quarterly review",
        "review. Next block",
        "<team>",
        "Uni🚀code",
    ] {
        let found = call(&service, "tasks.list", json!({"search":search}));
        assert_eq!(found.as_array().unwrap().len(), 1, "{search}");
        assert_eq!(found[0]["id"], html["id"]);
    }
    for search in ["strong", "secret_hidden", "secret_style", "x > y"] {
        assert_eq!(
            call(&service, "tasks.list", json!({"search":search})),
            json!([])
        );
    }
    let plain = call(
        &service,
        "tasks.create",
        json!({
            "title":"Plain", "notes":"Keep <example> &amp; literally\nwith whitespace."
        }),
    );
    assert_eq!(plain["notes_text"], plain["notes"]);
    assert_eq!(
        call(
            &service,
            "tasks.list",
            json!({"search":"<example> &amp; literally with"})
        )[0]["id"],
        plain["id"]
    );
    let markdown = call(
        &service,
        "tasks.create",
        json!({
            "title":"Markdown", "notes_format":"markdown",
            "notes":"Meet **quarterly** reviewers at [Example](https://hidden-destination.test).\n\n`<literal-code>`\n\n```text\n<block-code> &amp;\n```\n\n- [x] A task"
        }),
    );
    for search in [
        "Meet quarterly reviewers",
        "<literal-code>",
        "<block-code> &amp;",
        "A task",
    ] {
        assert_eq!(
            call(&service, "tasks.list", json!({"search":search}))[0]["id"],
            markdown["id"]
        );
    }
    assert_eq!(
        call(
            &service,
            "tasks.list",
            json!({"search":"hidden-destination"})
        ),
        json!([])
    );
    let updated = call(
        &service,
        "tasks.update",
        json!({"id":id(&html), "notes":"<p>Re<strong>joined</strong> word</p>"}),
    );
    assert_eq!(updated["notes_text"], "Rejoined word");
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == html["id"])
            .unwrap()["notes_text"],
        "Rejoined word"
    );
}
fn task(service: &TaskService, title: &str) -> Value {
    call(service, "tasks.create", json!({"title":title}))
}
fn task_ids(values: &Value) -> Vec<&str> {
    values.as_array().unwrap().iter().map(id).collect()
}
fn setting(service: &TaskService, key: &str, value: Value) {
    call(service, "settings.set", json!({"key":key,"value":value}));
}
fn children(service: &TaskService, parent: &Value) -> Vec<Value> {
    call(
        service,
        "tasks.list",
        json!({"parent_id":id(parent),"include_subtasks":true}),
    )
    .as_array()
    .unwrap()
    .clone()
}

#[test]
fn migrates_v1_in_place_without_losing_tasks_acknowledgements_or_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite3");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    conn.execute_batch(r#"
        INSERT INTO lists(id,name,folder_id,color,position,created_at,updated_at)
        VALUES('inbox','Inbox',NULL,'#4772fa',0,'2026-01-01T00:00:00.000Z','2026-01-01T00:00:00.000Z');
        INSERT INTO tasks(id,title,notes,list_id,due_date,due_time,reminder_at,reminder_notified_at,priority,position,created_at,updated_at,version)
        VALUES('legacy-task','Do not replace me','Legacy notes','inbox','2026-10-27','09:00','2026-10-27T08:00:00.000Z','2026-10-27T08:00:01.000Z',3,4,'2026-01-02T00:00:00.000Z','2026-02-03T00:00:00.000Z',7);
        INSERT INTO tasks(id,title,notes,list_id,reminder_at,position,created_at,updated_at)
        VALUES('pending-reminder','Pending reminder','','inbox','2026-10-27T09:00:00.000Z',5,'2026-01-01T00:00:00.000Z','2026-01-01T00:00:00.000Z');
        INSERT INTO settings(key,value) VALUES('theme','"dark"'),('notifications','true');
        UPDATE metadata SET value=42 WHERE key='revision';
        PRAGMA user_version=1;
    "#).unwrap();
    drop(conn);
    let service = TaskService::open(&path).unwrap();
    let legacy = call(&service, "tasks.get", json!({"id":"legacy-task"}));
    assert_eq!(legacy["title"], "Do not replace me");
    assert_eq!(legacy["notes"], "Legacy notes");
    assert_eq!(legacy["list_id"], "inbox");
    assert_eq!(legacy["priority"], 3);
    assert_eq!(legacy["position"], 4);
    assert_eq!(legacy["version"], 7);
    assert_eq!(legacy["created_at"], "2026-01-02T00:00:00.000Z");
    assert_eq!(legacy["updated_at"], "2026-02-03T00:00:00.000Z");
    assert_eq!(legacy["reminders"], json!(["2026-10-27T08:00:00.000Z"]));
    assert_eq!(legacy["pinned"], false);
    assert!(legacy["recurrence"].is_null());
    assert_eq!(revision(&service), 42);
    let snapshot = call(&service, "snapshot", json!({}));
    assert_eq!(snapshot["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["settings"]["theme"], "dark");
    assert_eq!(snapshot["settings"]["notifications"], true);
    assert_eq!(snapshot["settings"]["default_priority"], 0);
    assert_eq!(snapshot["settings"]["default_list_id"], "inbox");
    let due = call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-28T00:00:00Z"}),
    );
    assert_eq!(task_ids(&due), vec!["pending-reminder"]);
    let reopened = TaskService::open(&path).unwrap();
    assert_eq!(
        call(&reopened, "tasks.get", json!({"id":"legacy-task"})),
        legacy
    );
    assert_eq!(revision(&reopened), 42);
}

#[test]
fn advanced_task_fields_persist_and_partial_updates_preserve_unmentioned_content() {
    let (dir, service) = setup();
    let rich = "<p><strong>A real note</strong></p><ul><li>One</li></ul>";
    let value = call(
        &service,
        "tasks.create",
        json!({
            "title":"A scheduled task","notes":rich,"notes_format":"html","start_date":"2026-10-27","start_time":"08:30",
            "due_date":"2026-10-27","due_time":"10:00","duration_minutes":90,"timezone":"Europe/Zurich","pinned":true,
            "checklist":[{"id":"first","text":"Check one","completed":true},{"id":"second","text":"Check two","completed":false}],
            "recurrence":{"frequency":"weekly","interval":2,"weekdays":[2,4],"count":4},
            "reminders":["2026-10-27T08:45:00Z","2026-10-27T09:00:00Z"]
        }),
    );
    assert_eq!(value["start_date"], "2026-10-27");
    assert_eq!(value["start_time"], "08:30");
    assert_eq!(value["duration_minutes"], 90);
    assert_eq!(value["timezone"], "Europe/Zurich");
    assert_eq!(value["notes_format"], "html");
    assert_eq!(value["notes"], rich);
    assert_eq!(value["pinned"], true);
    assert_eq!(value["recurrence"]["anchor_date"], "2026-10-27");
    assert_eq!(value["reminders"].as_array().unwrap().len(), 2);
    let changed = call(
        &service,
        "tasks.update",
        json!({"id":id(&value),"title":"Only the title changes","expected_version":value["version"]}),
    );
    for field in [
        "start_date",
        "start_time",
        "duration_minutes",
        "timezone",
        "notes",
        "notes_format",
        "pinned",
        "checklist",
        "recurrence",
        "reminders",
    ] {
        assert_eq!(changed[field], value[field], "lost {field}");
    }
    let reopened = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    assert_eq!(get(&reopened, &changed), changed);
    let cleared = call(
        &reopened,
        "tasks.update",
        json!({"id":id(&value),"start_date":null,"duration_minutes":null,"recurrence":null,"reminders":[],"pinned":false,"checklist":[]}),
    );
    for field in [
        "start_date",
        "start_time",
        "duration_minutes",
        "recurrence",
        "reminder_at",
    ] {
        assert!(cleared[field].is_null(), "{field} not cleared");
    }
    assert_eq!(cleared["reminders"], json!([]));
    assert_eq!(cleared["checklist"], json!([]));
    assert_eq!(cleared["pinned"], false);
}

#[test]
fn invalid_scheduling_and_metadata_roll_back_atomically() {
    let (_dir, service) = setup();
    let value = task(&service, "Keep original");
    for patch in [
        json!({"start_time":"09:00"}),
        json!({"start_date":"2026-02-30"}),
        json!({"start_date":"2026-10-28","due_date":"2026-10-27"}),
        json!({"start_date":"2026-10-27","start_time":"11:00","due_date":"2026-10-27","due_time":"10:00"}),
        json!({"duration_minutes":0}),
        json!({"duration_minutes":525601}),
        json!({"duration_minutes":1.5}),
        json!({"timezone":"Invalid/Nowhere"}),
        json!({"recurrence":{"frequency":"daily"}}),
        json!({"due_date":"2026-10-27","recurrence":{"frequency":"weekly","weekdays":[8]}}),
        json!({"reminders":["not a timestamp"]}),
        json!({"reminders":[],"reminder_at":null}),
        json!({"notes_format":"executable"}),
        json!({"pinned":"yes"}),
        json!({"checklist":[{"id":"same","text":"First","completed":false},{"id":"same","text":"Second","completed":false}]}),
        json!({"checklist":[{"id":"item","text":" ","completed":false}]}),
        json!({"section_id":"missing"}),
    ] {
        let mut input = patch.as_object().unwrap().clone();
        input.insert("id".into(), value["id"].clone());
        input.insert("title".into(), json!("Must not persist"));
        let before = revision(&service);
        assert!(
            service
                .execute("tasks.update", Value::Object(input))
                .is_err(),
            "accepted {patch}"
        );
        assert_eq!(revision(&service), before, "revision changed for {patch}");
        assert_eq!(get(&service, &value), value, "partial write for {patch}");
    }
}

#[test]
fn multiple_reminders_acknowledge_independently_and_preserve_matching_acknowledgements() {
    let (_dir, service) = setup();
    let value = call(
        &service,
        "tasks.create",
        json!({"title":"Three reminders","reminders":["2026-10-27T08:00:00Z","2026-10-27T09:00:00+01:00","2026-10-27T09:00:00Z","2026-10-28T08:00:00Z"]}),
    );
    assert_eq!(
        value["reminders"],
        json!([
            "2026-10-27T08:00:00.000Z",
            "2026-10-27T09:00:00.000Z",
            "2026-10-28T08:00:00.000Z"
        ])
    );
    assert_eq!(value["reminder_at"], "2026-10-27T08:00:00.000Z");
    let due = call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-27T10:00:00Z"}),
    );
    assert_eq!(due.as_array().unwrap().len(), 2);
    assert!(due
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["id"] == value["id"]));
    let acknowledged = "2026-10-27T08:00:00.000Z";
    call(
        &service,
        "reminders.mark",
        json!({"id":id(&value),"reminder_at":acknowledged,"generation":due[0]["reminder_generation"]}),
    );
    let due = call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-27T10:00:00Z"}),
    );
    assert_eq!(due.as_array().unwrap().len(), 1);
    assert_eq!(due[0]["reminder_at"], "2026-10-27T09:00:00.000Z");
    call(
        &service,
        "tasks.update",
        json!({"id":id(&value),"reminders":[acknowledged,"2026-10-27T11:00:00Z"]}),
    );
    // A stale delayed acknowledgment for the removed reminder cannot consume its replacement.
    call(
        &service,
        "reminders.mark",
        json!({"id":id(&value),"reminder_at":"2026-10-27T09:00:00Z","generation":due[0]["reminder_generation"]}),
    );
    let due = call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-28T00:00:00Z"}),
    );
    assert_eq!(due.as_array().unwrap().len(), 1);
    assert_eq!(due[0]["reminder_at"], "2026-10-27T11:00:00.000Z");
    let single = call(
        &service,
        "tasks.update",
        json!({"id":id(&value),"reminder_at":"2026-10-29T09:00:00Z"}),
    );
    assert_eq!(single["reminders"], json!(["2026-10-29T09:00:00.000Z"]));
    let cleared = call(
        &service,
        "tasks.update",
        json!({"id":id(&value),"reminder_at":null}),
    );
    assert_eq!(cleared["reminders"], json!([]));
    assert!(call(
        &service,
        "reminders.due",
        json!({"now":"2027-01-01T00:00:00Z"})
    )
    .as_array()
    .unwrap()
    .is_empty());
}

#[test]
fn completing_recurring_task_clones_reset_subtree_and_calendar_offsets_once() {
    let (_dir, service) = setup();
    let tag = call(&service, "tags.create", json!({"name":"Recurring"}));
    let value = call(
        &service,
        "tasks.create",
        json!({
            "title":"Month end","notes":"Keep recurring notes","due_date":"2026-01-31","due_time":"09:00","timezone":"Europe/Zurich",
            "start_date":"2026-01-30","start_time":"09:00","duration_minutes":60,"recurrence":{"frequency":"monthly","count":3},
            "reminders":["2026-01-31T07:45:00Z"],"tag_ids":[id(&tag)],"checklist":[{"id":"item","text":"Reset me","completed":true}]
        }),
    );
    let attachment = call(
        &service,
        "attachments.add",
        json!({"id":id(&value),"name":"note.txt","mime_type":"text/plain","content_base64":"SGVsbG8="}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Child","parent_id":id(&value),"due_date":"2026-01-30","timezone":"Europe/Zurich","checklist":[{"id":"child-item","text":"Child check","completed":true}]}),
    );
    let completed = call(&service, "tasks.complete", json!({"id":id(&value)}));
    assert!(completed["completed_at"].is_string());
    let next_id = completed["recurrence_next_id"]
        .as_str()
        .expect("next occurrence ID");
    let next = call(&service, "tasks.get", json!({"id":next_id}));
    assert_eq!(next["due_date"], "2026-02-28");
    assert_eq!(next["due_time"], "09:00");
    assert_eq!(next["start_date"], "2026-02-27");
    assert_eq!(next["notes"], value["notes"]);
    assert_eq!(next["tag_ids"], value["tag_ids"]);
    assert_eq!(next["reminders"], json!(["2026-02-28T07:45:00.000Z"]));
    assert_eq!(next["checklist"][0]["completed"], false);
    assert!(next["completed_at"].is_null());
    assert!(next["deleted_at"].is_null());
    assert_eq!(next["recurrence"]["anchor_date"], "2026-01-31");
    assert_eq!(next["attachments"].as_array().unwrap().len(), 1);
    let cloned_attachment = &next["attachments"][0];
    assert_ne!(cloned_attachment["id"], attachment["id"]);
    assert_eq!(
        call(
            &service,
            "attachments.get",
            json!({"id":cloned_attachment["id"]})
        )["content_base64"],
        "SGVsbG8="
    );
    let next_children = children(&service, &next);
    assert_eq!(next_children.len(), 1);
    assert_ne!(next_children[0]["id"], child["id"]);
    assert_eq!(next_children[0]["due_date"], "2026-02-27");
    assert_eq!(next_children[0]["checklist"][0]["completed"], false);
    assert!(next_children[0]["completed_at"].is_null());
    let repeated = call(&service, "tasks.complete", json!({"id":id(&value)}));
    assert_eq!(repeated["recurrence_next_id"], next_id);
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let feb = call(&service, "tasks.complete", json!({"id":next_id}));
    let march = call(
        &service,
        "tasks.get",
        json!({"id":feb["recurrence_next_id"]}),
    );
    assert_eq!(march["due_date"], "2026-03-31");
    let ended = call(&service, "tasks.complete", json!({"id":id(&march)}));
    assert!(
        ended["recurrence_next_id"].is_null(),
        "count=3 must end after the third occurrence"
    );
}

#[test]
fn concurrent_completion_creates_exactly_one_next_occurrence() {
    let (dir, service) = setup();
    let value = call(
        &service,
        "tasks.create",
        json!({"title":"Only one next task","due_date":"2026-10-27","recurrence":{"frequency":"daily"}}),
    );
    let path = dir.path().join("tasks.sqlite3");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            let task_id = id(&value).to_owned();
            std::thread::spawn(move || {
                let service = TaskService::open(path).unwrap();
                barrier.wait();
                call(&service, "tasks.complete", json!({"id":task_id}))
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        results[0]["recurrence_next_id"],
        results[1]["recurrence_next_id"]
    );
    assert!(results[0]["recurrence_next_id"].is_string());
    let snapshot = call(&service, "snapshot", json!({}));
    assert_eq!(snapshot["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(
        snapshot["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|task| task["completed_at"].is_null())
            .count(),
        1
    );
    call(
        &service,
        "tasks.complete",
        json!({"id":id(&value),"completed":false}),
    );
    let recompleted = call(&service, "tasks.complete", json!({"id":id(&value)}));
    assert_eq!(
        recompleted["recurrence_next_id"],
        results[0]["recurrence_next_id"]
    );
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn recurrence_end_dates_and_dst_reminders_follow_the_local_calendar() {
    let (_dir, service) = setup();
    let spring = call(
        &service,
        "tasks.create",
        json!({"title":"DST","due_date":"2026-03-28","due_time":"09:00","timezone":"Europe/Zurich","recurrence":{"frequency":"daily","end_date":"2026-03-29"},"reminders":["2026-03-28T07:45:00Z"]}),
    );
    let done = call(&service, "tasks.complete", json!({"id":id(&spring)}));
    let next = call(
        &service,
        "tasks.get",
        json!({"id":done["recurrence_next_id"]}),
    );
    assert_eq!(next["due_date"], "2026-03-29");
    assert_eq!(next["due_time"], "09:00");
    assert_eq!(next["reminders"], json!(["2026-03-29T06:45:00.000Z"]));
    let last = call(&service, "tasks.complete", json!({"id":id(&next)}));
    assert!(last["recurrence_next_id"].is_null());
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn wont_do_dismisses_subtrees_and_reactivation_reopens_required_ancestors() {
    let (_dir, service) = setup();
    let parent = call(
        &service,
        "tasks.create",
        json!({"title":"Parent","pinned":true}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Child","parent_id":id(&parent)}),
    );
    let dismissed = call(&service, "tasks.wont_do", json!({"id":id(&parent)}));
    assert!(dismissed["wont_do_at"].is_string());
    assert!(get(&service, &child)["wont_do_at"].is_string());
    assert!(call(&service, "tasks.list", json!({"view":"all"}))
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        task_ids(&call(&service, "tasks.list", json!({"view":"wont_do"}))),
        vec![id(&parent)]
    );
    assert_eq!(
        call(
            &service,
            "tasks.list",
            json!({"view":"wont_do","include_subtasks":true})
        )
        .as_array()
        .unwrap()
        .len(),
        2
    );
    call(
        &service,
        "tasks.wont_do",
        json!({"id":id(&child),"wont_do":false}),
    );
    assert!(get(&service, &parent)["wont_do_at"].is_null());
    assert!(get(&service, &child)["wont_do_at"].is_null());
    assert_eq!(get(&service, &parent)["pinned"], true);
}

#[test]
fn duplicate_makes_new_ids_preserves_content_and_resets_terminal_status() {
    let (_dir, service) = setup();
    let destination = call(&service, "lists.create", json!({"name":"Destination"}));
    let tag = call(&service, "tags.create", json!({"name":"Keep tag"}));
    let parent = call(
        &service,
        "tasks.create",
        json!({"title":"Source","notes":"**Content**","notes_format":"markdown","pinned":true,"tag_ids":[id(&tag)],"checklist":[{"id":"check","text":"A checked item","completed":true}]}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Source child","parent_id":id(&parent)}),
    );
    call(
        &service,
        "attachments.add",
        json!({"id":id(&parent),"name":"local.txt","content_base64":"SGVsbG8="}),
    );
    call(&service, "tasks.complete", json!({"id":id(&parent)}));
    call(&service, "tasks.delete", json!({"id":id(&parent)}));
    let duplicated = call(
        &service,
        "tasks.duplicate",
        json!({"id":id(&parent),"list_id":id(&destination),"title":"A clean copy"}),
    );
    assert_ne!(duplicated["id"], parent["id"]);
    assert_eq!(duplicated["title"], "A clean copy");
    assert_eq!(duplicated["notes"], parent["notes"]);
    assert_eq!(duplicated["notes_format"], "markdown");
    assert_eq!(duplicated["pinned"], true);
    assert_eq!(duplicated["tag_ids"], parent["tag_ids"]);
    assert_eq!(duplicated["list_id"], destination["id"]);
    for field in [
        "completed_at",
        "deleted_at",
        "wont_do_at",
        "recurrence_next_id",
    ] {
        assert!(duplicated[field].is_null(), "{field} was inherited");
    }
    assert_eq!(duplicated["checklist"][0]["text"], "A checked item");
    assert_eq!(duplicated["checklist"][0]["completed"], false);
    assert_ne!(
        duplicated["checklist"][0]["id"],
        parent["checklist"][0]["id"]
    );
    assert_eq!(duplicated["attachments"].as_array().unwrap().len(), 1);
    let copied_children = children(&service, &duplicated);
    assert_eq!(copied_children.len(), 1);
    assert_ne!(copied_children[0]["id"], child["id"]);
    assert_eq!(copied_children[0]["list_id"], destination["id"]);
    assert!(copied_children[0]["completed_at"].is_null());
    assert!(copied_children[0]["deleted_at"].is_null());
    assert!(
        get(&service, &parent)["deleted_at"].is_string(),
        "source must remain untouched"
    );
}

#[test]
fn sections_enforce_list_membership_and_reassignment_without_losing_tasks() {
    let (_dir, service) = setup();
    let first = call(&service, "lists.create", json!({"name":"First"}));
    let second = call(&service, "lists.create", json!({"name":"Second"}));
    let section = call(
        &service,
        "sections.create",
        json!({"list_id":id(&first),"name":"Section"}),
    );
    let other_section = call(
        &service,
        "sections.create",
        json!({"list_id":id(&second),"name":"Other"}),
    );
    let parent = call(
        &service,
        "tasks.create",
        json!({"title":"Assigned","list_id":id(&first),"section_id":id(&section)}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Child","parent_id":id(&parent),"section_id":id(&section)}),
    );
    let before = revision(&service);
    assert!(service
        .execute(
            "tasks.update",
            json!({"id":id(&parent),"section_id":id(&other_section)})
        )
        .is_err());
    assert_eq!(revision(&service), before);
    let moved = call(
        &service,
        "tasks.update",
        json!({"id":id(&parent),"list_id":id(&second)}),
    );
    assert_eq!(moved["list_id"], second["id"]);
    assert!(moved["section_id"].is_null());
    assert!(get(&service, &child)["section_id"].is_null());
    call(
        &service,
        "tasks.update",
        json!({"id":id(&parent),"section_id":id(&other_section)}),
    );
    call(
        &service,
        "sections.delete",
        json!({"id":id(&other_section)}),
    );
    assert!(get(&service, &parent)["section_id"].is_null());
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn section_ordering_is_scoped_atomic_and_persisted() {
    let (dir, service) = setup();
    let list = call(&service, "lists.create", json!({"name":"List"}));
    let another = call(&service, "lists.create", json!({"name":"Other list"}));
    let first = call(
        &service,
        "sections.create",
        json!({"list_id":id(&list),"name":"First"}),
    );
    let second = call(
        &service,
        "sections.create",
        json!({"list_id":id(&list),"name":"Second"}),
    );
    let other = call(
        &service,
        "sections.create",
        json!({"list_id":id(&another),"name":"Other"}),
    );
    call(
        &service,
        "sections.reorder",
        json!({"ids":[id(&second),id(&first)]}),
    );
    let before = revision(&service);
    assert!(service
        .execute("sections.reorder", json!({"ids":[id(&first),id(&other)]}))
        .is_err());
    assert!(service
        .execute("sections.reorder", json!({"ids":[id(&first),id(&first)]}))
        .is_err());
    assert_eq!(revision(&service), before);
    let reopened = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let snapshot = call(&reopened, "snapshot", json!({}));
    let sections: Vec<_> = snapshot["sections"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|section| section["list_id"] == list["id"])
        .map(id)
        .collect();
    assert_eq!(sections, vec![id(&second), id(&first)]);
}

#[test]
fn archived_lists_are_hidden_from_active_smart_views_but_explicitly_accessible() {
    let (_dir, service) = setup();
    let list = call(
        &service,
        "lists.create",
        json!({"name":"Archive me","view_mode":"board","group_by":"section","sort_by":"priority","sort_order":"desc"}),
    );
    let value = call(
        &service,
        "tasks.create",
        json!({"title":"Keep this task","list_id":id(&list),"due_date":"2026-10-27"}),
    );
    call(&service, "lists.archive", json!({"id":id(&list)}));
    let snapshot = call(&service, "snapshot", json!({}));
    let archived = snapshot["lists"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == list["id"])
        .unwrap();
    assert!(archived["archived_at"].is_string());
    assert_eq!(archived["view_mode"], "board");
    assert_eq!(archived["group_by"], "section");
    assert_eq!(archived["sort_by"], "priority");
    assert_eq!(archived["sort_order"], "desc");
    for view in ["all", "today", "upcoming"] {
        assert!(call(
            &service,
            "tasks.list",
            json!({"view":view,"date":"2026-10-27"})
        )
        .as_array()
        .unwrap()
        .is_empty());
    }
    assert_eq!(
        task_ids(&call(&service, "tasks.list", json!({"list_id":id(&list)}))),
        vec![id(&value)]
    );
    call(&service, "tasks.complete", json!({"id":id(&value)}));
    assert_eq!(
        task_ids(&call(&service, "tasks.list", json!({"view":"completed"}))),
        vec![id(&value)]
    );
    call(
        &service,
        "lists.archive",
        json!({"id":id(&list),"archived":false}),
    );
    call(
        &service,
        "tasks.complete",
        json!({"id":id(&value),"completed":false}),
    );
    assert_eq!(
        task_ids(&call(&service, "tasks.list", json!({"view":"all"}))),
        vec![id(&value)]
    );
    assert!(service
        .execute("lists.archive", json!({"id":"inbox"}))
        .is_err());
}

#[test]
fn explicit_parent_queries_include_hidden_and_archived_list_children_only_in_requested_status() {
    let (_dir, service) = setup();
    for archived in [false, true] {
        let list = call(
            &service,
            "lists.create",
            json!({"name":format!("Scoped {archived}"), "show_in_smart_lists":archived}),
        );
        let parent = call(
            &service,
            "tasks.create",
            json!({"title":"Parent", "list_id":id(&list)}),
        );
        let child = call(
            &service,
            "tasks.create",
            json!({"title":"Child", "parent_id":id(&parent)}),
        );
        let completed = call(
            &service,
            "tasks.create",
            json!({"title":"Completed", "parent_id":id(&parent)}),
        );
        let deleted = call(
            &service,
            "tasks.create",
            json!({"title":"Deleted", "parent_id":id(&parent)}),
        );
        call(&service, "tasks.complete", json!({"id":id(&completed)}));
        call(&service, "tasks.delete", json!({"id":id(&deleted)}));
        if archived {
            call(&service, "lists.archive", json!({"id":id(&list)}));
        }
        let active = call(&service, "tasks.list", json!({"parent_id":id(&parent)}));
        assert_eq!(active.as_array().unwrap().len(), 1);
        assert_eq!(active[0]["id"], child["id"]);
        assert_eq!(
            call(
                &service,
                "tasks.list",
                json!({"parent_id":id(&parent), "view":"completed"})
            )[0]["id"],
            completed["id"]
        );
        assert_eq!(
            call(
                &service,
                "tasks.list",
                json!({"parent_id":id(&parent), "view":"trash"})
            )[0]["id"],
            deleted["id"]
        );
        assert_eq!(
            call(&service, "tasks.list", json!({"parent_id":null})),
            json!([])
        );
        assert_eq!(
            call(&service, "tasks.list", json!({"include_subtasks":true})),
            json!([])
        );
    }
}

#[test]
fn templates_capture_content_and_apply_independent_subtrees_to_selected_list() {
    let (_dir, service) = setup();
    let source = call(
        &service,
        "tasks.create",
        json!({"title":"Reusable","notes":"Captured content","notes_format":"markdown","priority":2,"checklist":[{"id":"step","text":"First step","completed":false}]}),
    );
    call(
        &service,
        "tasks.create",
        json!({"title":"Nested step","parent_id":id(&source)}),
    );
    let template = call(
        &service,
        "templates.create",
        json!({"name":"Routine","task_id":id(&source)}),
    );
    assert_eq!(template["name"], "Routine");
    call(
        &service,
        "tasks.update",
        json!({"id":id(&source),"notes":"Changed later"}),
    );
    let target = call(&service, "lists.create", json!({"name":"New list"}));
    let applied = call(
        &service,
        "templates.apply",
        json!({"id":id(&template),"list_id":id(&target)}),
    );
    assert_ne!(applied["id"], source["id"]);
    assert_eq!(applied["notes"], "Captured content");
    assert_eq!(applied["priority"], 2);
    assert_eq!(applied["list_id"], target["id"]);
    assert_eq!(applied["checklist"][0]["text"], "First step");
    assert_eq!(children(&service, &applied).len(), 1);
    assert_eq!(children(&service, &applied)[0]["list_id"], target["id"]);
    let again = call(
        &service,
        "templates.apply",
        json!({"id":id(&template),"list_id":id(&target)}),
    );
    assert_ne!(again["id"], applied["id"]);
    assert_ne!(
        children(&service, &again)[0]["id"],
        children(&service, &applied)[0]["id"]
    );
    call(
        &service,
        "templates.update",
        json!({"id":id(&template),"name":"Renamed routine"}),
    );
    let snapshot = call(&service, "snapshot", json!({}));
    assert_eq!(snapshot["templates"][0]["name"], "Renamed routine");
    call(&service, "templates.delete", json!({"id":id(&template)}));
    assert!(call(&service, "snapshot", json!({}))["templates"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(get(&service, &applied)["title"], "Reusable");
}

#[test]
fn task_creation_respects_saved_defaults_and_explicit_values_override_them() {
    let (_dir, service) = setup();
    let list = call(&service, "lists.create", json!({"name":"Default list"}));
    let tag = call(&service, "tags.create", json!({"name":"Default tag"}));
    setting(&service, "default_list_id", list["id"].clone());
    setting(&service, "default_priority", json!(3));
    setting(&service, "default_tag_id", tag["id"].clone());
    setting(&service, "default_reminder_timed", json!("15m"));
    setting(&service, "default_add_to", json!("top"));
    let initial = task(&service, "First");
    let value = call(
        &service,
        "tasks.create",
        json!({"title":"Defaults","due_date":"2026-10-27","due_time":"09:00","timezone":"Europe/Zurich"}),
    );
    assert_eq!(value["list_id"], list["id"]);
    assert_eq!(value["priority"], 3);
    assert_eq!(value["tag_ids"], json!([id(&tag)]));
    assert_eq!(value["reminders"], json!(["2026-10-27T07:45:00.000Z"]));
    assert!(value["position"].as_i64().unwrap() < initial["position"].as_i64().unwrap());
    let explicit = call(
        &service,
        "tasks.create",
        json!({"title":"Explicit","list_id":"inbox","priority":0,"tag_ids":[],"reminders":[],"due_date":null}),
    );
    assert_eq!(explicit["list_id"], "inbox");
    assert_eq!(explicit["priority"], 0);
    assert_eq!(explicit["tag_ids"], json!([]));
    assert_eq!(explicit["reminders"], json!([]));
}

#[test]
fn deleted_or_archived_default_destinations_do_not_break_quick_task_creation() {
    let (_dir, service) = setup();
    let list = call(
        &service,
        "lists.create",
        json!({"name":"Default destination"}),
    );
    let tag = call(&service, "tags.create", json!({"name":"Temporary default"}));
    setting(&service, "default_list_id", list["id"].clone());
    setting(&service, "default_tag_id", tag["id"].clone());
    call(&service, "tags.delete", json!({"id":id(&tag)}));
    call(&service, "lists.archive", json!({"id":id(&list)}));
    let created = task(&service, "Still able to capture a task");
    assert_eq!(created["list_id"], "inbox");
    assert_eq!(created["tag_ids"], json!([]));
    let snapshot = call(&service, "snapshot", json!({}));
    assert_eq!(snapshot["settings"]["default_list_id"], "inbox");
    assert!(snapshot["settings"]["default_tag_id"].is_null());
    call(
        &service,
        "lists.archive",
        json!({"id":id(&list),"archived":false}),
    );
    setting(&service, "default_list_id", list["id"].clone());
    call(&service, "lists.delete", json!({"id":id(&list)}));
    assert_eq!(
        task(&service, "Task after list deletion")["list_id"],
        "inbox"
    );
}

#[test]
fn advanced_preferences_are_durable_and_invalid_updates_are_atomic() {
    let (dir, service) = setup();
    let preferences = json!({"today":{"view_mode":"list","group_by":"priority","sort_by":"due_date","sort_order":"asc"}});
    for (key, value) in [
        (
            "smart_lists",
            json!({"today":"show","tomorrow":"auto","all":"hide"}),
        ),
        ("theme_color", json!("lilac")),
        ("font_size", json!("large")),
        ("week_start", json!(0)),
        ("time_format", json!(12)),
        ("view_preferences", preferences.clone()),
        ("default_reminder_allday", json!("18:00")),
        ("default_date", json!("tomorrow")),
        ("completion_sound", json!(false)),
    ] {
        setting(&service, key, value);
    }
    let before = revision(&service);
    for (key, value) in [
        ("smart_lists", json!({"today":"sometimes"})),
        ("theme_color", json!("invalid")),
        ("font_size", json!("enormous")),
        ("week_start", json!(4)),
        ("time_format", json!(13)),
        ("default_list_id", json!("missing")),
        ("default_tag_id", json!("missing")),
        ("view_preferences", json!({"today":{"group_by":"unknown"}})),
        ("default_reminder_allday", json!("bad time")),
        ("default_priority", json!(4)),
    ] {
        assert!(
            service
                .execute("settings.set", json!({"key":key,"value":value}))
                .is_err(),
            "accepted {key} {value}"
        );
    }
    assert_eq!(revision(&service), before);
    let reopened = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let settings = call(&reopened, "snapshot", json!({}))["settings"].clone();
    assert_eq!(settings["theme_color"], "lilac");
    assert_eq!(settings["view_preferences"], preferences);
    assert_eq!(settings["completion_sound"], false);
    let created = call(
        &reopened,
        "tasks.create",
        json!({"title":"Tomorrow's task","timezone":"UTC"}),
    );
    let expected = (chrono::Local::now().date_naive() + chrono::Duration::days(1)).to_string();
    assert_eq!(created["due_date"], expected);
    assert_eq!(
        created["reminders"],
        json!([format!("{expected}T18:00:00.000Z")])
    );
}

#[test]
fn native_interface_and_dock_preferences_default_on_existing_databases_and_validate_atomically() {
    let (dir, service) = setup();
    let defaults = [
        ("interface_style", json!("card")),
        ("app_icon", json!("default")),
        ("show_dock_icon", json!(true)),
        ("dock_count", json!("today")),
    ];
    // Simulate an existing schema6 database created before these JSON keys existed.
    let conn = rusqlite::Connection::open(service.path()).unwrap();
    for (key, _) in &defaults {
        conn.execute("DELETE FROM settings WHERE key=?1", [key])
            .unwrap();
    }
    let before = revision(&service);
    let settings = call(&service, "snapshot", json!({}))["settings"].clone();
    for (key, value) in &defaults {
        assert_eq!(settings[*key], *value);
    }
    assert_eq!(revision(&service), before);
    let changes = [
        ("interface_style", json!("flat")),
        ("app_icon", json!("rounded")),
        ("show_dock_icon", json!(false)),
        ("dock_count", json!("overdue")),
    ];
    for (key, value) in &changes {
        setting(&service, key, value.clone());
    }
    let before = revision(&service);
    for (key, value) in [
        ("interface_style", json!("glass")),
        ("app_icon", json!("custom")),
        ("show_dock_icon", json!(0)),
        ("dock_count", json!("notes")),
        ("dock_count", json!(null)),
    ] {
        assert!(service
            .execute("settings.set", json!({"key":key,"value":value}))
            .is_err());
    }
    assert_eq!(revision(&service), before);
    let reopened = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let settings = call(&reopened, "snapshot", json!({}))["settings"].clone();
    for (key, value) in &changes {
        assert_eq!(settings[*key], *value);
    }
    assert_eq!(
        conn.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        9
    );
    for mode in ["none", "today", "inbox", "all", "overdue"] {
        setting(&service, "dock_count", json!(mode));
    }
}

#[test]
fn comment_attachments_are_owned_by_the_task_and_partial_edits_preserve_associations() {
    let (_dir, service) = setup();
    let owner = task(&service, "With a discussion");
    let other = task(&service, "Different task");
    let file = call(
        &service,
        "attachments.add",
        json!({"id":id(&owner),"name":"note.txt","mime_type":"text/plain","content_base64":"SGVsbG8="}),
    );
    let foreign = call(
        &service,
        "attachments.add",
        json!({"id":id(&other),"name":"other.txt","content_base64":"SGVsbG8="}),
    );
    let comment = call(
        &service,
        "comments.add",
        json!({"id":id(&owner),"attachment_ids":[id(&file)]}),
    );
    assert_eq!(comment["body"], "");
    assert_eq!(comment["attachment_ids"], json!([id(&file)]));
    assert_eq!(get(&service, &owner)["comments"][0]["id"], comment["id"]);
    let edited = call(
        &service,
        "comments.update",
        json!({"id":id(&comment),"body":"Now there is text"}),
    );
    assert_eq!(edited["attachment_ids"], comment["attachment_ids"]);
    let before = revision(&service);
    let before_task = get(&service, &owner);
    assert!(service
        .execute(
            "comments.update",
            json!({"id":id(&comment),"body":"Must roll back","attachment_ids":[id(&foreign)]})
        )
        .is_err());
    assert!(service
        .execute(
            "comments.add",
            json!({"id":id(&owner),"body":"","attachment_ids":[]})
        )
        .is_err());
    assert_eq!(revision(&service), before);
    assert_eq!(get(&service, &owner), before_task);
    let detached = call(
        &service,
        "comments.update",
        json!({"id":id(&comment),"attachment_ids":[]}),
    );
    assert_eq!(detached["body"], "Now there is text");
    assert_eq!(detached["attachment_ids"], json!([]));
    call(
        &service,
        "comments.update",
        json!({"id":id(&comment),"attachment_ids":[id(&file)]}),
    );
    call(&service, "attachments.remove", json!({"id":id(&file)}));
    let comments = call(&service, "comments.list", json!({"id":id(&owner)}));
    assert_eq!(comments[0]["body"], "Now there is text");
    assert_eq!(comments[0]["attachment_ids"], json!([]));
    call(&service, "comments.delete", json!({"id":id(&comment)}));
    assert!(get(&service, &owner)["comments"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        call(&service, "attachments.get", json!({"id":id(&foreign)}))["content_base64"],
        "SGVsbG8="
    );
}

#[test]
fn saved_filters_validate_references_and_prune_only_deleted_list_and_tag_ids() {
    let (_dir, service) = setup();
    let list = call(&service, "lists.create", json!({"name":"Filtered list"}));
    let remove = call(
        &service,
        "tags.create",
        json!({"name":"Remove from filter"}),
    );
    let keep = call(&service, "tags.create", json!({"name":"Keep in filter"}));
    let filters = json!([{"id":"work-filter","name":"My work","list_ids":[id(&list),"inbox"],"tag_ids":[id(&remove),id(&keep)],"priorities":[1,3],"date":"today","match":"all"}]);
    setting(&service, "saved_filters", filters.clone());
    let before = revision(&service);
    for invalid in [
        json!([{"id":"bad","name":"Unknown list","list_ids":["missing"],"tag_ids":[],"priorities":[],"date":"all","match":"all"}]),
        json!([{"id":"bad","name":"Unknown tag","list_ids":[],"tag_ids":["missing"],"priorities":[],"date":"all","match":"all"}]),
        json!([{"id":"bad","name":"Unknown priority","list_ids":[],"tag_ids":[],"priorities":[4],"date":"all","match":"all"}]),
        json!([{"id":"bad","name":"Unknown date","list_ids":[],"tag_ids":[],"priorities":[],"date":"next-year","match":"all"}]),
        json!([filters[0].clone(), filters[0].clone()]),
    ] {
        assert!(service
            .execute(
                "settings.set",
                json!({"key":"saved_filters","value":invalid})
            )
            .is_err());
    }
    assert_eq!(revision(&service), before);
    call(&service, "tags.delete", json!({"id":id(&remove)}));
    call(&service, "lists.delete", json!({"id":id(&list)}));
    let actual = call(&service, "snapshot", json!({}))["settings"]["saved_filters"].clone();
    assert_eq!(actual.as_array().unwrap().len(), 1);
    assert_eq!(actual[0]["id"], "work-filter");
    assert_eq!(actual[0]["name"], "My work");
    assert_eq!(actual[0]["list_ids"], json!(["inbox"]));
    assert_eq!(actual[0]["tag_ids"], json!([id(&keep)]));
    assert_eq!(actual[0]["priorities"], json!([1, 3]));
    assert_eq!(actual[0]["date"], "today");
    assert_eq!(actual[0]["match"], "all");
    // A pruned filter remains valid for the next edit/save.
    setting(&service, "saved_filters", actual);
}

#[test]
fn setting_first_due_date_applies_default_reminder_using_the_requested_timezone() {
    let (_dir, service) = setup();
    setting(&service, "default_reminder_timed", json!("15m"));
    setting(&service, "default_reminder_allday", json!("09:00"));
    let undated = call(
        &service,
        "tasks.create",
        json!({"title":"Schedule me later","timezone":"UTC"}),
    );
    assert_eq!(undated["reminders"], json!([]));
    let scheduled = call(
        &service,
        "tasks.update",
        json!({"id":id(&undated),"due_date":"2026-10-27","due_time":"09:00","timezone":"Europe/Zurich"}),
    );
    assert_eq!(scheduled["timezone"], "Europe/Zurich");
    assert_eq!(scheduled["reminders"], json!(["2026-10-27T07:45:00.000Z"]));
    let all_day = call(
        &service,
        "tasks.create",
        json!({"title":"All-day task","timezone":"UTC"}),
    );
    let all_day = call(
        &service,
        "tasks.update",
        json!({"id":id(&all_day),"due_date":"2026-10-27"}),
    );
    assert_eq!(all_day["reminders"], json!(["2026-10-27T09:00:00.000Z"]));
    let explicit = call(
        &service,
        "tasks.create",
        json!({"title":"No reminder","timezone":"UTC"}),
    );
    let explicit = call(
        &service,
        "tasks.update",
        json!({"id":id(&explicit),"due_date":"2026-10-27","due_time":"09:00","reminders":[]}),
    );
    assert_eq!(explicit["reminders"], json!([]));
    let manual = call(
        &service,
        "tasks.create",
        json!({"title":"Existing manual reminder","timezone":"UTC","reminders":["2026-10-26T18:00:00Z"]}),
    );
    let manual = call(
        &service,
        "tasks.update",
        json!({"id":id(&manual),"due_date":"2026-10-27","due_time":"09:00"}),
    );
    assert_eq!(manual["reminders"], json!(["2026-10-26T18:00:00.000Z"]));
}

#[test]
fn template_attachment_payloads_do_not_enter_snapshot_and_survive_source_removal() {
    use base64::Engine;
    let (_dir, service) = setup();
    let source = call(&service, "tasks.create", json!({"title":"Template source"}));
    let bytes = vec![42u8; 512 * 1024];
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let file = call(
        &service,
        "attachments.add",
        json!({"id":source["id"],"name":"local.bin","mime_type":"application/octet-stream","content_base64":encoded}),
    );
    let template = call(
        &service,
        "templates.create",
        json!({"name":"With a file","task_id":source["id"]}),
    );
    call(&service, "attachments.remove", json!({"id":file["id"]}));
    let snapshot = call(&service, "snapshot", json!({})).to_string();
    assert!(
        snapshot.len() < 20000,
        "snapshot loaded binary template payload"
    );
    assert!(!snapshot.contains("content_base64"));
    let applied = call(&service, "templates.apply", json!({"id":template["id"]}));
    let copied = call(
        &service,
        "attachments.get",
        json!({"id":applied["attachments"][0]["id"]}),
    );
    assert_eq!(copied["content_base64"].as_str().unwrap(), encoded);
}

#[test]
fn editing_or_moving_a_dismissed_child_does_not_reactivate_ancestors() {
    let (_dir, service) = setup();
    let parent = call(
        &service,
        "tasks.create",
        json!({"title":"Dismissed parent"}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Dismissed child","parent_id":parent["id"]}),
    );
    call(&service, "tasks.wont_do", json!({"id":parent["id"]}));
    let parent_before = call(&service, "tasks.get", json!({"id":parent["id"]}));
    let child_before = call(&service, "tasks.get", json!({"id":child["id"]}));
    let edited = call(
        &service,
        "tasks.update",
        json!({"id":child["id"],"notes":"Kept as a record","expected_version":child_before["version"]}),
    );
    assert_eq!(edited["wont_do_at"], child_before["wont_do_at"]);
    assert_eq!(
        call(&service, "tasks.get", json!({"id":parent["id"]})),
        parent_before
    );
    let destination = call(
        &service,
        "tasks.create",
        json!({"title":"Another dismissed parent"}),
    );
    call(&service, "tasks.wont_do", json!({"id":destination["id"]}));
    let destination_before = call(&service, "tasks.get", json!({"id":destination["id"]}));
    call(
        &service,
        "tasks.update",
        json!({"id":child["id"],"parent_id":destination["id"]}),
    );
    assert_eq!(
        call(&service, "tasks.get", json!({"id":destination["id"]})),
        destination_before
    );
    assert_eq!(
        call(&service, "tasks.get", json!({"id":parent["id"]})),
        parent_before
    );
    // Explicit reactivation, unlike a metadata edit, must reopen the ancestor.
    call(
        &service,
        "tasks.complete",
        json!({"id":child["id"],"completed":false}),
    );
    assert!(call(&service, "tasks.get", json!({"id":destination["id"]}))["wont_do_at"].is_null());
    assert!(call(&service, "tasks.get", json!({"id":parent["id"]}))["wont_do_at"].is_string());
}

#[test]
fn delete_restore_and_parent_completion_preserve_dismissed_descendant_states() {
    let (_dir, service) = setup();
    let parent = call(&service, "tasks.create", json!({"title":"Parent"}));
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Child","parent_id":parent["id"]}),
    );
    call(&service, "tasks.wont_do", json!({"id":parent["id"]}));
    let dismissed = call(&service, "tasks.get", json!({"id":child["id"]}))["wont_do_at"].clone();
    call(&service, "tasks.delete", json!({"id":parent["id"]}));
    call(&service, "tasks.restore", json!({"id":parent["id"]}));
    assert_eq!(
        call(&service, "tasks.get", json!({"id":child["id"]}))["wont_do_at"],
        dismissed
    );
    assert!(call(&service, "tasks.get", json!({"id":parent["id"]}))["wont_do_at"].is_string());
    call(&service, "tasks.delete", json!({"id":child["id"]}));
    call(&service, "tasks.complete", json!({"id":parent["id"]}));
    let trashed = call(&service, "tasks.get", json!({"id":child["id"]}));
    assert_eq!(trashed["wont_do_at"], dismissed);
    assert!(trashed["completed_at"].is_null());
    assert!(trashed["deleted_at"].is_string());
    call(&service, "tasks.restore", json!({"id":child["id"]}));
    assert_eq!(
        call(&service, "tasks.get", json!({"id":child["id"]}))["wont_do_at"],
        dismissed
    );
    assert!(call(&service, "tasks.get", json!({"id":parent["id"]}))["completed_at"].is_string());
    call(
        &service,
        "tasks.wont_do",
        json!({"id":child["id"],"wont_do":false}),
    );
    assert!(call(&service, "tasks.get", json!({"id":parent["id"]}))["completed_at"].is_null());
}

#[test]
fn duplicating_active_subtree_does_not_resurrect_independently_trashed_children() {
    let (_dir, service) = setup();
    let parent = call(&service, "tasks.create", json!({"title":"Parent"}));
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Discarded child","parent_id":parent["id"]}),
    );
    call(&service, "tasks.delete", json!({"id":child["id"]}));
    let copy = call(&service, "tasks.duplicate", json!({"id":parent["id"]}));
    assert!(
        call(&service, "tasks.list", json!({"parent_id":copy["id"]}))
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rich_note_checkboxes_reset_in_new_occurrences_duplicates_and_templates() {
    let (_dir, service) = setup();
    let notes = r#"<p>Keep this <strong>text</strong>.</p><ul data-type="taskList"><li data-type="taskItem" data-checked="true"><p>First</p></li><li data-checked='true' data-type='taskItem'><p>Second</p></li><li data-type="taskItem" data-checked="false"><p>Third</p></li></ul>"#;
    let source = call(
        &service,
        "tasks.create",
        json!({"title":"Rich checklist","notes":notes,"notes_format":"html","due_date":"2026-10-27","recurrence":{"frequency":"daily","count":2}}),
    );
    let copy = call(&service, "tasks.duplicate", json!({"id":source["id"]}));
    let template = call(
        &service,
        "templates.create",
        json!({"name":"Rich template","task_id":source["id"]}),
    );
    let applied = call(&service, "templates.apply", json!({"id":template["id"]}));
    let finished = call(&service, "tasks.complete", json!({"id":source["id"]}));
    let next = call(
        &service,
        "tasks.get",
        json!({"id":finished["recurrence_next_id"]}),
    );
    for item in [copy, applied, next] {
        let notes = item["notes"].as_str().unwrap();
        assert_eq!(
            notes.matches("data-checked=\"false\"").count(),
            3,
            "{notes}"
        );
        assert!(notes.contains("Keep this <strong>text</strong>."));
        assert!(!notes.contains("data-checked=\"true\""));
        assert!(!notes.contains("data-checked='true'"));
    }
    assert_eq!(
        call(&service, "tasks.get", json!({"id":source["id"]}))["notes"],
        notes
    );
}

#[test]
fn nested_recurrence_advances_only_the_latest_child_series_without_forking() {
    let (_dir, service) = setup();
    let parent = call(
        &service,
        "tasks.create",
        json!({"title":"Recurring parent","due_date":"2026-10-27","recurrence":{"frequency":"daily","count":3}}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Recurring child","parent_id":parent["id"],"due_date":"2026-10-27","recurrence":{"frequency":"daily","count":3}}),
    );
    let first_child_finished = call(&service, "tasks.complete", json!({"id":child["id"]}));
    let finished = call(&service, "tasks.complete", json!({"id":parent["id"]}));
    let next_id = finished["recurrence_next_id"].clone();
    let children = call(&service, "tasks.list", json!({"parent_id":next_id}));
    assert_eq!(children.as_array().unwrap().len(), 1);
    assert_eq!(children[0]["due_date"], "2026-10-29");
    assert_eq!(children[0]["recurrence"]["count"], 1);
    let previous_child = call(
        &service,
        "tasks.get",
        json!({"id":first_child_finished["recurrence_next_id"]}),
    );
    assert_eq!(previous_child["recurrence_next_id"], children[0]["id"]);
    let before = call(&service, "snapshot", json!({}))["tasks"]
        .as_array()
        .unwrap()
        .len();
    call(
        &service,
        "tasks.complete",
        json!({"id":previous_child["id"],"completed":false}),
    );
    call(
        &service,
        "tasks.complete",
        json!({"id":previous_child["id"]}),
    );
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        before
    );
    let second = call(&service, "tasks.complete", json!({"id":next_id}));
    let final_children = call(
        &service,
        "tasks.list",
        json!({"parent_id":second["recurrence_next_id"]}),
    );
    assert!(final_children.as_array().unwrap().is_empty());
}

#[test]
fn a_stale_reminder_acknowledgement_cannot_consume_same_timestamp_rearmed_reminder() {
    let (_dir, service) = setup();
    let at = "2026-10-27T08:00:00Z";
    let task = call(
        &service,
        "tasks.create",
        json!({"title":"Reminder","reminders":[at]}),
    );
    let old = call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-27T09:00:00Z"}),
    )[0]
    .clone();
    call(
        &service,
        "tasks.update",
        json!({"id":task["id"],"reminders":[]}),
    );
    call(
        &service,
        "tasks.update",
        json!({"id":task["id"],"reminders":[at]}),
    );
    assert!(service
        .execute("reminders.mark", json!({"id":task["id"],"reminder_at":at}))
        .is_err());
    call(
        &service,
        "reminders.mark",
        json!({"id":task["id"],"reminder_at":at,"generation":old["reminder_generation"]}),
    );
    let current = call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-27T09:00:00Z"}),
    );
    assert_eq!(current.as_array().unwrap().len(), 1);
    assert_ne!(
        current[0]["reminder_generation"],
        old["reminder_generation"]
    );
    call(
        &service,
        "reminders.mark",
        json!({"id":task["id"],"reminder_at":at,"generation":current[0]["reminder_generation"]}),
    );
    assert!(call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-27T09:00:00Z"})
    )
    .as_array()
    .unwrap()
    .is_empty());
}

#[test]
fn optimistic_tag_and_comment_updates_preserve_newer_content() {
    let (_dir, service) = setup();
    let task = call(
        &service,
        "tasks.create",
        json!({"title":"Collaborative task"}),
    );
    let a = call(&service, "tags.create", json!({"name":"A"}));
    let b = call(&service, "tags.create", json!({"name":"B"}));
    let first = call(
        &service,
        "tasks.tags",
        json!({"id":task["id"],"tag_ids":[a["id"]],"expected_version":task["version"]}),
    );
    let newer = call(
        &service,
        "tasks.tags",
        json!({"id":task["id"],"tag_ids":[a["id"],b["id"]],"expected_version":first["version"]}),
    );
    assert!(service
        .execute(
            "tasks.tags",
            json!({"id":task["id"],"tag_ids":[a["id"]],"expected_version":first["version"]})
        )
        .is_err());
    assert_eq!(
        call(&service, "tasks.get", json!({"id":task["id"]}))["tag_ids"],
        newer["tag_ids"]
    );
    let comment = call(
        &service,
        "comments.add",
        json!({"id":task["id"],"body":"Original"}),
    );
    let changed = call(
        &service,
        "comments.update",
        json!({"id":comment["id"],"body":"Newer text","expected_version":comment["version"]}),
    );
    assert!(service
        .execute(
            "comments.update",
            json!({"id":comment["id"],"body":"Stale text","expected_version":comment["version"]})
        )
        .is_err());
    assert!(service
        .execute(
            "comments.delete",
            json!({"id":comment["id"],"expected_version":comment["version"]})
        )
        .is_err());
    assert_eq!(
        call(&service, "comments.list", json!({"id":task["id"]}))[0],
        changed
    );
}

#[test]
fn markdown_checklist_reset_preserves_code_literals_and_every_other_source_byte() {
    let (_dir, service) = setup();
    let notes="Exemple café 📝:\n```markdown\n- [x] Fenced literal\n```\n\n~~~text\n* [X] Tilde-fenced literal\n~~~\n\n    - [x] Indented literal\n\nInline `- [x]` and escaped \\- [x] stay unchanged.\n\n- [x] Actual task\n  - [X] Nested task\n- [ ] Already unchecked\n\n> ```markdown\n> - [x] Quoted fenced literal\n> ```\n\n> - [X] Actual quoted task\n";
    let expected = notes
        .replace("- [x] Actual task", "- [ ] Actual task")
        .replace("  - [X] Nested task", "  - [ ] Nested task")
        .replace("> - [X] Actual quoted task", "> - [ ] Actual quoted task");
    let source = call(
        &service,
        "tasks.create",
        json!({"title":"Markdown checklist","notes_format":"markdown","notes":notes,"due_date":"2026-10-27","recurrence":{"frequency":"daily","count":2}}),
    );
    let copy = call(&service, "tasks.duplicate", json!({"id":source["id"]}));
    assert_eq!(copy["notes"], expected);
    let template = call(
        &service,
        "templates.create",
        json!({"name":"Markdown template","task_id":source["id"]}),
    );
    let applied = call(&service, "templates.apply", json!({"id":template["id"]}));
    assert_eq!(applied["notes"], expected);
    let finished = call(&service, "tasks.complete", json!({"id":source["id"]}));
    let next = call(
        &service,
        "tasks.get",
        json!({"id":finished["recurrence_next_id"]}),
    );
    assert_eq!(next["notes"], expected);
    assert_eq!(
        call(&service, "tasks.get", json!({"id":source["id"]}))["notes"],
        notes
    );
}

#[test]
fn list_folder_metadata_and_note_defaults_persist_and_control_smart_views() {
    let (_dir, service) = setup();
    let folder = call(
        &service,
        "folders.create",
        json!({"name":"Design","icon":"🎨","pinned":true}),
    );
    assert_eq!(folder["icon"], "🎨");
    assert_eq!(folder["pinned"], true);
    let list = call(
        &service,
        "lists.create",
        json!({"name":"Notebook","folder_id":folder["id"],"icon":"📚","color":"","pinned":true,"list_type":"note"}),
    );
    assert_eq!(list["color"], "");
    assert_eq!(list["list_type"], "note");
    assert_eq!(list["pinned"], true);
    let note = call(
        &service,
        "tasks.create",
        json!({"title":"An actual note","list_id":list["id"],"due_date":"2026-10-27","notes":"Keep this content"}),
    );
    assert_eq!(note["is_note"], true);
    let explicit = call(
        &service,
        "tasks.create",
        json!({"title":"Explicit task","list_id":list["id"],"is_note":false}),
    );
    assert_eq!(explicit["is_note"], false);
    let converted = call(
        &service,
        "tasks.update",
        json!({"id":note["id"],"is_note":false,"expected_version":note["version"]}),
    );
    assert_eq!(converted["is_note"], false);
    assert_eq!(converted["due_date"], note["due_date"]);
    assert_eq!(converted["notes"], note["notes"]);
    call(
        &service,
        "lists.update",
        json!({"id":list["id"],"show_in_smart_lists":false,"list_type":"task","icon":null}),
    );
    for view in ["all", "today", "upcoming"] {
        assert!(call(
            &service,
            "tasks.list",
            json!({"view":view,"date":"2026-10-27"})
        )
        .as_array()
        .unwrap()
        .is_empty());
    }
    assert_eq!(
        call(&service, "tasks.list", json!({"list_id":list["id"]}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        call(&service, "tasks.list", json!({"search":"actual note"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let new_task = call(
        &service,
        "tasks.create",
        json!({"title":"New task","list_id":list["id"]}),
    );
    assert_eq!(new_task["is_note"], false);
    call(
        &service,
        "folders.update",
        json!({"id":folder["id"],"icon":"","pinned":false}),
    );
    let snapshot = call(&service, "snapshot", json!({}));
    assert_eq!(snapshot["folders"][0]["icon"], "");
    assert_eq!(snapshot["folders"][0]["pinned"], false);
    let revision = snapshot["revision"].clone();
    for invalid in [
        json!({"id":list["id"],"list_type":"project"}),
        json!({"id":list["id"],"pinned":"yes"}),
        json!({"id":list["id"],"show_in_smart_lists":null}),
        json!({"id":list["id"],"icon":"x".repeat(65)}),
    ] {
        assert!(service.execute("lists.update", invalid).is_err());
    }
    assert_eq!(call(&service, "revision", json!({}))["revision"], revision);
}

#[test]
fn duplicate_list_remaps_sections_hierarchy_recurrence_and_links_preserving_task_states() {
    use base64::Engine;
    let (_dir, service) = setup();
    let source = call(
        &service,
        "lists.create",
        json!({"name":"Source","icon":"🧪","color":"#abcdef","pinned":true,"view_mode":"board","group_by":"section","show_in_smart_lists":false}),
    );
    let section = call(
        &service,
        "sections.create",
        json!({"list_id":source["id"],"name":"First section"}),
    );
    let tag = call(&service, "tags.create", json!({"name":"Important"}));
    let parent = call(
        &service,
        "tasks.create",
        json!({"title":"Parent","list_id":source["id"],"section_id":section["id"],"tag_ids":[tag["id"]],"checklist":[{"id":"source-check","text":"Done item","completed":true}]}),
    );
    let child = call(
        &service,
        "tasks.create",
        json!({"title":"Child note","parent_id":parent["id"],"is_note":true}),
    );
    let outside = call(
        &service,
        "tasks.create",
        json!({"title":"Outside reference"}),
    );
    call(
        &service,
        "links.add",
        json!({"id":parent["id"],"linked_task_id":child["id"]}),
    );
    call(
        &service,
        "links.add",
        json!({"id":parent["id"],"linked_task_id":outside["id"]}),
    );
    let bytes = base64::engine::general_purpose::STANDARD.encode("Independent file");
    call(
        &service,
        "attachments.add",
        json!({"id":child["id"],"name":"note.txt","content_base64":bytes}),
    );
    let completed = call(
        &service,
        "tasks.create",
        json!({"title":"Already completed","list_id":source["id"]}),
    );
    call(&service, "tasks.complete", json!({"id":completed["id"]}));
    let dismissed = call(
        &service,
        "tasks.create",
        json!({"title":"Dismissed","list_id":source["id"]}),
    );
    call(&service, "tasks.wont_do", json!({"id":dismissed["id"]}));
    let trashed = call(
        &service,
        "tasks.create",
        json!({"title":"Discarded","list_id":source["id"]}),
    );
    call(&service, "tasks.delete", json!({"id":trashed["id"]}));
    let recurring = call(
        &service,
        "tasks.create",
        json!({"title":"Recurring","list_id":source["id"],"due_date":"2026-10-27","recurrence":{"frequency":"daily","count":3}}),
    );
    let old_occurrence = call(&service, "tasks.complete", json!({"id":recurring["id"]}));
    let duplicate = call(
        &service,
        "lists.duplicate",
        json!({"id":source["id"],"name":"Copied list"}),
    );
    assert_eq!(duplicate["name"], "Copied list");
    assert_eq!(duplicate["icon"], source["icon"]);
    assert_eq!(duplicate["pinned"], false);
    assert_eq!(duplicate["show_in_smart_lists"], false);
    assert_eq!(duplicate["view_mode"], "board");
    let snapshot = call(&service, "snapshot", json!({}));
    let copied: Vec<_> = snapshot["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["list_id"] == duplicate["id"])
        .collect();
    assert_eq!(copied.len(), 6);
    assert!(!copied.iter().any(|t| t["title"] == "Discarded"));
    let new_parent = *copied.iter().find(|t| t["title"] == "Parent").unwrap();
    let new_child = *copied.iter().find(|t| t["title"] == "Child note").unwrap();
    assert_eq!(new_child["parent_id"], new_parent["id"]);
    assert_eq!(new_child["is_note"], true);
    assert_ne!(new_parent["section_id"], section["id"]);
    assert!(snapshot["sections"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == new_parent["section_id"] && s["list_id"] == duplicate["id"]));
    assert_eq!(new_parent["tag_ids"], json!([tag["id"]]));
    assert_eq!(new_parent["checklist"][0]["completed"], true);
    assert_ne!(new_parent["checklist"][0]["id"], "source-check");
    assert!(new_parent["linked_task_ids"]
        .as_array()
        .unwrap()
        .contains(&new_child["id"]));
    assert!(new_parent["linked_task_ids"]
        .as_array()
        .unwrap()
        .contains(&outside["id"]));
    assert!(!new_parent["linked_task_ids"]
        .as_array()
        .unwrap()
        .contains(&child["id"]));
    assert_eq!(
        call(
            &service,
            "attachments.get",
            json!({"id":new_child["attachments"][0]["id"]})
        )["content_base64"],
        bytes
    );
    assert!(copied
        .iter()
        .find(|t| t["title"] == "Already completed")
        .unwrap()["completed_at"]
        .is_string());
    assert!(copied.iter().find(|t| t["title"] == "Dismissed").unwrap()["wont_do_at"].is_string());
    let old_copy = *copied
        .iter()
        .find(|t| t["title"] == "Recurring" && t["completed_at"].is_string())
        .unwrap();
    let next_copy = *copied
        .iter()
        .find(|t| t["id"] == old_copy["recurrence_next_id"])
        .unwrap();
    assert_ne!(next_copy["id"], old_occurrence["recurrence_next_id"]);
    let count_before = snapshot["tasks"].as_array().unwrap().len();
    call(
        &service,
        "tasks.complete",
        json!({"id":old_copy["id"],"completed":false}),
    );
    call(&service, "tasks.complete", json!({"id":old_copy["id"]}));
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        count_before
    );
}
