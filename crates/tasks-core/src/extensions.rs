use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskDetails {
    pub start_date: Option<String>,
    pub start_time: Option<String>,
    pub duration_minutes: Option<i64>,
    pub timezone: String,
    pub recurrence: Option<Value>,
    pub recurrence_next_id: Option<String>,
    pub is_note: bool,
    pub notes_format: String,
    pub checklist: Vec<Value>,
    pub progress: i64,
    #[serde(skip_deserializing)]
    pub progress_mode: String,
    #[serde(skip)]
    pub(super) stored_progress: Option<i64>,
    pub pinned: bool,
    pub wont_do_at: Option<String>,
    pub section_id: Option<String>,
}
impl Default for TaskDetails {
    fn default() -> Self {
        Self {
            start_date: None,
            start_time: None,
            duration_minutes: None,
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into()),
            recurrence: None,
            recurrence_next_id: None,
            is_note: false,
            notes_format: "plain".into(),
            checklist: vec![],
            progress: 0,
            progress_mode: "manual".into(),
            stored_progress: None,
            pinned: false,
            wont_do_at: None,
            section_id: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ListPreferences {
    pub view_mode: String,
    pub group_by: String,
    pub sort_by: String,
    pub sort_order: String,
}
impl Default for ListPreferences {
    fn default() -> Self {
        Self {
            view_mode: "list".into(),
            group_by: "none".into(),
            sort_by: "manual".into(),
            sort_order: "asc".into(),
        }
    }
}

pub(super) const TASK_EDIT_FIELDS: &[&str] = &[
    "title",
    "notes",
    "list_id",
    "parent_id",
    "due_date",
    "due_time",
    "reminder_at",
    "priority",
    "start_date",
    "start_time",
    "duration_minutes",
    "timezone",
    "recurrence",
    "reminders",
    "tag_ids",
    "notes_format",
    "checklist",
    "pinned",
    "section_id",
    "is_note",
    "progress",
];
pub(super) fn validate_task_fields(data: &Map<String, Value>, update: bool) -> Result<()> {
    let mut allowed = TASK_EDIT_FIELDS.to_vec();
    if update {
        allowed.extend(["id", "expected_version"]);
    }
    fields(data, &allowed)
}
pub(super) fn hydrate_task(conn: &Connection, task: &mut Task) -> Result<()> {
    task.notes_text = crate::text::visible_notes(&task.notes, &task.details.notes_format);
    let (checked, total) = checklist_progress(task);
    task.details.stored_progress = Some(task.details.progress);
    task.details.progress_mode = if total > 0 { "checklist" } else { "manual" }.into();
    task.details.progress = if task.completed_at.is_some() {
        100
    } else if total > 0 {
        (checked * 100 + total / 2) / total
    } else {
        task.details.progress.clamp(0, 100)
    };
    let (tags, attachments, links) = relations::task_metadata(conn, &task.id)?;
    task.tag_ids = tags;
    task.attachments = attachments;
    task.linked_task_ids = links;
    task.comments = comments(conn, &task.id)?;
    let mut stmt = conn.prepare("SELECT at FROM reminders WHERE task_id=?1 ORDER BY at")?;
    task.reminders = stmt
        .query_map([&task.id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    task.reminder_at = task.reminders.first().cloned();
    Ok(())
}

// Rich-note check items are real editor task items, not checkbox-looking text or
// source code. A native checklist and note check items may coexist after import.
fn checklist_progress(task: &Task) -> (i64, i64) {
    let mut checked = task
        .details
        .checklist
        .iter()
        .filter(|item| item["completed"] == true)
        .count() as i64;
    let mut total = task.details.checklist.len() as i64;
    match task.details.notes_format.as_str() {
        "markdown" => {
            for event in pulldown_cmark::Parser::new_ext(
                &task.notes,
                pulldown_cmark::Options::ENABLE_TASKLISTS,
            ) {
                if let pulldown_cmark::Event::TaskListMarker(done) = event {
                    checked += i64::from(done);
                    total += 1;
                }
            }
        }
        "html" => {
            use html5ever::tokenizer::{BufferQueue, Tokenizer};
            let input = BufferQueue::default();
            input.push_back(task.notes.as_str().into());
            let tokenizer = Tokenizer::new(NoteCheckItems::default(), Default::default());
            let _ = tokenizer.feed(&input);
            tokenizer.end();
            let counts = tokenizer.sink.counts.into_inner();
            checked += counts.0;
            total += counts.1;
        }
        _ => {}
    }
    (checked, total)
}

#[derive(Default)]
struct NoteCheckItems {
    counts: std::cell::RefCell<(i64, i64)>,
}
impl html5ever::tokenizer::TokenSink for NoteCheckItems {
    type Handle = ();
    fn process_token(
        &self,
        token: html5ever::tokenizer::Token,
        _: u64,
    ) -> html5ever::tokenizer::TokenSinkResult<()> {
        use html5ever::tokenizer::{states::RawKind, TagKind, Token, TokenSinkResult};
        if let Token::TagToken(tag) = token {
            if tag.kind == TagKind::StartTag {
                match tag.name.as_ref() {
                    "script" => return TokenSinkResult::RawData(RawKind::ScriptData),
                    "style" => return TokenSinkResult::RawData(RawKind::Rawtext),
                    "textarea" | "title" => return TokenSinkResult::RawData(RawKind::Rcdata),
                    "li" => {
                        let attr = |name: &str| {
                            tag.attrs
                                .iter()
                                .find(|a| a.name.local.as_ref() == name)
                                .map(|a| a.value.as_ref())
                        };
                        if attr("data-type")
                            .is_some_and(|value| value.eq_ignore_ascii_case("taskItem"))
                        {
                            if let Some(value @ ("true" | "false")) = attr("data-checked") {
                                let mut counts = self.counts.borrow_mut();
                                counts.0 += i64::from(value == "true");
                                counts.1 += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        TokenSinkResult::Continue
    }
}

fn stored_details_json(details: &TaskDetails) -> Result<String> {
    let mut stored = details.clone();
    stored.progress = details.stored_progress.unwrap_or(details.progress);
    stored.progress_mode = "manual".into();
    Ok(serde_json::to_string(&stored)?)
}
fn setting(conn: &Connection, key: &str) -> Result<Value> {
    let text: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(text
        .map(|t| serde_json::from_str(&t))
        .transpose()?
        .unwrap_or(Value::Null))
}
pub(super) fn creation_defaults(
    conn: &Connection,
    input: &Map<String, Value>,
) -> Result<Map<String, Value>> {
    let mut data = input.clone();
    if !data.contains_key("priority") {
        data.insert("priority".into(), setting(conn, "default_priority")?);
    }
    if !data.contains_key("list_id") && !data.contains_key("parent_id") {
        data.insert("list_id".into(), setting(conn, "default_list_id")?);
    }
    if !data.contains_key("tag_ids") {
        let tag = setting(conn, "default_tag_id")?;
        if let Some(id) = tag.as_str() {
            data.insert("tag_ids".into(), json!([id]));
        }
    }
    if !data.contains_key("due_date") {
        let default = setting(conn, "default_date")?;
        let days = match default.as_str() {
            Some("today") => Some(0),
            Some("tomorrow") => Some(1),
            _ => None,
        };
        if let Some(days) = days {
            data.insert(
                "due_date".into(),
                json!((Local::now().date_naive() + chrono::Duration::days(days)).to_string()),
            );
        }
    }
    if !data.contains_key("reminders") && !data.contains_key("reminder_at") {
        if let Some(date) = data.get("due_date").and_then(Value::as_str) {
            let timezone = data
                .get("timezone")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| TaskDetails::default().timezone);
            let timed = data.get("due_time").and_then(Value::as_str);
            let default = setting(
                conn,
                if timed.is_some() {
                    "default_reminder_timed"
                } else {
                    "default_reminder_allday"
                },
            )?;
            if let Some(preference) = default.as_str().filter(|s| *s != "none") {
                let time = timed.unwrap_or(preference);
                let instant = recurrence::local_instant(date, time, &timezone)?;
                let minutes = match preference {
                    "5m" => 5,
                    "15m" => 15,
                    "30m" => 30,
                    "1h" => 60,
                    _ => 0,
                };
                let reminder = DateTime::parse_from_rfc3339(&instant)
                    .map_err(|_| ServiceError::Invalid("Invalid reminder".into()))?
                    - chrono::Duration::minutes(minutes);
                data.insert(
                    "reminder_at".into(),
                    json!(reminder
                        .with_timezone(&Utc)
                        .to_rfc3339_opts(SecondsFormat::Millis, true)),
                );
            }
        }
    }
    Ok(data)
}
pub(super) fn prepare_task(
    conn: &Connection,
    task: &mut Task,
    data: &Map<String, Value>,
) -> Result<()> {
    let details = &mut task.details;
    for (key, target) in [
        ("start_date", &mut details.start_date),
        ("start_time", &mut details.start_time),
        ("section_id", &mut details.section_id),
    ] {
        if data.contains_key(key) {
            *target = optional_string(data, key)?;
        }
    }
    if data.get("start_date") == Some(&Value::Null) && !data.contains_key("start_time") {
        details.start_time = None;
    }
    if let Some(v) = data.get("duration_minutes") {
        details.duration_minutes = if v.is_null() {
            None
        } else {
            Some(
                v.as_i64()
                    .filter(|n| (1..=525600).contains(n))
                    .ok_or_else(|| {
                        ServiceError::Invalid(
                            "duration_minutes must be between 1 and 525600.".into(),
                        )
                    })?,
            )
        };
    }
    details.timezone = text_field(data, "timezone", &details.timezone)?;
    recurrence::validate_timezone(&details.timezone)?;
    if let Some(date) = &details.start_date {
        parse_date(date)?;
    }
    if let Some(time) = &details.start_time {
        let date = details
            .start_date
            .as_deref()
            .ok_or_else(|| ServiceError::Invalid("A start time requires a start date.".into()))?;
        recurrence::local_instant(date, time, &details.timezone)?;
    }
    if let (Some(start), Some(due)) = (&details.start_date, &task.due_date) {
        if start > due
            || (start == due
                && details.start_time.as_deref().unwrap_or("00:00")
                    > task.due_time.as_deref().unwrap_or("23:59"))
        {
            return invalid("Start must not be later than the due date/time.");
        }
    }
    if let Some(v) = data.get("recurrence") {
        details.recurrence = if v.is_null() {
            None
        } else {
            Some(recurrence::validate_rule(v)?)
        };
    }
    if let Some(rule) = &mut details.recurrence {
        let due = task
            .due_date
            .as_deref()
            .ok_or_else(|| ServiceError::Invalid("A repeating task requires a due date.".into()))?;
        if rule.get("anchor_date").is_none_or(Value::is_null) {
            rule["anchor_date"] = json!(due);
        }
        if data.contains_key("recurrence") || data.contains_key("due_date") {
            recurrence::validate_calendar(due, rule)?;
        }
    }
    details.notes_format = text_field(data, "notes_format", &details.notes_format)?;
    if !["plain", "markdown", "html"].contains(&details.notes_format.as_str()) {
        return invalid("notes_format must be plain, markdown, or html.");
    }
    details.pinned = bool_field(data, "pinned", details.pinned)?;
    details.is_note = bool_field(data, "is_note", details.is_note)?;
    if let Some(value) = data.get("progress") {
        details.progress = value
            .as_i64()
            .filter(|progress| (0..=100).contains(progress))
            .ok_or_else(|| {
                ServiceError::Invalid("progress must be an integer between 0 and 100.".into())
            })?;
        details.stored_progress = Some(details.progress);
    }
    if let Some(v) = data.get("checklist") {
        let items = v.as_array().filter(|v| v.len() <= 1000).ok_or_else(|| {
            ServiceError::Invalid("checklist must be an array of at most 1000 items.".into())
        })?;
        let mut seen = HashSet::new();
        let mut normalized = vec![];
        for value in items {
            let item = value
                .as_object()
                .ok_or_else(|| ServiceError::Invalid("Checklist item must be an object.".into()))?;
            fields(item, &["id", "text", "completed"])?;
            let id = optional_string(item, "id")?.unwrap_or_else(|| Uuid::new_v4().to_string());
            if !seen.insert(id.clone()) {
                return invalid("Checklist item IDs must be unique.");
            }
            normalized.push(json!({"id":id,"text":clean_name(required(item,"text")?,"text")?,"completed":bool_field(item,"completed",false)?}));
        }
        details.checklist = normalized;
    }
    if let Some(section) = &details.section_id {
        let list: Option<String> = conn
            .query_row("SELECT list_id FROM sections WHERE id=?1", [section], |r| {
                r.get(0)
            })
            .optional()?;
        if list.as_deref() != Some(task.list_id.as_str()) {
            if data.contains_key("section_id") {
                return invalid("Section must belong to this task's list.");
            }
            details.section_id = None;
        }
    }
    if let Some(values) = data.get("reminders") {
        if data.contains_key("reminder_at") {
            return invalid("Supply reminders or reminder_at, not both.");
        }
        let values = values.as_array().filter(|v| v.len() <= 20).ok_or_else(|| {
            ServiceError::Invalid("reminders must contain at most 20 RFC3339 timestamps.".into())
        })?;
        task.reminders = values
            .iter()
            .map(|v| {
                normalized_instant(v.as_str().ok_or_else(|| {
                    ServiceError::Invalid("Each reminder must be an RFC3339 timestamp.".into())
                })?)
            })
            .collect::<Result<_>>()?;
        task.reminders.sort();
        task.reminders.dedup();
        task.reminder_at = task.reminders.first().cloned();
    } else if data.contains_key("reminder_at") {
        task.reminders = task.reminder_at.iter().cloned().collect();
    }
    Ok(())
}
pub(super) fn persist_details(
    conn: &Connection,
    task: &Task,
    data: &Map<String, Value>,
) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET details_json=?2 WHERE id=?1",
        params![task.id, stored_details_json(&task.details)?],
    )?;
    if let Some(tags) = data.get("tag_ids") {
        relations::replace_tags(conn, &task.id, tags)?;
    }
    let mut stmt = conn.prepare("SELECT at FROM reminders WHERE task_id=?1")?;
    let previous = stmt
        .query_map([&task.id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for at in previous {
        if !task.reminders.contains(&at) {
            conn.execute(
                "DELETE FROM reminders WHERE task_id=?1 AND at=?2",
                params![task.id, at],
            )?;
        }
    }
    for at in &task.reminders {
        conn.execute(
            "INSERT OR IGNORE INTO reminders(task_id,at,generation) VALUES(?1,?2,?3)",
            params![task.id, at, Uuid::new_v4().to_string()],
        )?;
    }
    Ok(())
}
pub(super) fn apply_position_default(conn: &Connection, task: &mut Task) -> Result<()> {
    if setting(conn, "default_add_to")?.as_str() == Some("top") {
        task.position = conn.query_row(
            "SELECT COALESCE(MIN(position),1)-1 FROM tasks WHERE list_id=?1 AND parent_id IS ?2",
            params![task.list_id, task.parent_id],
            |r| r.get(0),
        )?;
        conn.execute(
            "UPDATE tasks SET position=?2 WHERE id=?1",
            params![task.id, task.position],
        )?;
    }
    Ok(())
}
pub(super) fn preferences(
    data: &Map<String, Value>,
    old: &ListPreferences,
) -> Result<ListPreferences> {
    let mut prefs = old.clone();
    for (key, target, allowed) in [
        (
            "view_mode",
            &mut prefs.view_mode,
            &["list", "board", "timeline"][..],
        ),
        (
            "group_by",
            &mut prefs.group_by,
            &["none", "section", "date", "priority", "list", "tag"][..],
        ),
        (
            "sort_by",
            &mut prefs.sort_by,
            &["manual", "due_date", "priority", "title", "created_at"][..],
        ),
        ("sort_order", &mut prefs.sort_order, &["asc", "desc"][..]),
    ] {
        *target = text_field(data, key, target)?;
        if !allowed.contains(&target.as_str()) {
            return invalid(format!("Invalid {key}."));
        }
    }
    Ok(prefs)
}

pub(super) fn default_settings() -> Map<String, Value> {
    json!({"smart_lists":{"today":"show","upcoming":"show","inbox":"show","all":"hide","completed":"show","trash":"show","wont_do":"auto","tomorrow":"hide","assigned":"auto","summary":"auto","tags":"show","filters":"show"},"theme_color":"default","font_size":"normal","sidebar_count":"all","completed_style":"strikethrough","show_list_color":false,"completion_sound":true,"week_start":1,"show_week_numbers":false,"time_format":24,"date_recognition":"remove","tag_recognition":"remove","default_priority":0,"default_list_id":"inbox","default_tag_id":null,"default_date":"none","default_reminder_timed":"none","default_reminder_allday":"none","default_add_to":"top","overdue_position":"top","view_preferences":{},"show_timezone":false,"hotkeys":{},"saved_filters":[],"summary_templates":[],"interface_style":"card","app_icon":"default","show_dock_icon":true,"dock_count":"today","launch_at_login":false,"launch_minimized":false,"global_quick_add_shortcut":"","global_show_hide_shortcut":""}).as_object().expect("object").clone()
}
pub(super) fn initialize_settings(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET details_json=?1 WHERE details_json='{}'",
        [serde_json::to_string(&TaskDetails::default())?],
    )?;
    for (key, value) in default_settings() {
        conn.execute(
            "INSERT OR IGNORE INTO settings(key,value) VALUES(?1,?2)",
            params![key, value.to_string()],
        )?;
    }
    Ok(())
}
fn enum_value(value: &Value, values: &[&str]) -> bool {
    value.as_str().is_some_and(|s| values.contains(&s))
}
fn valid_summary_templates(value: &Value) -> bool {
    let Some(templates) = value.as_array().filter(|items| items.len() <= 100) else {
        return false;
    };
    let mut seen = HashSet::new();
    templates.iter().all(|template| {
        let Some(template) = template.as_object() else {
            return false;
        };
        if fields(template, &["id", "name", "options"]).is_err() {
            return false;
        }
        let short_text = |key: &str| {
            template.get(key).and_then(Value::as_str).filter(|text| {
                !text.trim().is_empty()
                    && text.chars().count() <= 100
                    && !text.chars().any(char::is_control)
            })
        };
        let Some(id) = short_text("id") else {
            return false;
        };
        if !seen.insert(id) || short_text("name").is_none() {
            return false;
        }
        let Some(options) = template.get("options").and_then(Value::as_object) else {
            return false;
        };
        if fields(
            options,
            &[
                "range",
                "from",
                "to",
                "basis",
                "list_ids",
                "tag_ids",
                "statuses",
                "priorities",
                "group_by",
                "sort_by",
                "include_subtasks",
                "fields",
            ],
        )
        .is_err()
        {
            return false;
        }
        let choice = |key: &str, allowed: &[&str]| {
            options
                .get(key)
                .is_some_and(|value| enum_value(value, allowed))
        };
        let choices = |key: &str, allowed: &[&str]| {
            options
                .get(key)
                .and_then(Value::as_array)
                .is_some_and(|values| {
                    values.len() <= allowed.len()
                        && values.iter().all(|value| enum_value(value, allowed))
                        && values
                            .iter()
                            .enumerate()
                            .all(|(index, value)| !values[..index].contains(value))
                })
        };
        let ids = |key: &str| {
            options
                .get(key)
                .and_then(Value::as_array)
                .is_some_and(|values| {
                    values.len() <= 1000
                        && values.iter().all(|value| {
                            value.as_str().is_some_and(|id| {
                                !id.is_empty()
                                    && id.len() <= 200
                                    && !id.chars().any(char::is_control)
                            })
                        })
                })
        };
        let from = options
            .get("from")
            .and_then(Value::as_str)
            .and_then(|date| parse_date(date).ok());
        let to = options
            .get("to")
            .and_then(Value::as_str)
            .and_then(|date| parse_date(date).ok());
        from.zip(to).is_some_and(|(from, to)| from <= to)
            && choice("range", &["this_week", "last_week", "this_month", "custom"])
            && choice("basis", &["completed", "due", "created"])
            && choice("group_by", &["none", "list", "status"])
            && choice("sort_by", &["date", "title", "priority"])
            && ids("list_ids")
            && ids("tag_ids")
            && choices("statuses", &["active", "completed", "wont_do"])
            && choices(
                "fields",
                &[
                    "list",
                    "due",
                    "completed",
                    "priority",
                    "tags",
                    "progress",
                    "notes",
                ],
            )
            && options
                .get("include_subtasks")
                .is_some_and(Value::is_boolean)
            && options
                .get("priorities")
                .and_then(Value::as_array)
                .is_some_and(|values| {
                    values.len() <= 4
                        && values
                            .iter()
                            .all(|value| value.as_i64().is_some_and(|n| (0..=3).contains(&n)))
                        && values
                            .iter()
                            .enumerate()
                            .all(|(index, value)| !values[..index].contains(value))
                })
    })
}
pub(super) fn set_setting(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["key", "value"])?;
    let key = required(data, "key")?;
    let value = data
        .get("value")
        .ok_or_else(|| ServiceError::Invalid("value is required.".into()))?;
    let valid = match key {
        "theme" => enum_value(value, &["light", "dark", "system"]),
        "interface_style" => enum_value(value, &["card", "flat"]),
        "app_icon" => enum_value(value, &["default", "rounded"]),
        "dock_count" => enum_value(value, &["none", "today", "inbox", "all", "overdue"]),
        "theme_color" => enum_value(
            value,
            &[
                "default",
                "sky",
                "turquoise",
                "teal",
                "matcha",
                "sunshine",
                "peach",
                "lilac",
                "ebony",
                "navy",
                "gray",
                "dark",
            ],
        ),
        "notifications" | "show_list_color" | "completion_sound" | "show_week_numbers"
        | "show_timezone" | "show_dock_icon" => value.is_boolean(),
        "launch_at_login"
        | "launch_minimized"
        | "global_quick_add_shortcut"
        | "global_show_hide_shortcut" => {
            crate::validate_desktop_integration_setting(key, value)?;
            let other = match key {
                "global_quick_add_shortcut" => Some("global_show_hide_shortcut"),
                "global_show_hide_shortcut" => Some("global_quick_add_shortcut"),
                _ => None,
            };
            if let Some(other) = other.filter(|_| value.as_str().is_some_and(|s| !s.is_empty())) {
                let saved: Option<String> = conn
                    .query_row("SELECT value FROM settings WHERE key=?1", [other], |row| {
                        row.get(0)
                    })
                    .optional()?;
                if saved
                    .as_deref()
                    .map(serde_json::from_str::<Value>)
                    .transpose()?
                    .as_ref()
                    == Some(value)
                {
                    return invalid(
                        "Quick Add and Show/Hide Main Window must use different global shortcuts.",
                    );
                }
            }
            true
        }
        "sidebar_width" => value.as_f64().is_some_and(|n| (150.0..=500.0).contains(&n)),
        "list_width" => value
            .as_f64()
            .is_some_and(|n| (250.0..=1000.0).contains(&n)),
        "font_size" => enum_value(value, &["normal", "large", "extra-large"]),
        "sidebar_count" => enum_value(value, &["all", "tasks", "hidden"]),
        "completed_style" => enum_value(value, &["default", "strikethrough"]),
        "week_start" => value.as_i64().is_some_and(|n| [0, 1, 6].contains(&n)),
        "time_format" => value.as_i64().is_some_and(|n| [12, 24].contains(&n)),
        "date_recognition" => enum_value(value, &["off", "keep", "remove"]),
        "tag_recognition" => enum_value(value, &["keep", "remove"]),
        "default_priority" => value.as_i64().is_some_and(|n| (0..=3).contains(&n)),
        "default_list_id" => value
            .as_str()
            .is_some_and(|id| get_list(conn, id).is_ok_and(|list| list.archived_at.is_none())),
        "default_tag_id" => {
            value.is_null()
                || value.as_str().is_some_and(|id| {
                    conn.query_row("SELECT 1 FROM tags WHERE id=?1", [id], |_| Ok(()))
                        .is_ok()
                })
        }
        "default_date" => enum_value(value, &["none", "today", "tomorrow"]),
        "default_reminder_timed" => {
            enum_value(value, &["none", "on-time", "5m", "15m", "30m", "1h"])
        }
        "default_reminder_allday" => enum_value(value, &["none", "09:00", "18:00"]),
        "default_add_to" | "overdue_position" => enum_value(value, &["top", "bottom"]),
        "smart_lists" => value.as_object().is_some_and(|v| {
            v.iter().all(|(k, v)| {
                [
                    "all",
                    "today",
                    "tomorrow",
                    "upcoming",
                    "inbox",
                    "completed",
                    "trash",
                    "wont_do",
                    "assigned",
                    "summary",
                    "tags",
                    "filters",
                ]
                .contains(&k.as_str())
                    && enum_value(v, &["show", "hide", "auto"])
            })
        }),
        "view_preferences" => value.as_object().is_some_and(|v| {
            v.iter().all(|(key, p)| {
                ([
                    "today",
                    "tomorrow",
                    "upcoming",
                    "all",
                    "inbox",
                    "completed",
                    "trash",
                    "wont_do",
                ]
                .contains(&key.as_str())
                    || key.starts_with("tag:")
                    || key.starts_with("filter:"))
                    && p.as_object().is_some_and(|p| {
                        fields(p, &["view_mode", "group_by", "sort_by", "sort_order"]).is_ok()
                            && preferences(p, &ListPreferences::default()).is_ok()
                    })
            })
        }),
        "saved_filters" => valid_saved_filters(conn, value),
        "summary_templates" => valid_summary_templates(value),
        "hotkeys" => value.as_object().is_some_and(|v| {
            v.iter().all(|(key, chord)| {
                [
                    "quick_add",
                    "print_task",
                    "search",
                    "settings",
                    "shortcuts",
                    "complete_task",
                    "delete_task",
                    "pin_task",
                    "set_date",
                    "clear_date",
                    "set_today",
                    "set_tomorrow",
                    "set_next_week",
                    "priority_none",
                    "priority_low",
                    "priority_medium",
                    "priority_high",
                    "list_view",
                    "kanban_view",
                    "toggle_all_groups",
                    "toggle_all_subtasks",
                    "add_subtask",
                    "today",
                    "tomorrow",
                    "upcoming",
                    "inbox",
                    "all",
                    "completed",
                    "wont_do",
                    "trash",
                ]
                .contains(&key.as_str())
                    && chord.as_str().is_some_and(|s| {
                        s.len() <= 64 && s.chars().all(|c| c.is_ascii() && !c.is_ascii_control())
                    })
            })
        }),
        _ => return invalid("Unknown setting."),
    };
    if !valid {
        return invalid(format!("Invalid value for setting {key}."));
    }
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value.to_string()])?;
    Ok(json!({"ok":true}))
}

fn touch_details(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET details_json=?2,updated_at=?3,version=version+1 WHERE id=?1",
        params![task.id, stored_details_json(&task.details)?, now()],
    )?;
    Ok(())
}
pub(super) fn complete(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    let before = get_task(conn, required(data, "id")?)?;
    let completed = bool_field(data, "completed", true)?;
    complete_task(conn, data)?;
    for id in if completed {
        subtree(conn, "tasks", &before.id)?
    } else {
        ancestors(conn, &before.id)?
    } {
        let mut task = get_task(conn, &id)?;
        if task.deleted_at.is_some() {
            continue;
        }
        if task.details.wont_do_at.take().is_some() {
            touch_details(conn, &task)?;
        }
    }
    if completed && before.completed_at.is_none() && before.details.recurrence_next_id.is_none() {
        if let (Some(due), Some(rule)) = (&before.due_date, &before.details.recurrence) {
            let zone: chrono_tz::Tz = before
                .details
                .timezone
                .parse()
                .map_err(|_| ServiceError::Invalid("Invalid timezone.".into()))?;
            let today = Utc::now().with_timezone(&zone).date_naive().to_string();
            if let Some(next) = recurrence::next_date(due, rule, &today)? {
                let existing: Option<String> = conn
                    .query_row(
                        "SELECT id FROM tasks WHERE recurrence_source_id=?1",
                        [&before.id],
                        |r| r.get(0),
                    )
                    .optional()?;
                let next_id = if let Some(id) = existing {
                    id
                } else {
                    let shift = (parse_date(&next)? - parse_date(due)?).num_days();
                    let clone = duplicate_tree(
                        conn,
                        &before.id,
                        &before.list_id,
                        before.parent_id.as_deref(),
                        None,
                        Some(shift),
                        true,
                    )?;
                    conn.execute(
                        "UPDATE tasks SET recurrence_source_id=?2 WHERE id=?1",
                        params![clone.id, before.id],
                    )?;
                    clone.id
                };
                let mut finished = get_task(conn, &before.id)?;
                finished.details.recurrence_next_id = Some(next_id);
                touch_details(conn, &finished)?;
            }
        }
    }
    Ok(json!(get_task(conn, &before.id)?))
}
fn task_creation_data(task: &Task, list_id: &str, parent_id: Option<&str>) -> Map<String, Value> {
    json!({"title":task.title,"notes":task.notes,"list_id":list_id,"parent_id":parent_id,"due_date":task.due_date,"due_time":task.due_time,"priority":task.priority,"start_date":task.details.start_date,"start_time":task.details.start_time,"duration_minutes":task.details.duration_minutes,"timezone":task.details.timezone,"recurrence":task.details.recurrence,"reminders":task.reminders,"tag_ids":task.tag_ids,"notes_format":task.details.notes_format,"is_note":task.details.is_note,"checklist":task.details.checklist,"progress":task.details.stored_progress.unwrap_or(task.details.progress),"pinned":task.details.pinned,"section_id":if task.list_id==list_id{task.details.section_id.clone()}else{None}}).as_object().expect("object").clone()
}
fn child_ids(conn: &Connection, id: &str, include_deleted: bool) -> Result<Vec<String>> {
    let mut stmt=conn.prepare("SELECT id FROM tasks WHERE parent_id=?1 AND (?2 OR deleted_at IS NULL) ORDER BY position,created_at,id")?;
    let values = stmt
        .query_map(params![id, include_deleted], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(values)
}
fn duplicate_tree(
    conn: &Connection,
    id: &str,
    list_id: &str,
    parent: Option<&str>,
    title: Option<&str>,
    shift: Option<i64>,
    recurring: bool,
) -> Result<Task> {
    let source = get_task(conn, id)?;
    let mut data = task_creation_data(&source, list_id, parent);
    data.insert("progress".into(), json!(0));
    data.insert(
        "notes".into(),
        json!(reset_note_checkboxes(
            &source.notes,
            &source.details.notes_format
        )),
    );
    if let Some(title) = title {
        data.insert("title".into(), json!(clean_name(title, "title")?));
    }
    if let Some(items) = data.get_mut("checklist").and_then(Value::as_array_mut) {
        for item in items {
            item["id"] = json!(Uuid::new_v4().to_string());
            item["completed"] = json!(false);
        }
    }
    if let Some(days) = shift {
        for key in ["due_date", "start_date"] {
            if let Some(date) = data.get(key).and_then(Value::as_str) {
                let next = parse_date(date)?
                    .checked_add_signed(chrono::Duration::days(days))
                    .ok_or_else(|| {
                        ServiceError::Invalid("Repeating date is out of range.".into())
                    })?;
                data.insert(key.into(), json!(next.to_string()));
            }
        }
        let reminders: Vec<String> = source
            .reminders
            .iter()
            .map(|at| {
                if let (Some(old), Some(new)) = (
                    &source.due_date,
                    data.get("due_date").and_then(Value::as_str),
                ) {
                    recurrence::shift_instant_by_due(
                        at,
                        old,
                        new,
                        source.due_time.as_deref(),
                        &source.details.timezone,
                    )
                } else {
                    let at = DateTime::parse_from_rfc3339(at)
                        .map_err(|_| ServiceError::Invalid("Invalid reminder.".into()))?;
                    Ok((at + chrono::Duration::days(days))
                        .with_timezone(&Utc)
                        .to_rfc3339_opts(SecondsFormat::Millis, true))
                }
            })
            .collect::<Result<_>>()?;
        data.insert("reminders".into(), json!(reminders));
    }
    if recurring {
        if let Some(rule) = data.get_mut("recurrence").and_then(Value::as_object_mut) {
            if let Some(count) = rule.get("count").and_then(Value::as_i64) {
                rule.insert("count".into(), json!(count - 1));
            }
        }
    }
    let created = create_task(conn, &data)?;
    let new_id = created["id"].as_str().expect("task id");
    relations::duplicate_metadata(conn, id, new_id)?;
    // Preserve the source child order independently of the user's quick-add preference.
    for (position, child) in child_ids(conn, id, source.deleted_at.is_some())?
        .iter()
        .enumerate()
    {
        let candidate = get_task(conn, child)?;
        if shift.is_some() && candidate.details.recurrence_next_id.is_some() {
            continue;
        }
        let (child_shift, advance_child) = if shift.is_some() {
            if let (Some(due), Some(rule)) = (&candidate.due_date, &candidate.details.recurrence) {
                let zone: chrono_tz::Tz = candidate
                    .details
                    .timezone
                    .parse()
                    .map_err(|_| ServiceError::Invalid("Invalid timezone.".into()))?;
                let today = Utc::now().with_timezone(&zone).date_naive().to_string();
                let Some(next) = recurrence::next_date(due, rule, &today)? else {
                    continue;
                };
                (
                    Some((parse_date(&next)? - parse_date(due)?).num_days()),
                    true,
                )
            } else {
                (shift, false)
            }
        } else {
            (None, false)
        };
        let child = duplicate_tree(
            conn,
            child,
            list_id,
            Some(new_id),
            None,
            child_shift,
            advance_child,
        )?;
        if advance_child {
            // Advancing as part of the parent is still this child's one next
            // occurrence. Record the same identity link as direct completion so
            // reopening an old child cannot fork its already-generated series.
            conn.execute(
                "UPDATE tasks SET recurrence_source_id=?2 WHERE id=?1",
                params![child.id, candidate.id],
            )?;
            let mut previous = get_task(conn, &candidate.id)?;
            previous.details.recurrence_next_id = Some(child.id.clone());
            touch_details(conn, &previous)?;
        }
        conn.execute(
            "UPDATE tasks SET position=?2 WHERE id=?1",
            params![child.id, position as i64],
        )?;
    }
    get_task(conn, new_id)
}
fn wont_do(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "wont_do"])?;
    let task = get_task(conn, required(data, "id")?)?;
    if task.deleted_at.is_some() {
        return invalid("Restore a task from Trash first.");
    }
    let dismiss = bool_field(data, "wont_do", true)?;
    let timestamp = now();
    for id in if dismiss {
        subtree(conn, "tasks", &task.id)?
    } else {
        ancestors(conn, &task.id)?
    } {
        let mut item = get_task(conn, &id)?;
        if item.deleted_at.is_some() {
            continue;
        }
        item.details.wont_do_at = if dismiss {
            Some(timestamp.clone())
        } else {
            None
        };
        conn.execute("UPDATE tasks SET completed_at=NULL,details_json=?2,updated_at=?3,version=version+1 WHERE id=?1",params![id,serde_json::to_string(&item.details)?,timestamp])?;
    }
    Ok(json!(get_task(conn, &task.id)?))
}
pub(super) fn sections(conn: &Connection) -> Result<Value> {
    let mut stmt=conn.prepare("SELECT id,list_id,name,position,created_at,updated_at FROM sections ORDER BY position,created_at,id")?;
    let items=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"list_id":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"position":r.get::<_,i64>(3)?,"created_at":r.get::<_,String>(4)?,"updated_at":r.get::<_,String>(5)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!(items))
}
fn section(conn: &Connection, id: &str) -> Result<Value> {
    conn.query_row("SELECT id,list_id,name,position,created_at,updated_at FROM sections WHERE id=?1",[id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"list_id":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"position":r.get::<_,i64>(3)?,"created_at":r.get::<_,String>(4)?,"updated_at":r.get::<_,String>(5)?}))).optional()?.ok_or_else(||ServiceError::NotFound("Section".into()))
}
fn section_mutate(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    let timestamp = now();
    match action {
        "sections.create" => {
            fields(data, &["list_id", "name"])?;
            let list = required(data, "list_id")?;
            get_list(conn, list)?;
            let id = Uuid::new_v4().to_string();
            let name = clean_name(required(data, "name")?, "name")?;
            let position: i64 = conn.query_row(
                "SELECT COALESCE(MAX(position),-1)+1 FROM sections WHERE list_id=?1",
                [list],
                |r| r.get(0),
            )?;
            conn.execute("INSERT INTO sections(id,list_id,name,position,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)",params![id,list,name,position,timestamp])?;
            section(conn, &id)
        }
        "sections.update" => {
            fields(data, &["id", "name"])?;
            let id = required(data, "id")?;
            let old = section(conn, id)?;
            let name = clean_name(
                &text_field(data, "name", old["name"].as_str().expect("name"))?,
                "name",
            )?;
            conn.execute(
                "UPDATE sections SET name=?2,updated_at=?3 WHERE id=?1",
                params![id, name, timestamp],
            )?;
            section(conn, id)
        }
        "sections.delete" => {
            fields(data, &["id"])?;
            let id = required(data, "id")?;
            section(conn, id)?;
            conn.execute("UPDATE tasks SET details_json=json_set(details_json,'$.section_id',NULL),updated_at=?2,version=version+1 WHERE json_extract(details_json,'$.section_id')=?1",params![id,timestamp])?;
            conn.execute("DELETE FROM sections WHERE id=?1", [id])?;
            Ok(json!({"ok":true}))
        }
        "sections.reorder" => {
            fields(data, &["ids"])?;
            let ids = data
                .get("ids")
                .and_then(Value::as_array)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| ServiceError::Invalid("ids must be a non-empty array.".into()))?;
            let mut seen = HashSet::new();
            let mut list = None;
            for id in ids {
                let id = id
                    .as_str()
                    .ok_or_else(|| ServiceError::Invalid("Section IDs must be strings.".into()))?;
                if !seen.insert(id) {
                    return invalid("Section IDs must be unique.");
                }
                let item = section(conn, id)?;
                if list.as_ref().is_some_and(|l| l != &item["list_id"]) {
                    return invalid("Sections must be in the same list.");
                }
                list = Some(item["list_id"].clone());
            }
            let mut stmt = conn.prepare(
                "SELECT id FROM sections WHERE list_id=?1 ORDER BY position,created_at,id",
            )?;
            let mut ordered = stmt
                .query_map([list.as_ref().and_then(Value::as_str)], |r| {
                    r.get::<_, String>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut replacements = ids.iter();
            for id in &mut ordered {
                if seen.contains(id.as_str()) {
                    *id = replacements
                        .next()
                        .and_then(Value::as_str)
                        .expect("one selected section per slot")
                        .to_string();
                }
            }
            for (pos, id) in ordered.iter().enumerate() {
                conn.execute(
                    "UPDATE sections SET position=?2,updated_at=?3 WHERE id=?1",
                    params![id, pos as i64, timestamp],
                )?;
            }
            Ok(json!({"ok":true}))
        }
        _ => invalid("Unknown section action."),
    }
}
fn capture_template(conn: &Connection, id: &str) -> Result<Value> {
    let task = get_task(conn, id)?;
    let mut data = task_creation_data(&task, &task.list_id, None);
    data.remove("parent_id");
    let mut files = vec![];
    for file in &task.attachments {
        files.push(relations::read(
            conn,
            "attachments.get",
            json!({"id":file["id"]}).as_object().expect("object"),
        )?);
    }
    let children = child_ids(conn, id, false)?
        .iter()
        .map(|id| capture_template(conn, id))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"data":data,"children":children,"files":files}))
}
fn template_preview(payload: &Value) -> Value {
    let mut preview = payload.get("data").cloned().unwrap_or(json!({}));
    preview["subtasks"] = json!(payload
        .get("children")
        .and_then(Value::as_array)
        .map(|v| v.iter().map(template_preview).collect::<Vec<_>>())
        .unwrap_or_default());
    preview
}
pub(super) fn migrate_template_previews(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id FROM templates")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        let payload = template_payload(conn, &id)?;
        conn.execute(
            "UPDATE templates SET preview_json=?2 WHERE id=?1",
            params![id, template_preview(&payload).to_string()],
        )?;
    }
    Ok(())
}
pub(super) fn templates(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT id,name,preview_json,created_at,updated_at FROM templates ORDER BY created_at,id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut values = vec![];
    for (id, name, payload, created, updated) in rows {
        let payload: Value = serde_json::from_str(&payload)?;
        values.push(
            json!({"id":id,"name":name,"task":payload,"created_at":created,"updated_at":updated}),
        );
    }
    Ok(json!(values))
}
fn template_payload(conn: &Connection, id: &str) -> Result<Value> {
    let text: Option<String> = conn
        .query_row("SELECT task_json FROM templates WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(serde_json::from_str(&text.ok_or_else(|| {
        ServiceError::NotFound("Template".into())
    })?)?)
}
fn apply_template(
    conn: &Connection,
    payload: &Value,
    list: &str,
    parent: Option<&str>,
) -> Result<Task> {
    let mut data = payload["data"]
        .as_object()
        .ok_or_else(|| ServiceError::Invalid("Invalid stored template.".into()))?
        .clone();
    let notes = data.get("notes").and_then(Value::as_str).unwrap_or("");
    let format = data
        .get("notes_format")
        .and_then(Value::as_str)
        .unwrap_or("plain");
    data.insert("notes".into(), json!(reset_note_checkboxes(notes, format)));
    data.insert("list_id".into(), json!(list));
    data.insert("parent_id".into(), json!(parent));
    data.insert("section_id".into(), Value::Null);
    data.insert("progress".into(), json!(0));
    if let Some(ids) = data.get_mut("tag_ids").and_then(Value::as_array_mut) {
        ids.retain(|id| {
            id.as_str().is_some_and(|id| {
                conn.query_row("SELECT 1 FROM tags WHERE id=?1", [id], |_| Ok(()))
                    .is_ok()
            })
        });
    }
    if let Some(items) = data.get_mut("checklist").and_then(Value::as_array_mut) {
        for item in items {
            item["id"] = json!(Uuid::new_v4().to_string());
            item["completed"] = json!(false);
        }
    }
    let created = create_task(conn, &data)?;
    let id = created["id"].as_str().expect("id");
    if let Some(files) = payload.get("files").and_then(Value::as_array) {
        for file in files {
            relations::mutate(conn,"attachments.add",json!({"id":id,"name":file["name"],"mime_type":file["mime_type"],"content_base64":file["content_base64"]}).as_object().expect("object"))?;
        }
    }
    if let Some(children) = payload.get("children").and_then(Value::as_array) {
        for (pos, child) in children.iter().enumerate() {
            let child = apply_template(conn, child, list, Some(id))?;
            conn.execute(
                "UPDATE tasks SET position=?2 WHERE id=?1",
                params![child.id, pos as i64],
            )?;
        }
    }
    get_task(conn, id)
}
pub(super) fn read(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    match action {
        "comments.list" => {
            fields(data, &["id"])?;
            let id = required(data, "id")?;
            get_task(conn, id)?;
            Ok(json!(comments(conn, id)?))
        }
        "templates.list" => {
            fields(data, &[])?;
            templates(conn)
        }
        "templates.get" => {
            fields(data, &["id"])?;
            let id = required(data, "id")?;
            templates(conn)?
                .as_array()
                .expect("array")
                .iter()
                .find(|t| t["id"] == id)
                .cloned()
                .ok_or_else(|| ServiceError::NotFound("Template".into()))
        }
        _ => invalid("Unknown read action."),
    }
}
pub(super) fn mutate(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    match action {
        "comments.add" | "comments.update" | "comments.delete" => {
            comment_mutate(conn, action, data)
        }
        "lists.duplicate" => duplicate_list(conn, data),
        "tasks.duplicate" => {
            fields(data, &["id", "list_id", "title"])?;
            let source = get_task(conn, required(data, "id")?)?;
            let list = text_field(data, "list_id", &source.list_id)?;
            let title = optional_string(data, "title")?;
            get_list(conn, &list)?;
            let parent = if list == source.list_id {
                source.parent_id.as_deref()
            } else {
                None
            };
            Ok(json!(duplicate_tree(
                conn,
                &source.id,
                &list,
                parent,
                title.as_deref(),
                None,
                false
            )?))
        }
        "tasks.wont_do" => wont_do(conn, data),
        "lists.archive" => {
            fields(data, &["id", "archived"])?;
            let id = required(data, "id")?;
            get_list(conn, id)?;
            if id == "inbox" {
                return invalid("Inbox cannot be archived.");
            }
            let archived = bool_field(data, "archived", true)?;
            let time = now();
            conn.execute(
                "UPDATE lists SET archived_at=?2,updated_at=?3 WHERE id=?1",
                params![id, if archived { Some(time.clone()) } else { None }, time],
            )?;
            if archived {
                conn.execute("UPDATE settings SET value='\"inbox\"' WHERE key='default_list_id' AND value=?1",[json!(id).to_string()])?;
            }
            Ok(json!(get_list(conn, id)?))
        }
        "sections.create" | "sections.update" | "sections.delete" | "sections.reorder" => {
            section_mutate(conn, action, data)
        }
        "templates.create" => {
            fields(data, &["name", "task_id"])?;
            let name = clean_name(required(data, "name")?, "name")?;
            let payload = capture_template(conn, required(data, "task_id")?)?;
            let id = Uuid::new_v4().to_string();
            let time = now();
            conn.execute("INSERT INTO templates(id,name,task_json,preview_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)",params![id,name,payload.to_string(),template_preview(&payload).to_string(),time])?;
            read(
                conn,
                "templates.get",
                json!({"id":id}).as_object().expect("object"),
            )
        }
        "templates.update" => {
            fields(data, &["id", "name"])?;
            let id = required(data, "id")?;
            template_payload(conn, id)?;
            let old_name: String =
                conn.query_row("SELECT name FROM templates WHERE id=?1", [id], |r| r.get(0))?;
            let name = clean_name(&text_field(data, "name", &old_name)?, "name")?;
            conn.execute(
                "UPDATE templates SET name=?2,updated_at=?3 WHERE id=?1",
                params![id, name, now()],
            )?;
            read(
                conn,
                "templates.get",
                json!({"id":id}).as_object().expect("object"),
            )
        }
        "templates.delete" => {
            fields(data, &["id"])?;
            let id = required(data, "id")?;
            template_payload(conn, id)?;
            conn.execute("DELETE FROM templates WHERE id=?1", [id])?;
            Ok(json!({"ok":true}))
        }
        "templates.apply" => {
            fields(data, &["id", "list_id"])?;
            let payload = template_payload(conn, required(data, "id")?)?;
            let list = text_field(data, "list_id", "inbox")?;
            get_list(conn, &list)?;
            Ok(json!(apply_template(conn, &payload, &list, None)?))
        }
        _ => invalid("Unknown Tasks extension action."),
    }
}

pub(super) fn comments(conn: &Connection, task_id: &str) -> Result<Vec<Value>> {
    let mut stmt=conn.prepare("SELECT id,task_id,body,created_at,updated_at,version FROM comments WHERE task_id=?1 ORDER BY created_at,id")?;
    let mut items=stmt.query_map([task_id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"task_id":r.get::<_,String>(1)?,"body":r.get::<_,String>(2)?,"created_at":r.get::<_,String>(3)?,"updated_at":r.get::<_,String>(4)?,"author":"You","version":r.get::<_,i64>(5)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare(
        "SELECT attachment_id FROM comment_attachments WHERE comment_id=?1 ORDER BY attachment_id",
    )?;
    for item in &mut items {
        let ids = stmt
            .query_map([item["id"].as_str().expect("id")], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        item["attachment_ids"] = json!(ids);
    }
    Ok(items)
}
fn comment_mutate(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    fields(
        data,
        if action == "comments.delete" {
            &["id", "expected_version"][..]
        } else {
            &["id", "body", "attachment_ids", "expected_version"][..]
        },
    )?;
    let id = required(data, "id")?;
    let (task_id, previous_body): (String, String) = if action == "comments.add" {
        (get_task(conn, id)?.id, String::new())
    } else {
        conn.query_row("SELECT task_id,body FROM comments WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?
        .ok_or_else(|| ServiceError::NotFound("Comment".into()))?
    };
    if action != "comments.add" {
        if let Some(value) = data.get("expected_version") {
            let expected = value.as_i64().ok_or_else(|| {
                ServiceError::Invalid("expected_version must be an integer.".into())
            })?;
            let actual: i64 =
                conn.query_row("SELECT version FROM comments WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
            if expected != actual {
                return Err(ServiceError::Conflict);
            }
        }
    }
    let body = text_field(data, "body", &previous_body)?.trim().to_string();
    if body.chars().count() > 10000 || body.contains('\0') {
        return invalid("Comments must contain at most 10000 characters.");
    }
    let mut attachment_ids = vec![];
    if let Some(ids) = data.get("attachment_ids") {
        let ids = ids.as_array().filter(|v| v.len() <= 20).ok_or_else(|| {
            ServiceError::Invalid("A comment may contain at most 20 attachments.".into())
        })?;
        for id in ids {
            let id = id
                .as_str()
                .ok_or_else(|| ServiceError::Invalid("Attachment IDs must be strings.".into()))?;
            let owner: Option<String> = conn
                .query_row("SELECT task_id FROM attachments WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            if owner.as_deref() != Some(&task_id) {
                return invalid("Comment attachments must belong to this task.");
            }
            if !attachment_ids.contains(&id.to_string()) {
                attachment_ids.push(id.to_string());
            }
        }
    } else if action != "comments.add" {
        let mut stmt =
            conn.prepare("SELECT attachment_id FROM comment_attachments WHERE comment_id=?1")?;
        attachment_ids = stmt
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
    }
    if action != "comments.delete" && body.is_empty() && attachment_ids.is_empty() {
        return invalid("Write a comment or attach an image.");
    }
    let time = now();
    let comment_id = if action == "comments.add" {
        Uuid::new_v4().to_string()
    } else {
        id.into()
    };
    match action {
        "comments.add" => {
            conn.execute("INSERT INTO comments(id,task_id,body,created_at,updated_at) VALUES(?1,?2,?3,?4,?4)",params![comment_id,task_id,body,time])?;
        }
        "comments.update" => {
            conn.execute(
                "UPDATE comments SET body=?2,updated_at=?3,version=version+1 WHERE id=?1",
                params![id, body, time],
            )?;
        }
        "comments.delete" => {
            conn.execute("DELETE FROM comments WHERE id=?1", [id])?;
        }
        _ => return invalid("Unknown comment action."),
    }
    if action != "comments.delete" {
        conn.execute(
            "DELETE FROM comment_attachments WHERE comment_id=?1",
            [&comment_id],
        )?;
        for attachment in attachment_ids {
            conn.execute(
                "INSERT INTO comment_attachments(comment_id,attachment_id) VALUES(?1,?2)",
                params![comment_id, attachment],
            )?;
        }
    }
    conn.execute(
        "UPDATE tasks SET version=version+1,updated_at=?2 WHERE id=?1",
        params![task_id, time],
    )?;
    if action == "comments.delete" {
        Ok(json!({"ok":true}))
    } else {
        comments(conn, &task_id)?
            .into_iter()
            .find(|c| c["id"] == comment_id)
            .ok_or_else(|| ServiceError::NotFound("Comment".into()))
    }
}
fn valid_saved_filters(conn: &Connection, value: &Value) -> bool {
    let Some(filters) = value.as_array().filter(|v| v.len() <= 100) else {
        return false;
    };
    let mut seen = HashSet::new();
    filters.iter().all(|filter| {
        let Some(filter) = filter.as_object() else {
            return false;
        };
        if fields(
            filter,
            &[
                "id",
                "name",
                "list_ids",
                "tag_ids",
                "priorities",
                "date",
                "match",
            ],
        )
        .is_err()
        {
            return false;
        }
        let Some(id) = filter
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 100)
        else {
            return false;
        };
        if !seen.insert(id) {
            return false;
        }
        if !filter
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 100)
        {
            return false;
        }
        for (key, table) in [("list_ids", "lists"), ("tag_ids", "tags")] {
            if !filter
                .get(key)
                .and_then(Value::as_array)
                .is_some_and(|ids| {
                    ids.len() <= 100
                        && ids.iter().all(|id| {
                            id.as_str().is_some_and(|id| {
                                conn.query_row(
                                    &format!("SELECT 1 FROM {table} WHERE id=?1"),
                                    [id],
                                    |_| Ok(()),
                                )
                                .is_ok()
                            })
                        })
                })
            {
                return false;
            }
        }
        filter
            .get("priorities")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.len() <= 4
                    && items
                        .iter()
                        .all(|v| v.as_i64().is_some_and(|n| (0..=3).contains(&n)))
            })
            && filter.get("date").is_some_and(|v| {
                enum_value(
                    v,
                    &["all", "today", "tomorrow", "week", "overdue", "undated"],
                )
            })
            && filter
                .get("match")
                .is_some_and(|v| enum_value(v, &["all", "any"]))
    })
}
pub(super) fn prune_filters(conn: &Connection) -> Result<()> {
    let mut value = setting(conn, "saved_filters")?;
    if let Some(filters) = value.as_array_mut() {
        for filter in filters {
            for (key, table) in [("list_ids", "lists"), ("tag_ids", "tags")] {
                if let Some(ids) = filter.get_mut(key).and_then(Value::as_array_mut) {
                    ids.retain(|id| {
                        id.as_str().is_some_and(|id| {
                            conn.query_row(
                                &format!("SELECT 1 FROM {table} WHERE id=?1"),
                                [id],
                                |_| Ok(()),
                            )
                            .is_ok()
                        })
                    });
                }
            }
        }
    }
    conn.execute(
        "UPDATE settings SET value=?1 WHERE key='saved_filters'",
        [value.to_string()],
    )?;
    Ok(())
}

fn reset_note_checkboxes(notes: &str, format: &str) -> String {
    use std::sync::LazyLock;
    static TASK_ITEM: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?is)<li\b(?:[^\"'<>]|\"[^\"]*\"|'[^']*')*>"#)
            .expect("static task item pattern")
    });
    static TASK_TYPE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)\bdata-type\s*=\s*(?:\"taskItem\"|'taskItem'|taskItem)(?:\s|/?>)"#)
            .expect("static task type pattern")
    });
    static CHECKED: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)\bdata-checked\s*=\s*(?:\"true\"|'true'|true)(\s|/?>)"#)
            .expect("static checked pattern")
    });
    match format {
        "html" => TASK_ITEM
            .replace_all(notes, |captures: &regex::Captures| {
                let tag = &captures[0];
                if TASK_TYPE.is_match(tag) {
                    CHECKED
                        .replace_all(tag, "data-checked=\"false\"$1")
                        .into_owned()
                } else {
                    tag.to_string()
                }
            })
            .into_owned(),
        "markdown" => {
            // Use the parser only to locate actual task-list markers. Replacing
            // their single ASCII state byte preserves all other source bytes,
            // including fenced/indented code, escaped text and formatting.
            let mut result = notes.as_bytes().to_vec();
            let parser =
                pulldown_cmark::Parser::new_ext(notes, pulldown_cmark::Options::ENABLE_TASKLISTS);
            for (event, range) in parser.into_offset_iter() {
                if matches!(event, pulldown_cmark::Event::TaskListMarker(true)) {
                    let marker = &notes[range.clone()];
                    if let Some(offset) = marker.find("[x]").or_else(|| marker.find("[X]")) {
                        result[range.start + offset + 1] = b' ';
                    }
                }
            }
            String::from_utf8(result).expect("only ASCII task marker bytes changed")
        }
        _ => notes.to_string(),
    }
}

pub(super) fn icon(data: &Map<String, Value>, fallback: &str) -> Result<String> {
    let icon = if data.get("icon") == Some(&Value::Null) {
        String::new()
    } else {
        text_field(data, "icon", fallback)?
    };
    if icon.chars().count() > 64 || icon.chars().any(char::is_control) {
        return invalid("Icon must be an emoji or short icon name, at most64 characters.");
    }
    Ok(icon)
}
pub(super) fn list_type(data: &Map<String, Value>, fallback: &str) -> Result<String> {
    let value = text_field(data, "list_type", fallback)?;
    if !["task", "note"].contains(&value.as_str()) {
        return invalid("list_type must be task or note.");
    }
    Ok(value)
}
pub(super) fn list_color(data: &Map<String, Value>, fallback: &str) -> Result<String> {
    if data.get("color").and_then(Value::as_str) == Some("")
        || (!data.contains_key("color") && fallback.is_empty())
    {
        Ok(String::new())
    } else {
        color(data, fallback)
    }
}
fn clone_list_tree(
    conn: &Connection,
    source_id: &str,
    list: &str,
    parent: Option<&str>,
    section_map: &std::collections::HashMap<String, String>,
    mapping: &mut std::collections::HashMap<String, String>,
) -> Result<()> {
    let source = get_task(conn, source_id)?;
    let mut data = task_creation_data(&source, list, parent);
    data.insert(
        "section_id".into(),
        json!(source
            .details
            .section_id
            .as_ref()
            .and_then(|id| section_map.get(id))),
    );
    if let Some(items) = data.get_mut("checklist").and_then(Value::as_array_mut) {
        for item in items {
            item["id"] = json!(Uuid::new_v4().to_string());
        }
    }
    let cloned = create_task(conn, &data)?;
    let id = cloned["id"].as_str().expect("created task id").to_string();
    relations::duplicate_metadata(conn, source_id, &id)?;
    mapping.insert(source_id.into(), id.clone());
    for child in child_ids(conn, source_id, false)? {
        clone_list_tree(conn, &child, list, Some(&id), section_map, mapping)?;
    }
    Ok(())
}
fn duplicate_list(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "name", "folder_id"])?;
    let source = get_list(conn, required(data, "id")?)?;
    let name = text_field(data, "name", &format!("{} (copy)", source.name))?;
    let folder = if data.contains_key("folder_id") {
        optional_string(data, "folder_id")?
    } else {
        source.folder_id.clone()
    };
    let list=create_list(conn,json!({"name":name,"folder_id":folder,"color":source.color,"icon":source.icon,"pinned":false,"show_in_smart_lists":source.show_in_smart_lists,"list_type":source.list_type,"view_mode":source.preferences.view_mode,"group_by":source.preferences.group_by,"sort_by":source.preferences.sort_by,"sort_order":source.preferences.sort_order}).as_object().expect("object"))?;
    let list_id = list["id"].as_str().expect("list id");
    let mut section_map = std::collections::HashMap::new();
    for section in sections(conn)?
        .as_array()
        .expect("sections array")
        .iter()
        .filter(|s| s["list_id"] == source.id)
    {
        let created = section_mutate(
            conn,
            "sections.create",
            json!({"list_id":list_id,"name":section["name"]})
                .as_object()
                .expect("object"),
        )?;
        section_map.insert(
            section["id"].as_str().expect("section id").to_string(),
            created["id"].as_str().expect("section id").to_string(),
        );
    }
    let mut mapping = std::collections::HashMap::new();
    let mut stmt=conn.prepare("SELECT id FROM tasks WHERE list_id=?1 AND parent_id IS NULL AND deleted_at IS NULL ORDER BY position,created_at,id")?;
    let roots = stmt
        .query_map([&source.id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in roots {
        clone_list_tree(conn, &id, list_id, None, &section_map, &mut mapping)?;
    }
    // Restore copied lifecycle states only after children exist; otherwise creating
    // a child would correctly reopen its completed parent during construction.
    for (old_id, new_id) in &mapping {
        let old = get_task(conn, old_id)?;
        let mut new = get_task(conn, new_id)?;
        new.details.wont_do_at = old.details.wont_do_at.clone();
        new.details.recurrence_next_id = old
            .details
            .recurrence_next_id
            .as_ref()
            .and_then(|id| mapping.get(id).cloned());
        let old_source: Option<String> = conn.query_row(
            "SELECT recurrence_source_id FROM tasks WHERE id=?1",
            [old_id],
            |r| r.get(0),
        )?;
        let new_source = old_source.as_ref().and_then(|id| mapping.get(id));
        conn.execute("UPDATE tasks SET details_json=?2,completed_at=?3,position=?4,recurrence_source_id=?5 WHERE id=?1",params![new_id,serde_json::to_string(&new.details)?,old.completed_at,old.position,new_source])?;
    }
    let mut stmt = conn.prepare("SELECT task_id,linked_task_id FROM task_links")?;
    let links = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (left, right) in links {
        if mapping.contains_key(&left) || mapping.contains_key(&right) {
            let a = mapping.get(&left).unwrap_or(&left);
            let b = mapping.get(&right).unwrap_or(&right);
            let (a, b) = if a < b { (a, b) } else { (b, a) };
            conn.execute(
                "INSERT OR IGNORE INTO task_links(task_id,linked_task_id) VALUES(?1,?2)",
                params![a, b],
            )?;
            conn.execute(
                "UPDATE tasks SET version=version+1,updated_at=?3 WHERE id IN (?1,?2)",
                params![a, b, now()],
            )?;
        }
    }
    Ok(json!(get_list(conn, list_id)?))
}
