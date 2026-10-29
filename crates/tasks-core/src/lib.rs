//! The only business-logic layer used by Tasks' desktop, CLI, and MCP transports.
//! Each operation opens a short-lived SQLite connection. WAL permits readers while
//! writes are serialized by SQLite; all multi-record mutations use IMMEDIATE transactions.
use chrono::{DateTime, Days, Local, NaiveDate, NaiveTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
mod desktop_settings;
pub use desktop_settings::validate_desktop_integration_setting;
mod extensions;
mod history;
mod recurrence;
mod relations;
mod reminder_actions;
mod sync;
mod text;
use serde_json::{json, Map, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;

/// Preview a draft schedule without opening or creating a database. Transports
/// can call this before constructing a TaskService for pure calendar requests.
pub fn preview_recurrence(input: Value) -> Result<Value> {
    let data = input
        .as_object()
        .ok_or_else(|| ServiceError::Invalid("Input must be a JSON object.".into()))?;
    recurrence::preview(data)
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("This task was changed elsewhere. Reload it before saving.")]
    Conflict,
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub notes: String,
    /// Visible note content shared by search and presentation; never persisted separately.
    pub notes_text: String,
    pub list_id: String,
    pub parent_id: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub reminder_at: Option<String>,
    pub priority: i64,
    pub position: i64,
    pub completed_at: Option<String>,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    #[serde(flatten)]
    pub details: extensions::TaskDetails,
    pub reminders: Vec<String>,
    pub tag_ids: Vec<String>,
    pub attachments: Vec<Value>,
    pub linked_task_ids: Vec<String>,
    pub comments: Vec<Value>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub pinned: bool,
    pub parent_id: Option<String>,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct TaskList {
    pub id: String,
    pub name: String,
    pub folder_id: Option<String>,
    pub color: String,
    pub icon: String,
    pub pinned: bool,
    pub show_in_smart_lists: bool,
    pub list_type: String,
    pub archived_at: Option<String>,
    #[serde(flatten)]
    pub preferences: extensions::ListPreferences,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Override with TASKS_DB to isolate tests, integrations, or alternate local stores.
pub fn default_db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("TASKS_DB").filter(|p| !p.is_empty()) {
        return path.into();
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or(home.join("AppData/Roaming"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or(home.join(".local/share"));
    base.join("com.dwenn.tasks/tasks.sqlite3")
}

#[derive(Debug, Clone)]
pub struct TaskService {
    path: Arc<PathBuf>,
}
impl TaskService {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.as_os_str().is_empty() || path.to_string_lossy() == ":memory:" {
            return invalid("Use a file path for the shared database.");
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let service = Self {
            path: Arc::new(path),
        };
        // On a brand-new file, journal-mode negotiation can return SQLITE_BUSY
        // immediately without invoking SQLite's busy handler. Retry only this
        // idempotent startup path, never a business operation that may have saved.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut delay = Duration::from_millis(10);
        loop {
            match service.initialize() {
                Ok(()) => return Ok(service),
                Err(error) if startup_contention(&error) && Instant::now() < deadline => {
                    std::thread::sleep(
                        delay.min(deadline.saturating_duration_since(Instant::now())),
                    );
                    delay = (delay * 2).min(Duration::from_millis(200));
                }
                Err(error) => return Err(error),
            }
        }
    }
    fn initialize(&self) -> Result<()> {
        let mut conn = self.connect_with_timeout(Duration::from_millis(250))?;
        let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            conn.pragma_update(None, "journal_mode", "WAL")?;
        }
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        // Existing databases do not need a write lock merely to open the app.
        if version == 9 {
            return Ok(());
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Another process may have finished the migration while we waited.
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 9 {
            return invalid(
                "This database was created by a newer version of Tasks. Update the application.",
            );
        }
        if version == 0 {
            tx.execute_batch(include_str!("../migrations/001_initial.sql"))?;
            let now = now();
            tx.execute("INSERT INTO lists(id,name,folder_id,color,position,created_at,updated_at) VALUES('inbox','Inbox',NULL,'#4772fa',0,?1,?1)", [&now])?;
            tx.execute("INSERT INTO settings(key,value) VALUES('theme','\"light\"'),('notifications','false')", [])?;
            tx.pragma_update(None, "user_version", 1)?;
        }
        if version < 2 {
            tx.execute_batch(include_str!("../migrations/002_tasks_features.sql"))?;
            extensions::initialize_settings(&tx)?;
            tx.pragma_update(None, "user_version", 2)?;
        }
        if version < 3 {
            tx.execute_batch(include_str!("../migrations/003_comment_attachments.sql"))?;
            tx.pragma_update(None, "user_version", 3)?;
        }
        if version < 4 {
            tx.execute_batch(include_str!("../migrations/004_template_previews.sql"))?;
            extensions::migrate_template_previews(&tx)?;
            tx.pragma_update(None, "user_version", 4)?;
        }
        if version < 5 {
            tx.execute_batch(include_str!("../migrations/005_concurrency_tokens.sql"))?;
            tx.pragma_update(None, "user_version", 5)?;
        }
        if version < 6 {
            tx.execute_batch(include_str!("../migrations/006_list_metadata.sql"))?;
            tx.pragma_update(None, "user_version", 6)?;
        }
        if version < 7 {
            tx.execute_batch(include_str!("../migrations/007_task_activities.sql"))?;
            tx.pragma_update(None, "user_version", 7)?;
        }
        if version < 8 {
            reminder_actions::initialize(&tx)?;
            tx.pragma_update(None, "user_version", 8)?;
        }
        if version < 9 {
            sync::initialize(&tx)?;
            tx.pragma_update(None, "user_version", 9)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn connect(&self) -> Result<Connection> {
        self.connect_with_timeout(Duration::from_secs(10))
    }
    fn connect_with_timeout(&self, timeout: Duration) -> Result<Connection> {
        let conn = Connection::open(&*self.path)?;
        conn.busy_timeout(timeout)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        // Tasks are small writes: prefer durability over the marginal write speed
        // of NORMAL, including when the OS loses power immediately after a save.
        conn.pragma_update(None, "synchronous", "FULL")?;
        Ok(conn)
    }
    pub fn execute(&self, action: &str, input: Value) -> Result<Value> {
        let data = input
            .as_object()
            .ok_or_else(|| ServiceError::Invalid("Input must be a JSON object.".into()))?;
        if action == "recurrence.preview" {
            // Draft calendar previews are pure calculations: no connection,
            // transaction, revision change, or saved task is required.
            return recurrence::preview(data);
        }
        let mut conn = self.connect()?;
        if matches!(
            action,
            "snapshot"
                | "settings.get"
                | "revision"
                | "tasks.list"
                | "tasks.get"
                | "tasks.activities"
                | "reminders.due"
                | "reminders.pending"
                | "tags.list"
                | "attachments.get"
                | "attachments.read_chunk"
                | "templates.list"
                | "templates.get"
                | "comments.list"
                | "sync.status"
                | "sync.conflicts"
                | "sync.checkpoint"
        ) {
            let tx = conn.transaction()?;
            let result = match action {
                "snapshot" => {
                    fields(data, &[])?;
                    snapshot(&tx)?
                }
                "settings.get" => {
                    fields(data, &[])?;
                    json!(read_settings(&tx)?)
                }
                "revision" => {
                    fields(data, &[])?;
                    json!({"revision": revision(&tx)?})
                }
                "tasks.list" => list_tasks(&tx, data)?,
                "tasks.get" => {
                    fields(data, &["id"])?;
                    json!(get_task(&tx, required(data, "id")?)?)
                }
                "tasks.activities" => history::read(&tx, data)?,
                "sync.status" | "sync.conflicts" | "sync.checkpoint" => {
                    sync::read(&tx, action, data)?
                }
                "reminders.due" => due_reminders(&tx, data)?,
                "reminders.pending" => reminder_actions::read(&tx, action, data)?,
                "tags.list" | "attachments.get" | "attachments.read_chunk" => {
                    relations::read(&tx, action, data)?
                }
                "templates.list" | "templates.get" | "comments.list" => {
                    extensions::read(&tx, action, data)?
                }
                _ => unreachable!(),
            };
            tx.commit()?;
            return Ok(result);
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = if history::records_action(action) {
            Some(history::capture(&tx)?)
        } else {
            None
        };
        let result =
            match action {
                "tasks.create" => create_task(&tx, data)?,
                "tasks.update" => update_task(&tx, data)?,
                "tasks.complete" => extensions::complete(&tx, data)?,
                "tasks.delete" => delete_task(&tx, data)?,
                "tasks.restore" => restore_task(&tx, data)?,
                "tasks.restore_content" => history::restore_content(&tx, data)?,
                "tasks.reorder" => reorder(&tx, "tasks", data)?,
                "folders.create" => create_folder(&tx, data)?,
                "folders.update" => update_folder(&tx, data)?,
                "folders.delete" => delete_folder(&tx, data)?,
                "folders.reorder" => reorder(&tx, "folders", data)?,
                "lists.create" => create_list(&tx, data)?,
                "lists.update" => update_list(&tx, data)?,
                "lists.delete" => delete_list(&tx, data)?,
                "lists.reorder" => reorder(&tx, "lists", data)?,
                "settings.set" => extensions::set_setting(&tx, data)?,
                "sync.export" | "sync.merge" | "sync.resolve" | "sync.checkpoint.set" => {
                    sync::mutate(&tx, action, data)?
                }
                "reminders.mark" => mark_reminder(&tx, data)?,
                "reminders.capture" | "reminders.resolve" => {
                    reminder_actions::mutate(&tx, action, data)?
                }
                "tags.create" | "tags.update" | "tags.delete" | "tasks.tags" | "links.add"
                | "links.remove" | "attachments.add" | "attachments.import"
                | "attachments.remove" => relations::mutate(&tx, action, data)?,
                "tasks.duplicate" | "tasks.wont_do" | "lists.archive" | "lists.duplicate"
                | "sections.create" | "sections.update" | "sections.delete"
                | "sections.reorder" | "templates.create" | "templates.update"
                | "templates.delete" | "templates.apply" | "comments.add" | "comments.update"
                | "comments.delete" => extensions::mutate(&tx, action, data)?,
                _ => return invalid(format!("Unknown action: {action}")),
            };
        let result = if matches!(action, "tasks.create" | "tasks.update")
            && data.get("progress").and_then(Value::as_i64) == Some(100)
            && result["progress_mode"] == "manual"
            && result["completed_at"].is_null()
            && result["is_note"] == false
        {
            extensions::complete(&tx, json!({"id":result["id"]}).as_object().expect("object"))?
        } else {
            result
        };
        if action == "tags.delete" {
            extensions::prune_filters(&tx)?;
        }
        if let Some(previous) = previous {
            history::record(&tx, action, previous)?;
        }
        if !action.starts_with("sync.")
            && !matches!(action, "reminders.mark" | "reminders.capture")
            && (action != "settings.set"
                || matches!(
                    data.get("key").and_then(Value::as_str),
                    Some("saved_filters" | "summary_templates")
                ))
        {
            sync::record_local(&tx)?;
        }
        // Export and duplicate downloads are observational for the UI; avoid
        // unnecessary refreshes and remote-upload loops during polling.
        if !matches!(action, "sync.export" | "sync.checkpoint.set")
            && (action != "sync.merge" || result["visible_changes"] == true)
        {
            tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        }
        tx.commit()?;
        Ok(result)
    }
}

fn startup_contention(error: &ServiceError) -> bool {
    matches!(error, ServiceError::Database(rusqlite::Error::SqliteFailure(code, _))
        if matches!(code.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(ServiceError::Invalid(message.into()))
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn fields(data: &Map<String, Value>, allowed: &[&str]) -> Result<()> {
    if let Some(key) = data.keys().find(|k| !allowed.contains(&k.as_str())) {
        return invalid(format!("Unknown field: {key}"));
    }
    Ok(())
}
fn required<'a>(data: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    data.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ServiceError::Invalid(format!("{key} must be a non-empty string.")))
}
fn optional_string(data: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    match data.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        _ => invalid(format!("{key} must be a non-empty string or null.")),
    }
}
fn text_field(data: &Map<String, Value>, key: &str, default: &str) -> Result<String> {
    match data.get(key) {
        None => Ok(default.into()),
        Some(Value::String(s)) => Ok(s.clone()),
        _ => invalid(format!("{key} must be a string.")),
    }
}
fn bool_field(data: &Map<String, Value>, key: &str, default: bool) -> Result<bool> {
    match data.get(key) {
        None => Ok(default),
        Some(Value::Bool(v)) => Ok(*v),
        _ => invalid(format!("{key} must be true or false.")),
    }
}
fn clean_name(value: &str, field: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 500 {
        return invalid(format!(
            "{field} must contain between 1 and 500 characters."
        ));
    }
    if trimmed.chars().any(|c| c == '\0' || c == '\r' || c == '\n') {
        return invalid(format!(
            "{field} cannot contain line breaks or null characters."
        ));
    }
    Ok(trimmed.into())
}
fn priority(data: &Map<String, Value>, fallback: i64) -> Result<i64> {
    match data.get("priority") {
        None => Ok(fallback),
        Some(v) => v.as_i64().filter(|n| (0..=3).contains(n)).ok_or_else(|| {
            ServiceError::Invalid("priority must be an integer from 0 to 3.".into())
        }),
    }
}
fn parse_date(s: &str) -> Result<NaiveDate> {
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        ServiceError::Invalid("due_date/date must use YYYY-MM-DD and be a real date.".into())
    })?;
    if date.format("%Y-%m-%d").to_string() != s {
        return invalid("due_date/date must use YYYY-MM-DD.");
    }
    Ok(date)
}
fn normalized_instant(s: &str) -> Result<String> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| {
            t.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .map_err(|_| {
            ServiceError::Invalid(
                "reminder_at/now must be an RFC3339 timestamp with a timezone.".into(),
            )
        })
}
fn validate_dates(task: &mut Task) -> Result<()> {
    if let Some(d) = &task.due_date {
        parse_date(d)?;
    }
    if let Some(t) = &task.due_time {
        if task.due_date.is_none() {
            return invalid("A due time requires a due date.");
        }
        let parsed = NaiveTime::parse_from_str(t, "%H:%M")
            .map_err(|_| ServiceError::Invalid("due_time must use HH:MM (00:00–23:59).".into()))?;
        if parsed.format("%H:%M").to_string() != *t {
            return invalid("due_time must use HH:MM.");
        }
    }
    if let Some(t) = &task.reminder_at {
        task.reminder_at = Some(normalized_instant(t)?);
    }
    if task.notes.len() > 1_000_000 || task.notes.contains('\0') {
        return invalid("Notes must be under 1 MB and cannot contain null characters.");
    }
    Ok(())
}
fn revision(conn: &Connection) -> Result<i64> {
    Ok(
        conn.query_row("SELECT value FROM metadata WHERE key='revision'", [], |r| {
            r.get(0)
        })?,
    )
}
const TASK_COLUMNS: &str = "id,title,notes,list_id,parent_id,due_date,due_time,reminder_at,priority,position,completed_at,deleted_at,created_at,updated_at,version,details_json";
fn task_row(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        title: r.get(1)?,
        notes: r.get(2)?,
        notes_text: String::new(),
        list_id: r.get(3)?,
        parent_id: r.get(4)?,
        due_date: r.get(5)?,
        due_time: r.get(6)?,
        reminder_at: r.get(7)?,
        priority: r.get(8)?,
        position: r.get(9)?,
        completed_at: r.get(10)?,
        deleted_at: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        version: r.get(14)?,
        details: serde_json::from_str(&r.get::<_, String>(15)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(15, rusqlite::types::Type::Text, Box::new(e))
        })?,
        reminders: vec![],
        tag_ids: vec![],
        attachments: vec![],
        linked_task_ids: vec![],
        comments: vec![],
    })
}
fn get_task(conn: &Connection, id: &str) -> Result<Task> {
    let mut task = conn
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1"),
            [id],
            task_row,
        )
        .optional()?
        .ok_or_else(|| ServiceError::NotFound("Task".into()))?;
    extensions::hydrate_task(conn, &mut task)?;
    Ok(task)
}
fn folder_row(r: &Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: r.get(0)?,
        name: r.get(1)?,
        parent_id: r.get(2)?,
        icon: r.get(6)?,
        pinned: r.get(7)?,
        position: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}
fn get_folder(conn: &Connection, id: &str) -> Result<Folder> {
    conn.query_row(
        "SELECT id,name,parent_id,position,created_at,updated_at,icon,pinned FROM folders WHERE id=?1",
        [id],
        folder_row,
    )
    .optional()?
    .ok_or_else(|| ServiceError::NotFound("Folder".into()))
}
fn list_row(r: &Row) -> rusqlite::Result<TaskList> {
    Ok(TaskList {
        id: r.get(0)?,
        name: r.get(1)?,
        folder_id: r.get(2)?,
        color: r.get(3)?,
        icon: r.get(9)?,
        pinned: r.get(10)?,
        show_in_smart_lists: r.get(11)?,
        list_type: r.get(12)?,
        archived_at: r.get(7)?,
        preferences: serde_json::from_str(&r.get::<_, String>(8)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?,
        position: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}
fn get_list(conn: &Connection, id: &str) -> Result<TaskList> {
    conn.query_row(
        "SELECT id,name,folder_id,color,position,created_at,updated_at,archived_at,preferences_json,icon,pinned,show_in_smart_lists,list_type FROM lists WHERE id=?1",
        [id],
        list_row,
    )
    .optional()?
    .ok_or_else(|| ServiceError::NotFound("List".into()))
}
fn all_tasks(conn: &Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks ORDER BY position,created_at,id"
    ))?;
    let mut items = stmt
        .query_map([], task_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for task in &mut items {
        extensions::hydrate_task(conn, task)?;
    }
    Ok(items)
}
fn read_settings(conn: &Connection) -> Result<Map<String, Value>> {
    let mut stmt = conn.prepare("SELECT key,value FROM settings")?;
    let pairs = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut settings = extensions::default_settings();
    for (k, v) in pairs {
        settings.insert(k, serde_json::from_str(&v)?);
    }
    Ok(settings)
}
fn snapshot(conn: &Connection) -> Result<Value> {
    let tasks = all_tasks(conn)?;
    let mut stmt = conn.prepare("SELECT id,name,parent_id,position,created_at,updated_at,icon,pinned FROM folders ORDER BY pinned DESC,position,created_at,id")?;
    let folders = stmt
        .query_map([], folder_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare("SELECT id,name,folder_id,color,position,created_at,updated_at,archived_at,preferences_json,icon,pinned,show_in_smart_lists,list_type FROM lists ORDER BY pinned DESC,position,created_at,id")?;
    let lists = stmt
        .query_map([], list_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let settings = read_settings(conn)?;
    Ok(
        json!({"tasks":tasks,"folders":folders,"lists":lists,"settings":settings,"revision":revision(conn)?,"tags":relations::tags(conn)?,"sections":extensions::sections(conn)?,"templates":extensions::templates(conn)?}),
    )
}
fn list_tasks(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(
        data,
        &[
            "view",
            "list_id",
            "search",
            "date",
            "parent_id",
            "include_subtasks",
            "tag_id",
            "section_id",
        ],
    )?;
    let view = text_field(data, "view", "all")?;
    if ![
        "inbox",
        "today",
        "tomorrow",
        "upcoming",
        "all",
        "completed",
        "trash",
        "wont_do",
    ]
    .contains(&view.as_str())
    {
        return invalid("Unknown task view.");
    }
    let date = match data.get("date") {
        Some(_) => parse_date(required(data, "date")?)?,
        None => Local::now().date_naive(),
    };
    let end = date
        .checked_add_days(Days::new(6))
        .ok_or_else(|| ServiceError::Invalid("Date is out of range.".into()))?;
    let list = optional_string(data, "list_id")?;
    let parent = optional_string(data, "parent_id")?;
    // Date smart views are reminders of everything due, including an independently
    // scheduled subtask whose parent has no date. Ordinary list views stay rooted.
    let subtasks = bool_field(
        data,
        "include_subtasks",
        matches!(view.as_str(), "today" | "tomorrow" | "upcoming"),
    )?;
    let search = text::normalized(&text_field(data, "search", "")?).to_lowercase();
    let all = all_tasks(conn)?;
    let tag = optional_string(data, "tag_id")?;
    let section = optional_string(data, "section_id")?;
    let mut stmt = conn
        .prepare("SELECT id FROM lists WHERE archived_at IS NOT NULL OR show_in_smart_lists=0")?;
    let archived: HashSet<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let status_roots = !data.contains_key("include_subtasks")
        && matches!(view.as_str(), "completed" | "trash" | "wont_do");
    let grouped_parents: HashSet<String> = if status_roots {
        all.iter()
            .filter(|task| {
                if view == "trash" {
                    task.deleted_at.is_some()
                } else if view == "wont_do" {
                    task.details.wont_do_at.is_some()
                } else {
                    task.completed_at.is_some()
                }
            })
            .map(|task| task.id.clone())
            .collect()
    } else {
        HashSet::new()
    };
    let tasks: Vec<Task> = all
        .into_iter()
        .filter(|t| {
            if list.is_none()
                && parent.is_none()
                && search.is_empty()
                && !matches!(view.as_str(), "completed" | "trash" | "wont_do")
                && archived.contains(&t.list_id)
            {
                return false;
            }
            if tag.as_ref().is_some_and(|tag| !t.tag_ids.contains(tag)) {
                return false;
            }
            if data.contains_key("section_id") && t.details.section_id != section {
                return false;
            }
            if data.contains_key("parent_id") {
                if t.parent_id != parent {
                    return false;
                }
            } else if status_roots {
                // A separately completed/trashed child is a visible root of this
                // view. If its parent shares the status, show that subtree once.
                if t.parent_id
                    .as_ref()
                    .is_some_and(|id| grouped_parents.contains(id))
                {
                    return false;
                }
            } else if !subtasks && t.parent_id.is_some() {
                return false;
            }
            if let Some(ref list) = list {
                if t.list_id != *list {
                    return false;
                }
            }
            if !search.is_empty()
                && !text::normalized(&t.title).to_lowercase().contains(&search)
                && !text::normalized(&t.notes_text)
                    .to_lowercase()
                    .contains(&search)
            {
                return false;
            }
            match view.as_str() {
                "trash" => t.deleted_at.is_some(),
                "completed" => t.deleted_at.is_none() && t.completed_at.is_some(),
                "wont_do" => t.deleted_at.is_none() && t.details.wont_do_at.is_some(),
                _ if t.deleted_at.is_some()
                    || t.completed_at.is_some()
                    || t.details.wont_do_at.is_some() =>
                {
                    false
                }
                "inbox" => t.list_id == "inbox",
                "today" => t.due_date.as_ref().is_some_and(|d| d <= &date.to_string()),
                "tomorrow" => t
                    .due_date
                    .as_ref()
                    .is_some_and(|d| d == &(date + chrono::Duration::days(1)).to_string()),
                "upcoming" => t
                    .due_date
                    .as_ref()
                    .is_some_and(|d| d >= &date.to_string() && d <= &end.to_string()),
                _ => true,
            }
        })
        .collect();
    Ok(json!(tasks))
}
fn next_position(
    conn: &Connection,
    table: &str,
    parent_column: &str,
    parent: Option<&str>,
    list: Option<&str>,
) -> Result<i64> {
    let sql = if list.is_some() {
        format!("SELECT COALESCE(MAX(position),-1)+1 FROM {table} WHERE {parent_column} IS ?1 AND list_id=?2")
    } else {
        format!("SELECT COALESCE(MAX(position),-1)+1 FROM {table} WHERE {parent_column} IS ?1")
    };
    Ok(if let Some(list) = list {
        conn.query_row(&sql, params![parent, list], |r| r.get(0))?
    } else {
        conn.query_row(&sql, [parent], |r| r.get(0))?
    })
}
fn subtree(conn: &Connection, table: &str, id: &str) -> Result<Vec<String>> {
    let sql = format!("WITH RECURSIVE tree(id) AS (SELECT id FROM {table} WHERE id=?1 UNION ALL SELECT t.id FROM {table} t JOIN tree ON t.parent_id=tree.id) SELECT id FROM tree");
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}
fn ancestors(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("WITH RECURSIVE tree(id,parent_id) AS (SELECT id,parent_id FROM tasks WHERE id=?1 UNION ALL SELECT t.id,t.parent_id FROM tasks t JOIN tree ON t.id=tree.parent_id) SELECT id FROM tree")?;
    let ids = stmt
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}
fn reopen_ancestors(conn: &Connection, id: &str, time: &str) -> Result<()> {
    for id in ancestors(conn, id)? {
        conn.execute("UPDATE tasks SET completed_at=NULL,details_json=json_set(details_json,'$.wont_do_at',NULL,'$.progress',CASE WHEN completed_at IS NOT NULL THEN 0 ELSE COALESCE(json_extract(details_json,'$.progress'),0) END),updated_at=?2,version=version+1 WHERE id=?1 AND (completed_at IS NOT NULL OR json_extract(details_json,'$.wont_do_at') IS NOT NULL)",params![id,time])?;
    }
    Ok(())
}
fn create_task(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    extensions::validate_task_fields(data, false)?;
    let defaults = extensions::creation_defaults(conn, data)?;
    let data = &defaults;
    let time = now();
    let parent_id = optional_string(data, "parent_id")?;
    let list_id = if let Some(ref parent) = parent_id {
        let p = get_task(conn, parent)?;
        if p.deleted_at.is_some() {
            return invalid("Cannot add a subtask to a task in Trash.");
        }
        p.list_id
    } else {
        text_field(data, "list_id", "inbox")?
    };
    let target_list = get_list(conn, &list_id)?;
    let mut task = Task {
        id: Uuid::new_v4().to_string(),
        title: clean_name(required(data, "title")?, "title")?,
        notes: text_field(data, "notes", "")?,
        notes_text: String::new(),
        list_id,
        parent_id,
        due_date: optional_string(data, "due_date")?,
        due_time: optional_string(data, "due_time")?,
        reminder_at: optional_string(data, "reminder_at")?,
        priority: priority(data, 0)?,
        position: 0,
        completed_at: None,
        deleted_at: None,
        created_at: time.clone(),
        updated_at: time.clone(),
        version: 1,
        details: extensions::TaskDetails {
            is_note: target_list.list_type == "note",
            ..Default::default()
        },
        reminders: vec![],
        tag_ids: vec![],
        attachments: vec![],
        linked_task_ids: vec![],
        comments: vec![],
    };
    validate_dates(&mut task)?;
    extensions::prepare_task(conn, &mut task, data)?;
    task.position = next_position(
        conn,
        "tasks",
        "parent_id",
        task.parent_id.as_deref(),
        Some(&task.list_id),
    )?;
    conn.execute("INSERT INTO tasks(id,title,notes,list_id,parent_id,due_date,due_time,reminder_at,priority,position,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",params![task.id,task.title,task.notes,task.list_id,task.parent_id,task.due_date,task.due_time,task.reminder_at,task.priority,task.position,time])?;
    extensions::persist_details(conn, &task, data)?;
    extensions::apply_position_default(conn, &mut task)?;
    if let Some(parent) = &task.parent_id {
        reopen_ancestors(conn, parent, &time)?;
    }
    Ok(json!(get_task(conn, &task.id)?))
}
fn update_task(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    extensions::validate_task_fields(data, true)?;
    let mut task = get_task(conn, required(data, "id")?)?;
    if let Some(v) = data.get("expected_version") {
        let version = v
            .as_i64()
            .ok_or_else(|| ServiceError::Invalid("expected_version must be an integer.".into()))?;
        if version != task.version {
            return Err(ServiceError::Conflict);
        }
    }
    let old = task.clone();
    if data.contains_key("title") {
        task.title = clean_name(required(data, "title")?, "title")?;
    }
    task.notes = text_field(data, "notes", &task.notes)?;
    task.priority = priority(data, task.priority)?;
    if data.contains_key("list_id") {
        task.list_id = required(data, "list_id")?.into();
        get_list(conn, &task.list_id)?;
    }
    if data.contains_key("parent_id") {
        task.parent_id = optional_string(data, "parent_id")?;
    }
    for (key, target) in [
        ("due_date", &mut task.due_date),
        ("due_time", &mut task.due_time),
        ("reminder_at", &mut task.reminder_at),
    ] {
        if data.contains_key(key) {
            *target = optional_string(data, key)?;
        }
    }
    if data.get("due_date") == Some(&Value::Null) && !data.contains_key("due_time") {
        task.due_time = None;
    }
    validate_dates(&mut task)?;
    let descendants = subtree(conn, "tasks", &task.id)?;
    if let Some(parent) = &task.parent_id {
        if descendants.contains(parent) {
            return invalid("A task cannot be its own parent or be moved into its descendants.");
        }
        let parent = get_task(conn, parent)?;
        if parent.deleted_at.is_some() {
            return invalid("Cannot move a task under a task in Trash.");
        }
        if data.contains_key("list_id")
            && task.list_id != parent.list_id
            && task.parent_id == old.parent_id
        {
            task.parent_id = None;
        } else {
            task.list_id = parent.list_id;
        }
    }
    let mut effective = data.clone();
    if old.due_date.is_none()
        && old.reminders.is_empty()
        && task.due_date.is_some()
        && !data.contains_key("reminder_at")
        && !data.contains_key("reminders")
    {
        let timezone = text_field(data, "timezone", &task.details.timezone)?;
        let input = json!({"due_date":task.due_date,"due_time":task.due_time,"timezone":timezone});
        let defaults = extensions::creation_defaults(conn, input.as_object().expect("object"))?;
        if let Some(reminder) = defaults.get("reminder_at") {
            effective.insert("reminder_at".into(), reminder.clone());
            task.reminder_at = reminder.as_str().map(str::to_owned);
        }
    }
    extensions::prepare_task(conn, &mut task, &effective)?;
    let time = now();
    if task.list_id != old.list_id || task.parent_id != old.parent_id {
        task.position = next_position(
            conn,
            "tasks",
            "parent_id",
            task.parent_id.as_deref(),
            Some(&task.list_id),
        )?;
    }
    conn.execute("UPDATE tasks SET title=?2,notes=?3,list_id=?4,parent_id=?5,due_date=?6,due_time=?7,reminder_at=?8,priority=?9,position=?10,updated_at=?11,version=version+1,reminder_notified_at=CASE WHEN reminder_at IS ?8 THEN reminder_notified_at ELSE NULL END WHERE id=?1",params![task.id,task.title,task.notes,task.list_id,task.parent_id,task.due_date,task.due_time,task.reminder_at,task.priority,task.position,time])?;
    extensions::persist_details(conn, &task, data)?;
    if task.list_id != old.list_id {
        for id in descendants.iter().skip(1) {
            conn.execute(
                "UPDATE tasks SET list_id=?2,details_json=json_set(details_json,'$.section_id',NULL),updated_at=?3,version=version+1 WHERE id=?1",
                params![id, task.list_id, time],
            )?;
        }
    }
    if task.completed_at.is_none() && task.deleted_at.is_none() && task.details.wont_do_at.is_none()
    {
        if let Some(parent) = &task.parent_id {
            reopen_ancestors(conn, parent, &time)?;
        }
    }
    Ok(json!(get_task(conn, &task.id)?))
}
fn complete_task(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "completed"])?;
    let task = get_task(conn, required(data, "id")?)?;
    if task.deleted_at.is_some() {
        return invalid("Restore a task from Trash before changing its completion.");
    }
    let complete = bool_field(data, "completed", true)?;
    let time = now();
    if complete {
        for id in subtree(conn, "tasks", &task.id)? {
            conn.execute("UPDATE tasks SET completed_at=?2,updated_at=?2,version=version+1 WHERE id=?1 AND completed_at IS NULL AND deleted_at IS NULL",params![id,time])?;
        }
    } else {
        reopen_ancestors(conn, &task.id, &time)?;
    }
    Ok(json!(get_task(conn, &task.id)?))
}
fn delete_task(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id"])?;
    let task = get_task(conn, required(data, "id")?)?;
    let time = now();
    for id in subtree(conn, "tasks", &task.id)? {
        conn.execute("UPDATE tasks SET deleted_at=?2,updated_at=?2,version=version+1 WHERE id=?1 AND deleted_at IS NULL",params![id,time])?;
    }
    Ok(json!(get_task(conn, &task.id)?))
}
fn restore_task(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id"])?;
    let task = get_task(conn, required(data, "id")?)?;
    let time = now();
    let mut ids = subtree(conn, "tasks", &task.id)?;
    ids.extend(ancestors(conn, &task.id)?);
    for id in &ids {
        conn.execute("UPDATE tasks SET deleted_at=NULL,updated_at=?2,version=version+1 WHERE id=?1 AND deleted_at IS NOT NULL",params![id,time])?;
    }
    // A child might have been trashed before its parent was completed. Restoring
    // the whole subtree must not leave that incomplete child below a done parent.
    for id in &ids {
        let restored = get_task(conn, id)?;
        if restored.completed_at.is_none() && restored.details.wont_do_at.is_none() {
            reopen_ancestors(conn, id, &time)?;
        }
    }
    Ok(json!(get_task(conn, &task.id)?))
}
fn create_folder(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["name", "parent_id", "icon", "pinned"])?;
    let parent = optional_string(data, "parent_id")?;
    if let Some(p) = &parent {
        get_folder(conn, p)?;
    }
    let time = now();
    let folder = Folder {
        id: Uuid::new_v4().to_string(),
        name: clean_name(required(data, "name")?, "name")?,
        icon: extensions::icon(data, "")?,
        pinned: bool_field(data, "pinned", false)?,
        parent_id: parent.clone(),
        position: next_position(conn, "folders", "parent_id", parent.as_deref(), None)?,
        created_at: time.clone(),
        updated_at: time.clone(),
    };
    conn.execute("INSERT INTO folders(id,name,parent_id,position,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)",params![folder.id,folder.name,folder.parent_id,folder.position,time])?;
    conn.execute(
        "UPDATE folders SET icon=?2,pinned=?3 WHERE id=?1",
        params![folder.id, folder.icon, folder.pinned],
    )?;
    Ok(json!(folder))
}
fn update_folder(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "name", "parent_id", "icon", "pinned"])?;
    let mut folder = get_folder(conn, required(data, "id")?)?;
    let old_parent = folder.parent_id.clone();
    folder.icon = extensions::icon(data, &folder.icon)?;
    folder.pinned = bool_field(data, "pinned", folder.pinned)?;
    if data.contains_key("name") {
        folder.name = clean_name(required(data, "name")?, "name")?;
    }
    if data.contains_key("parent_id") {
        folder.parent_id = optional_string(data, "parent_id")?;
    }
    if let Some(p) = &folder.parent_id {
        get_folder(conn, p)?;
        if subtree(conn, "folders", &folder.id)?.contains(p) {
            return invalid("A folder cannot be moved into itself or its descendants.");
        }
    }
    if folder.parent_id != old_parent {
        folder.position = next_position(
            conn,
            "folders",
            "parent_id",
            folder.parent_id.as_deref(),
            None,
        )?;
    }
    let time = now();
    conn.execute(
        "UPDATE folders SET name=?2,parent_id=?3,position=?4,updated_at=?5 WHERE id=?1",
        params![
            folder.id,
            folder.name,
            folder.parent_id,
            folder.position,
            time
        ],
    )?;
    conn.execute(
        "UPDATE folders SET icon=?2,pinned=?3 WHERE id=?1",
        params![folder.id, folder.icon, folder.pinned],
    )?;
    Ok(json!(get_folder(conn, &folder.id)?))
}
fn delete_folder(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id"])?;
    let folder = get_folder(conn, required(data, "id")?)?;
    let time = now();
    let mut stmt =
        conn.prepare("SELECT id FROM folders WHERE parent_id=?1 ORDER BY position,id")?;
    let children = stmt
        .query_map([&folder.id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut position = next_position(
        conn,
        "folders",
        "parent_id",
        folder.parent_id.as_deref(),
        None,
    )?;
    for id in children {
        conn.execute(
            "UPDATE folders SET parent_id=?2,position=?3,updated_at=?4 WHERE id=?1",
            params![id, folder.parent_id, position, time],
        )?;
        position += 1;
    }
    let mut stmt = conn.prepare("SELECT id FROM lists WHERE folder_id=?1 ORDER BY position,id")?;
    let children = stmt
        .query_map([&folder.id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut position = next_position(
        conn,
        "lists",
        "folder_id",
        folder.parent_id.as_deref(),
        None,
    )?;
    for id in children {
        conn.execute(
            "UPDATE lists SET folder_id=?2,position=?3,updated_at=?4 WHERE id=?1",
            params![id, folder.parent_id, position, time],
        )?;
        position += 1;
    }
    conn.execute("DELETE FROM folders WHERE id=?1", [folder.id])?;
    Ok(json!({"ok":true}))
}
fn color(data: &Map<String, Value>, fallback: &str) -> Result<String> {
    let color = text_field(data, "color", fallback)?;
    if color.len() != 7
        || !color.starts_with('#')
        || !color[1..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return invalid("color must be a six-digit hex color, such as #4772fa.");
    }
    Ok(color)
}
fn create_list(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(
        data,
        &[
            "name",
            "folder_id",
            "color",
            "view_mode",
            "group_by",
            "sort_by",
            "sort_order",
            "icon",
            "pinned",
            "show_in_smart_lists",
            "list_type",
        ],
    )?;
    let folder = optional_string(data, "folder_id")?;
    if let Some(p) = &folder {
        get_folder(conn, p)?;
    }
    let time = now();
    let list = TaskList {
        id: Uuid::new_v4().to_string(),
        name: clean_name(required(data, "name")?, "name")?,
        folder_id: folder.clone(),
        color: extensions::list_color(data, "")?,
        icon: extensions::icon(data, "")?,
        pinned: bool_field(data, "pinned", false)?,
        show_in_smart_lists: bool_field(data, "show_in_smart_lists", true)?,
        list_type: extensions::list_type(data, "task")?,
        archived_at: None,
        preferences: extensions::preferences(data, &extensions::ListPreferences::default())?,
        position: next_position(conn, "lists", "folder_id", folder.as_deref(), None)?,
        created_at: time.clone(),
        updated_at: time.clone(),
    };
    conn.execute("INSERT INTO lists(id,name,folder_id,color,position,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)",params![list.id,list.name,list.folder_id,list.color,list.position,time])?;
    conn.execute(
        "UPDATE lists SET preferences_json=?2 WHERE id=?1",
        params![list.id, serde_json::to_string(&list.preferences)?],
    )?;
    conn.execute(
        "UPDATE lists SET icon=?2,pinned=?3,show_in_smart_lists=?4,list_type=?5 WHERE id=?1",
        params![
            list.id,
            list.icon,
            list.pinned,
            list.show_in_smart_lists,
            list.list_type
        ],
    )?;
    Ok(json!(list))
}
fn update_list(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(
        data,
        &[
            "id",
            "name",
            "folder_id",
            "color",
            "view_mode",
            "group_by",
            "sort_by",
            "sort_order",
            "icon",
            "pinned",
            "show_in_smart_lists",
            "list_type",
        ],
    )?;
    let mut list = get_list(conn, required(data, "id")?)?;
    if list.id == "inbox" {
        return invalid("Inbox is a built-in list and cannot be changed.");
    }
    let old_folder = list.folder_id.clone();
    if data.contains_key("name") {
        list.name = clean_name(required(data, "name")?, "name")?;
    }
    if data.contains_key("folder_id") {
        list.folder_id = optional_string(data, "folder_id")?;
    }
    if let Some(p) = &list.folder_id {
        get_folder(conn, p)?;
    }
    list.color = extensions::list_color(data, &list.color)?;
    list.icon = extensions::icon(data, &list.icon)?;
    list.pinned = bool_field(data, "pinned", list.pinned)?;
    list.show_in_smart_lists = bool_field(data, "show_in_smart_lists", list.show_in_smart_lists)?;
    list.list_type = extensions::list_type(data, &list.list_type)?;
    list.preferences = extensions::preferences(data, &list.preferences)?;
    if list.folder_id != old_folder {
        list.position = next_position(conn, "lists", "folder_id", list.folder_id.as_deref(), None)?;
    }
    let time = now();
    conn.execute(
        "UPDATE lists SET name=?2,folder_id=?3,color=?4,position=?5,updated_at=?6 WHERE id=?1",
        params![
            list.id,
            list.name,
            list.folder_id,
            list.color,
            list.position,
            time
        ],
    )?;
    conn.execute(
        "UPDATE lists SET preferences_json=?2 WHERE id=?1",
        params![list.id, serde_json::to_string(&list.preferences)?],
    )?;
    conn.execute(
        "UPDATE lists SET icon=?2,pinned=?3,show_in_smart_lists=?4,list_type=?5 WHERE id=?1",
        params![
            list.id,
            list.icon,
            list.pinned,
            list.show_in_smart_lists,
            list.list_type
        ],
    )?;
    Ok(json!(get_list(conn, &list.id)?))
}
fn delete_list(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id"])?;
    let list = get_list(conn, required(data, "id")?)?;
    if list.id == "inbox" {
        return invalid("Inbox cannot be deleted.");
    }
    let time = now();
    let start = next_position(conn, "tasks", "parent_id", None, Some("inbox"))?;
    // Keep root ordering distinct from the existing Inbox while preserving descendants.
    let mut stmt=conn.prepare("SELECT id FROM tasks WHERE list_id=?1 AND parent_id IS NULL ORDER BY position,created_at,id")?;
    let roots = stmt
        .query_map([&list.id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (index, id) in roots.iter().enumerate() {
        conn.execute(
            "UPDATE tasks SET position=?2 WHERE id=?1",
            params![id, start + index as i64],
        )?;
    }
    conn.execute(
        "UPDATE tasks SET list_id='inbox',details_json=json_set(details_json,'$.section_id',NULL),updated_at=?2,version=version+1 WHERE list_id=?1",
        params![list.id, time],
    )?;
    conn.execute(
        "UPDATE settings SET value='\"inbox\"' WHERE key='default_list_id' AND value=?1",
        [json!(list.id).to_string()],
    )?;
    conn.execute("DELETE FROM lists WHERE id=?1", [&list.id])?;
    extensions::prune_filters(conn)?;
    Ok(json!({"ok":true}))
}
fn reorder(conn: &Connection, table: &str, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["ids"])?;
    let values = data
        .get("ids")
        .and_then(Value::as_array)
        .ok_or_else(|| ServiceError::Invalid("ids must be an array of IDs.".into()))?;
    if values.is_empty() {
        return invalid("ids must contain at least one ID.");
    }
    if values.len() > 10000 {
        return invalid("Too many items to reorder at once.");
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for value in values {
        let id = value
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ServiceError::Invalid("Every ID must be a non-empty string.".into()))?;
        if !seen.insert(id.to_owned()) {
            return invalid("Reorder IDs must be unique.");
        }
        ids.push(id.to_owned());
    }
    let (parent, list, parent_col) = match table {
        "tasks" => {
            let first = get_task(conn, &ids[0])?;
            for id in &ids {
                let t = get_task(conn, id)?;
                if t.list_id != first.list_id || t.parent_id != first.parent_id {
                    return invalid(
                        "Only tasks in the same list and parent can be reordered together.",
                    );
                }
            }
            (first.parent_id, Some(first.list_id), "parent_id")
        }
        "folders" => {
            let first = get_folder(conn, &ids[0])?;
            for id in &ids {
                if get_folder(conn, id)?.parent_id != first.parent_id {
                    return invalid("Only sibling folders can be reordered together.");
                }
            }
            (first.parent_id, None, "parent_id")
        }
        "lists" => {
            let first = get_list(conn, &ids[0])?;
            for id in &ids {
                if get_list(conn, id)?.folder_id != first.folder_id {
                    return invalid("Only lists in the same folder can be reordered together.");
                }
            }
            (first.folder_id, None, "folder_id")
        }
        _ => unreachable!(),
    };
    let sql = if list.is_some() {
        format!("SELECT id FROM {table} WHERE {parent_col} IS ?1 AND list_id=?2 ORDER BY position,created_at,id")
    } else {
        format!("SELECT id FROM {table} WHERE {parent_col} IS ?1 ORDER BY position,created_at,id")
    };
    let mut stmt = conn.prepare(&sql)?;
    let mut ordered: Vec<String> = if let Some(l) = &list {
        stmt.query_map(params![parent, l], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    } else {
        stmt.query_map([parent], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut replacements = ids.iter();
    for id in &mut ordered {
        if seen.contains(id) {
            *id = replacements
                .next()
                .expect("one replacement for each selected ID")
                .clone();
        }
    }
    let time = now();
    let update = if table == "tasks" {
        format!("UPDATE {table} SET position=?2,updated_at=?3,version=version+1 WHERE id=?1 AND position<>?2")
    } else {
        format!("UPDATE {table} SET position=?2,updated_at=?3 WHERE id=?1 AND position<>?2")
    };
    for (position, id) in ordered.iter().enumerate() {
        conn.execute(&update, params![id, position as i64, time])?;
    }
    Ok(json!({"ok":true}))
}
fn due_reminders(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["now"])?;
    let time = match data.get("now") {
        None => now(),
        Some(_) => normalized_instant(required(data, "now")?)?,
    };
    let mut stmt=conn.prepare("SELECT r.task_id,r.at,r.generation FROM reminders r JOIN tasks t ON t.id=r.task_id JOIN lists l ON l.id=t.list_id WHERE r.at<=?1 AND r.notified_at IS NULL AND t.deleted_at IS NULL AND t.completed_at IS NULL AND json_extract(t.details_json,'$.wont_do_at') IS NULL AND l.archived_at IS NULL ORDER BY r.at,t.id")?;
    let rows = stmt
        .query_map([time], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut tasks = vec![];
    for (id, at, generation) in rows {
        let mut task = get_task(conn, &id)?;
        task.reminder_at = Some(at);
        let mut value = json!(task);
        value["reminder_generation"] = json!(generation);
        tasks.push(value);
    }
    Ok(json!(tasks))
}

fn mark_reminder(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "reminder_at", "generation"])?;
    let id = required(data, "id")?;
    let generation = required(data, "generation")?;
    get_task(conn, id)?;
    let reminder = normalized_instant(required(data, "reminder_at")?)?;
    conn.execute("UPDATE reminders SET notified_at=?3 WHERE task_id=?1 AND at=?2 AND generation=?4 AND EXISTS(SELECT 1 FROM tasks WHERE id=?1 AND completed_at IS NULL AND deleted_at IS NULL AND json_extract(details_json,'$.wont_do_at') IS NULL)",params![id,reminder,now(),generation])?;
    Ok(json!({"ok":true}))
}
