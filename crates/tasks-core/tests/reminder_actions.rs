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
fn capture(service: &TaskService) -> Value {
    call(
        service,
        "reminders.capture",
        json!({"now":"2026-10-29T09:00:00Z"}),
    )["pending"]
        .clone()
}
fn action(reminder: &Value, name: &str) -> Value {
    json!({"id":reminder["id"],"reminder_at":reminder["reminder_at"],"generation":reminder["reminder_generation"],"action":name})
}
fn task(service: &TaskService) -> Value {
    call(
        service,
        "tasks.create",
        json!({"title":"Reminder","due_date":"2026-10-30","reminders":["2026-10-29T08:00:00Z","2026-10-30T08:00:00Z"]}),
    )
}

#[test]
fn capture_persists_controls_and_delivery_together_across_restart() {
    let (dir, service) = setup();
    let original = task(&service);
    let pending = capture(&service);
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert_eq!(pending[0]["id"], original["id"]);
    let reopened = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    assert_eq!(call(&reopened, "reminders.pending", json!({})), pending);
    assert_eq!(
        call(
            &reopened,
            "reminders.due",
            json!({"now":"2026-10-29T09:00:00Z"})
        ),
        json!([])
    );
    let repeated = call(
        &reopened,
        "reminders.capture",
        json!({"now":"2026-10-29T09:00:00Z"}),
    );
    assert_eq!(repeated["captured"], json!([]));
    assert_eq!(repeated["pending"], pending);
}

#[test]
fn snooze_preserves_deadline_and_other_alarms_and_invalidates_old_controls() {
    let (_dir, service) = setup();
    let original = task(&service);
    let pending = capture(&service);
    let stale_done = action(&pending[0], "done");
    let mut snooze = action(&pending[0], "snooze");
    snooze["minutes"] = json!(10);
    snooze["now"] = json!("2026-10-29T09:00:00Z");
    assert_eq!(call(&service, "reminders.resolve", snooze)["ok"], true);
    let updated = call(&service, "tasks.get", json!({"id":original["id"]}));
    assert_eq!(updated["due_date"], original["due_date"]);
    assert!(updated["completed_at"].is_null());
    assert_eq!(
        updated["reminders"],
        json!(["2026-10-29T09:10:00.000Z", "2026-10-30T08:00:00.000Z"])
    );
    assert_eq!(call(&service, "reminders.pending", json!({})), json!([]));
    assert_eq!(
        call(&service, "reminders.resolve", stale_done)["stale"],
        true
    );
    assert_eq!(
        call(
            &service,
            "reminders.due",
            json!({"now":"2026-10-29T09:09:59Z"})
        ),
        json!([])
    );
    assert_eq!(
        call(
            &service,
            "reminders.due",
            json!({"now":"2026-10-29T09:10:00Z"})
        )[0]["id"],
        original["id"]
    );
}

#[test]
fn dismiss_only_acknowledges_the_selected_alarm() {
    let (_dir, service) = setup();
    let original = task(&service);
    let pending = capture(&service);
    assert_eq!(
        call(
            &service,
            "reminders.resolve",
            action(&pending[0], "dismiss")
        )["ok"],
        true
    );
    assert_eq!(
        call(&service, "tasks.get", json!({"id":original["id"]})),
        original
    );
    assert_eq!(call(&service, "reminders.pending", json!({})), json!([]));
    assert_eq!(
        call(
            &service,
            "reminders.due",
            json!({"now":"2026-10-30T09:00:00Z"})
        )[0]["reminder_at"],
        "2026-10-30T08:00:00.000Z"
    );
}

#[test]
fn done_completes_once_and_retains_recurring_task_semantics() {
    let (_dir, service) = setup();
    let original = call(
        &service,
        "tasks.create",
        json!({"title":"Repeat","due_date":"2026-10-29","recurrence":{"frequency":"daily"},"reminders":["2026-10-29T08:00:00Z"]}),
    );
    let pending = capture(&service);
    let done = action(&pending[0], "done");
    assert_eq!(
        call(&service, "reminders.resolve", done.clone())["ok"],
        true
    );
    assert_eq!(call(&service, "reminders.resolve", done)["stale"], true);
    let completed = call(&service, "tasks.get", json!({"id":original["id"]}));
    assert!(completed["completed_at"].is_string());
    assert!(completed["recurrence_next_id"].is_string());
    let next = call(
        &service,
        "tasks.get",
        json!({"id":completed["recurrence_next_id"]}),
    );
    assert_eq!(next["due_date"], "2026-10-30");
    assert_eq!(
        call(&service, "snapshot", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(call(&service, "reminders.pending", json!({})), json!([]));
}

#[test]
fn stale_controls_cannot_modify_rescheduled_completed_deleted_or_archived_tasks() {
    for mutation in ["reschedule", "complete", "delete", "archive", "wont_do"] {
        let (_dir, service) = setup();
        let list = call(&service, "lists.create", json!({"name":"Reminders"}));
        let original = call(
            &service,
            "tasks.create",
            json!({"title":mutation,"list_id":list["id"],"reminders":["2026-10-29T08:00:00Z"]}),
        );
        let pending = capture(&service);
        match mutation {
            "reschedule" => {
                call(
                    &service,
                    "tasks.update",
                    json!({"id":original["id"],"reminders":[]}),
                );
                // Same timestamp, new generation: a stale popup must remain invalid.
                call(
                    &service,
                    "tasks.update",
                    json!({"id":original["id"],"reminders":["2026-10-29T08:00:00Z"]}),
                );
            }
            "archive" => {
                call(&service, "lists.archive", json!({"id":list["id"]}));
            }
            _ => {
                call(
                    &service,
                    &format!("tasks.{mutation}"),
                    json!({"id":original["id"]}),
                );
            }
        }
        assert_eq!(
            call(&service, "reminders.pending", json!({})),
            json!([]),
            "{mutation}"
        );
        let before = call(&service, "tasks.get", json!({"id":original["id"]}));
        assert_eq!(
            call(&service, "reminders.resolve", action(&pending[0], "done"))["stale"],
            true,
            "{mutation}"
        );
        assert_eq!(
            call(&service, "tasks.get", json!({"id":original["id"]})),
            before,
            "{mutation}"
        );
    }
}

#[test]
fn snoozing_to_an_already_delivered_time_rearms_that_alarm() {
    let (_dir, service) = setup();
    call(
        &service,
        "tasks.create",
        json!({"title":"Two alarms","reminders":["2026-10-29T08:00:00Z","2026-10-29T08:10:00Z"]}),
    );
    let pending = capture(&service);
    assert_eq!(pending.as_array().unwrap().len(), 2);
    let mut snooze = action(&pending[0], "snooze");
    snooze["minutes"] = json!(10);
    snooze["now"] = json!("2026-10-29T08:00:00Z");
    call(&service, "reminders.resolve", snooze);
    let due = call(
        &service,
        "reminders.due",
        json!({"now":"2026-10-29T08:10:00Z"}),
    );
    assert_eq!(due.as_array().unwrap().len(), 1);
    assert_ne!(
        due[0]["reminder_generation"],
        pending[1]["reminder_generation"]
    );
    assert_eq!(
        call(
            &service,
            "reminders.resolve",
            action(&pending[1], "dismiss")
        )["stale"],
        true
    );
}

#[test]
fn invalid_snooze_does_not_consume_a_pending_alarm() {
    let (_dir, service) = setup();
    task(&service);
    let pending = capture(&service);
    for minutes in [json!(0), json!(-1), json!(10081), json!(1.5), json!("5")] {
        let mut input = action(&pending[0], "snooze");
        input["minutes"] = minutes;
        assert!(service.execute("reminders.resolve", input).is_err());
        assert_eq!(call(&service, "reminders.pending", json!({})), pending);
    }
}

#[test]
fn old_reminder_controls_stay_invalid_after_reopening_or_restoring_the_task() {
    for mutation in ["complete", "delete", "archive", "wont_do"] {
        let (_dir, service) = setup();
        let list = call(&service, "lists.create", json!({"name":"Reminders"}));
        let original = call(
            &service,
            "tasks.create",
            json!({"title":mutation,"list_id":list["id"],"reminders":["2026-10-29T08:00:00Z"]}),
        );
        let pending = capture(&service);
        match mutation {
            "complete" => {
                call(&service, "tasks.complete", json!({"id":original["id"]}));
                call(
                    &service,
                    "tasks.complete",
                    json!({"id":original["id"],"completed":false}),
                );
            }
            "delete" => {
                call(&service, "tasks.delete", json!({"id":original["id"]}));
                call(&service, "tasks.restore", json!({"id":original["id"]}));
            }
            "archive" => {
                call(&service, "lists.archive", json!({"id":list["id"]}));
                call(
                    &service,
                    "lists.archive",
                    json!({"id":list["id"],"archived":false}),
                );
            }
            _ => {
                call(&service, "tasks.wont_do", json!({"id":original["id"]}));
                call(
                    &service,
                    "tasks.wont_do",
                    json!({"id":original["id"],"wont_do":false}),
                );
            }
        }
        assert_eq!(
            call(&service, "reminders.pending", json!({})),
            json!([]),
            "{mutation}"
        );
        assert_eq!(
            call(&service, "reminders.resolve", action(&pending[0], "done"))["stale"],
            true,
            "{mutation}"
        );
        assert!(
            call(&service, "tasks.get", json!({"id":original["id"]}))["completed_at"].is_null(),
            "{mutation}"
        );
    }
}
