use serde_json::{json, Value};
use std::sync::{Arc, Barrier};
use tasks_core::TaskService;

fn call(service: &TaskService, action: &str, input: Value) -> Value {
    service.execute(action, input).unwrap()
}
fn next(service: &TaskService, task: &Value) -> Value {
    let completed = call(service, "tasks.complete", json!({"id":task["id"]}));
    call(
        service,
        "tasks.get",
        json!({"id":completed["recurrence_next_id"]}),
    )
}

#[test]
fn custom_monthly_completion_is_atomic_across_writers_and_preserves_dst_offsets_and_count() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let initial = call(
        &service,
        "tasks.create",
        json!({
            "title":"Calendar checks", "due_date":"2026-03-01", "due_time":"09:00",
            "start_date":"2026-02-28", "timezone":"Europe/Zurich",
            "reminders":["2026-03-01T07:45:00Z"],
            "recurrence":{"frequency":"monthly","month_days":[1,15,31],"count":3}
        }),
    );
    let barrier = Arc::new(Barrier::new(6));
    let workers = (0..6)
        .map(|_| {
            let service = service.clone();
            let barrier = barrier.clone();
            let id = initial["id"].clone();
            std::thread::spawn(move || {
                barrier.wait();
                call(&service, "tasks.complete", json!({"id":id}))["recurrence_next_id"].clone()
            })
        })
        .collect::<Vec<_>>();
    let ids = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(ids.iter().all(|id| id == &ids[0]));
    let second = call(&service, "tasks.get", json!({"id":ids[0]}));
    assert_eq!(second["due_date"], "2026-03-15");
    assert_eq!(second["start_date"], "2026-03-14");
    assert_eq!(second["reminders"], json!(["2026-03-15T07:45:00.000Z"]));
    assert_eq!(second["recurrence"]["count"], 2);
    let third = next(&service, &second);
    assert_eq!(third["due_date"], "2026-03-31");
    assert_eq!(third["reminders"], json!(["2026-03-31T06:45:00.000Z"]));
    assert_eq!(third["recurrence"]["month_days"], json!([1, 15, 31]));
    assert_eq!(third["recurrence"]["count"], 1);
    assert!(
        call(&service, "tasks.complete", json!({"id":third["id"]}))["recurrence_next_id"].is_null()
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
fn custom_patterns_persist_through_templates_and_invalid_updates_leave_task_and_revision_intact() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let initial = call(
        &service,
        "tasks.create",
        json!({"title":"Last weekday","due_date":"2026-01-30","recurrence":{"frequency":"monthly","workday":"last"}}),
    );
    let template = call(
        &service,
        "templates.create",
        json!({"name":"Monthly review","task_id":initial["id"]}),
    );
    let copy = call(&service, "templates.apply", json!({"id":template["id"]}));
    assert_eq!(copy["recurrence"], initial["recurrence"]);
    assert_eq!(next(&service, &copy)["due_date"], "2026-02-27");
    let before = call(&service, "revision", json!({}));
    for recurrence in [
        json!({"frequency":"monthly","month_days":[1],"workday":"first"}),
        json!({"frequency":"yearly","year_month":2,"month_days":[30]}),
        json!({"frequency":"monthly","ordinal_weekday":{"ordinal":6,"weekday":1}}),
    ] {
        assert!(service
            .execute(
                "tasks.update",
                json!({"id":initial["id"],"title":"Must not save","recurrence":recurrence})
            )
            .is_err());
    }
    assert_eq!(call(&service, "revision", json!({})), before);
    assert_eq!(
        call(&service, "tasks.get", json!({"id":initial["id"]})),
        initial
    );
    let reopened = TaskService::open(service.path()).unwrap();
    assert_eq!(
        call(&reopened, "tasks.get", json!({"id":initial["id"]}))["recurrence"],
        initial["recurrence"]
    );
}

#[test]
fn annual_leap_date_and_fifth_weekday_skip_missing_occurrences_without_consuming_count() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let leap = call(
        &service,
        "tasks.create",
        json!({"title":"Leap day","due_date":"2024-02-29","recurrence":{"frequency":"yearly","year_month":2,"month_days":[29],"count":3,"end_date":"2030-12-31"}}),
    );
    let leap_next = next(&service, &leap);
    assert_eq!(leap_next["due_date"], "2028-02-29");
    assert_eq!(leap_next["recurrence"]["count"], 2);
    assert!(
        call(&service, "tasks.complete", json!({"id":leap_next["id"]}))["recurrence_next_id"]
            .is_null()
    );
    let ordinal = call(
        &service,
        "tasks.create",
        json!({"title":"Fifth Monday","due_date":"2026-03-30","recurrence":{"frequency":"monthly","ordinal_weekday":{"ordinal":5,"weekday":1},"count":4}}),
    );
    let ordinal_next = next(&service, &ordinal);
    assert_eq!(ordinal_next["due_date"], "2026-06-29");
    assert_eq!(ordinal_next["recurrence"]["count"], 3);
}

#[test]
fn sterile_patterns_reject_new_configuration_atomically_and_legacy_tasks_remain_editable_and_completable(
) {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    for (due, rule) in [
        (
            "2025-04-30",
            json!({"frequency":"yearly","month_days":[31]}),
        ),
        (
            "2025-04-30",
            json!({"frequency":"monthly","month_days":[31],"interval":12}),
        ),
        (
            "2025-02-28",
            json!({"frequency":"yearly","year_month":2,"month_days":[29],"interval":4}),
        ),
    ] {
        let before = call(&service, "revision", json!({}));
        assert!(service
            .execute(
                "tasks.create",
                json!({"title":"Impossible","due_date":due,"recurrence":rule})
            )
            .is_err());
        assert_eq!(call(&service, "revision", json!({})), before);
        let original = call(
            &service,
            "tasks.create",
            json!({"title":"Legacy saved schedule","due_date":due}),
        );
        let before = call(&service, "revision", json!({}));
        assert!(service
            .execute(
                "tasks.update",
                json!({"id":original["id"],"title":"Should roll back","recurrence":rule})
            )
            .is_err());
        assert_eq!(call(&service, "revision", json!({})), before);
        assert_eq!(
            call(&service, "tasks.get", json!({"id":original["id"]})),
            original
        );
        // Simulate a rule saved by the previous release before contextual checks.
        let conn = rusqlite::Connection::open(service.path()).unwrap();
        let text: String = conn
            .query_row(
                "SELECT details_json FROM tasks WHERE id=?1",
                [original["id"].as_str().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        let mut details: Value = serde_json::from_str(&text).unwrap();
        details["recurrence"] = rule;
        conn.execute(
            "UPDATE tasks SET details_json=?2 WHERE id=?1",
            rusqlite::params![original["id"].as_str().unwrap(), details.to_string()],
        )
        .unwrap();
        let edited = call(
            &service,
            "tasks.update",
            json!({"id":original["id"],"notes":"Still editable"}),
        );
        assert_eq!(edited["notes"], "Still editable");
        let completed = call(&service, "tasks.complete", json!({"id":original["id"]}));
        assert!(completed["completed_at"].is_string());
        assert!(completed["recurrence_next_id"].is_null());
    }
    let valid = call(
        &service,
        "tasks.create",
        json!({"title":"Valid but exhausted","due_date":"2024-02-29","recurrence":{"frequency":"yearly","year_month":2,"month_days":[29],"interval":4,"count":1,"end_date":"2024-02-29"}}),
    );
    assert!(
        call(&service, "tasks.complete", json!({"id":valid["id"]}))["completed_at"].is_string()
    );
}
