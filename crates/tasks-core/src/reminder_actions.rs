//! Durable, generation-bound reminder actions. Capturing a notification and
//! queuing its controls is one transaction, so process restarts never lose it.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE pending_reminders (
        task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        at TEXT NOT NULL,
        generation TEXT NOT NULL,
        captured_at TEXT NOT NULL,
        PRIMARY KEY(task_id,at,generation)
    );
    CREATE TRIGGER pending_reminders_task_lifecycle
    AFTER UPDATE OF completed_at,deleted_at,details_json,list_id ON tasks
    WHEN NEW.completed_at IS NOT NULL OR NEW.deleted_at IS NOT NULL
      OR json_extract(NEW.details_json,'$.wont_do_at') IS NOT NULL
      OR (SELECT archived_at FROM lists WHERE id=NEW.list_id) IS NOT NULL
    BEGIN
      DELETE FROM pending_reminders WHERE task_id=NEW.id;
    END;
    CREATE TRIGGER pending_reminders_list_archived
    AFTER UPDATE OF archived_at ON lists WHEN NEW.archived_at IS NOT NULL
    BEGIN
      DELETE FROM pending_reminders WHERE task_id IN (SELECT id FROM tasks WHERE list_id=NEW.id);
    END;",
    )?;
    Ok(())
}

pub(super) fn read(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    debug_assert_eq!(action, "reminders.pending");
    fields(data, &[])?;
    pending(conn)
}

fn pending(conn: &Connection) -> Result<Value> {
    let mut statement = conn.prepare(
        "SELECT p.task_id,p.at,p.generation FROM pending_reminders p
        JOIN reminders r ON r.task_id=p.task_id AND r.at=p.at AND r.generation=p.generation
        JOIN tasks t ON t.id=p.task_id JOIN lists l ON l.id=t.list_id
        WHERE t.deleted_at IS NULL AND t.completed_at IS NULL
        AND json_extract(t.details_json,'$.wont_do_at') IS NULL AND l.archived_at IS NULL
        ORDER BY p.at,p.task_id",
    )?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut result = Vec::new();
    for (id, at, generation) in rows {
        let mut task = json!(get_task(conn, &id)?);
        task["reminder_at"] = json!(at);
        task["reminder_generation"] = json!(generation);
        result.push(task);
    }
    Ok(json!(result))
}

pub(super) fn mutate(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    match action {
        "reminders.capture" => capture(conn, data),
        "reminders.resolve" => resolve(conn, data),
        _ => invalid("Unknown reminder action"),
    }
}

fn capture(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["now"])?;
    // Discard orphaned or superseded controls without changing live reminders.
    conn.execute(
        "DELETE FROM pending_reminders WHERE NOT EXISTS (
        SELECT 1 FROM reminders r JOIN tasks t ON t.id=r.task_id JOIN lists l ON l.id=t.list_id
        WHERE r.task_id=pending_reminders.task_id AND r.at=pending_reminders.at
        AND r.generation=pending_reminders.generation AND t.deleted_at IS NULL
        AND t.completed_at IS NULL AND json_extract(t.details_json,'$.wont_do_at') IS NULL
        AND l.archived_at IS NULL)",
        [],
    )?;
    let due = due_reminders(conn, data)?;
    let mut captured = Vec::new();
    for task in due.as_array().into_iter().flatten() {
        let id = task["id"].as_str().unwrap_or_default();
        let at = task["reminder_at"].as_str().unwrap_or_default();
        let generation = task["reminder_generation"].as_str().unwrap_or_default();
        conn.execute("INSERT OR IGNORE INTO pending_reminders(task_id,at,generation,captured_at) VALUES(?1,?2,?3,?4)", params![id,at,generation,now()])?;
        mark_reminder(
            conn,
            json!({"id":id,"reminder_at":at,"generation":generation})
                .as_object()
                .unwrap(),
        )?;
        captured.push(task.clone());
    }
    Ok(json!({"captured":captured,"pending":pending(conn)?}))
}

fn resolve(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(
        data,
        &[
            "id",
            "reminder_at",
            "generation",
            "action",
            "minutes",
            "now",
        ],
    )?;
    let id = required(data, "id")?;
    let at = normalized_instant(required(data, "reminder_at")?)?;
    let generation = required(data, "generation")?;
    let action = required(data, "action")?;
    if !matches!(action, "done" | "dismiss" | "snooze") {
        return invalid("Choose Done, Dismiss, or Snooze.");
    }
    let snooze = if action == "snooze" {
        let minutes = data
            .get("minutes")
            .and_then(Value::as_i64)
            .filter(|v| (1..=10080).contains(v))
            .ok_or_else(|| {
                ServiceError::Invalid("Snooze must be between 1 minute and 7 days.".into())
            })?;
        let time = data
            .get("now")
            .map(|_| required(data, "now").and_then(normalized_instant))
            .transpose()?
            .unwrap_or_else(now);
        let parsed = chrono::DateTime::parse_from_rfc3339(&time)
            .map_err(|_| ServiceError::Invalid("Invalid current time.".into()))?;
        Some(
            parsed
                .checked_add_signed(chrono::Duration::minutes(minutes))
                .ok_or_else(|| ServiceError::Invalid("Snooze time is out of range.".into()))?
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        )
    } else {
        if data.contains_key("minutes") || data.contains_key("now") {
            return invalid("A snooze duration is only valid for Snooze.");
        }
        None
    };
    let current = pending(conn)?;
    let valid =
        current.as_array().into_iter().flatten().any(|t| {
            t["id"] == id && t["reminder_at"] == at && t["reminder_generation"] == generation
        });
    if !valid {
        return Ok(json!({"ok":false,"stale":true}));
    }
    if action == "done" {
        extensions::complete(conn, json!({"id":id,"completed":true}).as_object().unwrap())?;
    } else if let Some(next) = snooze {
        let task = get_task(conn, id)?;
        let mut reminders: Vec<String> = task
            .reminders
            .iter()
            .filter(|value| **value != at)
            .cloned()
            .collect();
        reminders.push(next.clone());
        reminders.sort();
        reminders.dedup();
        update_task(
            conn,
            json!({"id":id,"reminders":reminders,"expected_version":task.version})
                .as_object()
                .unwrap(),
        )?;
        // A newly snoozed time may coincide with an older acknowledged alarm.
        // Issue a fresh generation so its old notification cannot suppress it.
        conn.execute(
            "UPDATE reminders SET notified_at=NULL,generation=?3 WHERE task_id=?1 AND at=?2",
            params![id, next, Uuid::new_v4().to_string()],
        )?;
    }
    conn.execute(
        "DELETE FROM pending_reminders WHERE task_id=?1 AND at=?2 AND generation=?3",
        params![id, at, generation],
    )?;
    Ok(json!({"ok":true,"stale":false}))
}
