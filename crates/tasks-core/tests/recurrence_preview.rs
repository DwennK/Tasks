use serde_json::{json, Value};
use tasks_core::TaskService;

#[test]
fn standalone_preview_needs_no_database_or_service_instance() {
    let result = tasks_core::preview_recurrence(json!({"due_date":"2026-10-28","recurrence":{"frequency":"daily","count":2},"range_start":"2026-10-28","range_end":"2026-11-01"})).unwrap();
    assert_eq!(result["dates"], json!(["2026-10-29"]));
    assert!(tasks_core::preview_recurrence(json!([])).is_err());
}

fn call(service: &TaskService, action: &str, input: Value) -> Value {
    service.execute(action, input).unwrap()
}

#[test]
fn previews_match_real_successors_without_any_saved_state_change() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let fixtures = [
        (
            "2026-03-25",
            "2026-03-25",
            "2026-04-05",
            json!({"frequency":"daily","count":10}),
        ),
        (
            "2026-10-26",
            "2026-10-26",
            "2026-12-20",
            json!({"frequency":"weekly","interval":2,"weekdays":[1,5],"count":7}),
        ),
        (
            "2026-01-01",
            "2026-01-01",
            "2026-06-30",
            json!({"frequency":"monthly","month_days":[1,15,-1],"count":10}),
        ),
        (
            "2026-01-31",
            "2026-01-31",
            "2026-04-30",
            json!({"frequency":"monthly","count":4}),
        ),
        (
            "2026-01-01",
            "2026-01-01",
            "2026-10-31",
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":5,"weekday":1},"count":4}),
        ),
        (
            "2026-01-30",
            "2026-01-30",
            "2026-05-30",
            json!({"frequency":"monthly","workday":"last","count":4}),
        ),
        (
            "2026-01-30",
            "2026-01-30",
            "2026-03-31",
            json!({"frequency":"monthly","month_days":[-1],"skip_weekends":true,"count":4}),
        ),
        (
            "2024-02-29",
            "2028-01-01",
            "2028-12-31",
            json!({"frequency":"yearly","year_month":2,"month_days":[29],"count":3}),
        ),
        (
            "2026-11-26",
            "2027-09-01",
            "2027-12-31",
            json!({"frequency":"yearly","year_month":11,"ordinal_weekday":{"ordinal":4,"weekday":4},"end_date":"2027-11-25"}),
        ),
    ];
    for (due, start, end, rule) in fixtures {
        let task = call(
            &service,
            "tasks.create",
            json!({"title":"Preview comparison","due_date":due,"due_time":"09:00","timezone":"Europe/Zurich","recurrence":rule}),
        );
        let before = call(&service, "snapshot", json!({}));
        // Pass the original draft rule without anchor_date, as the GUI does.
        let preview = call(
            &service,
            "recurrence.preview",
            json!({"due_date":due,"recurrence":rule,"timezone":"Europe/Zurich","range_start":start,"range_end":end}),
        );
        assert_eq!(preview["truncated"], false, "{rule}");
        assert_eq!(preview["assumption"], "due_date");
        assert_eq!(call(&service, "snapshot", json!({})), before);
        let mut actual = Vec::new();
        let mut current = task;
        for _ in 0..128 {
            let done = call(&service, "tasks.complete", json!({"id":current["id"]}));
            if done["recurrence_next_id"].is_null() {
                break;
            }
            current = call(
                &service,
                "tasks.get",
                json!({"id":done["recurrence_next_id"]}),
            );
            let next = current["due_date"].as_str().unwrap();
            if next > end {
                break;
            }
            if next >= start {
                actual.push(next.to_owned());
            }
        }
        assert_eq!(preview["dates"], json!(actual), "{due} {rule}");
    }
}

#[test]
fn preview_limits_end_count_and_completion_assumptions_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let service = TaskService::open(dir.path().join("tasks.sqlite3")).unwrap();
    let before = call(&service, "snapshot", json!({}));
    let request = json!({"due_date":"2026-10-28","recurrence":{"frequency":"daily"},"range_start":"2026-10-28","range_end":"2026-11-10","limit":3});
    let preview = call(&service, "recurrence.preview", request.clone());
    assert_eq!(
        preview["dates"],
        json!(["2026-10-29", "2026-10-30", "2026-10-31"])
    );
    assert_eq!(preview["truncated"], true);
    let mut counted = request.clone();
    counted["recurrence"]["count"] = json!(4);
    assert_eq!(
        call(&service, "recurrence.preview", counted)["truncated"],
        false
    );
    let mut ended = request.clone();
    ended["recurrence"]["end_date"] = json!("2026-10-30");
    assert_eq!(
        call(&service, "recurrence.preview", ended)["dates"],
        json!(["2026-10-29", "2026-10-30"])
    );
    let mut complete = request.clone();
    complete["recurrence"]["mode"] = json!("completion_date");
    assert_eq!(
        call(&service, "recurrence.preview", complete)["assumption"],
        "completion_on_due_date"
    );
    let mut inactive = request.clone();
    inactive["recurrence"] = Value::Null;
    assert_eq!(
        call(&service, "recurrence.preview", inactive)["dates"],
        json!([])
    );
    let old = json!({"due_date":"1900-01-01","recurrence":{"frequency":"daily"},"range_start":"2026-10-01","range_end":"2026-11-11"});
    assert_eq!(call(&service, "recurrence.preview", old)["truncated"], true);
    for patch in [
        json!({"range_end":"2027-12-31"}),
        json!({"range_end":"2026-01-01"}),
        json!({"limit":0}),
        json!({"limit":513}),
        json!({"timezone":"not-a-zone"}),
        json!({"due_date":"2026-02-30"}),
        json!({"extra":true}),
    ] {
        let mut invalid = request.clone();
        invalid
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(service.execute("recurrence.preview", invalid).is_err());
    }
    assert_eq!(call(&service, "snapshot", json!({})), before);
}
