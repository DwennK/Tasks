use clap::{Args, Subcommand};
use serde_json::{json, Map, Value};

/// Less frequently used task fields; --fields covers nullable clearing and future fields.
#[derive(Args)]
pub struct TaskFields {
    /// Convert between note (true) and task (false); defaults to the list kind
    #[arg(long)]
    is_note: Option<bool>,
    /// Start date YYYY-MM-DD
    #[arg(long)]
    start: Option<String>,
    #[arg(long)]
    start_time: Option<String>,
    /// Duration in minutes
    #[arg(long)]
    duration: Option<u32>,
    /// IANA timezone such as Europe/Zurich
    #[arg(long)]
    timezone: Option<String>,
    /// Recurrence JSON object (or null to clear)
    #[arg(long)]
    repeat: Option<String>,
    /// Comma-separated absolute RFC3339 reminders; empty string clears
    #[arg(long)]
    reminders: Option<String>,
    /// Comma-separated tag IDs; empty string clears
    #[arg(long)]
    tags: Option<String>,
    #[arg(long,value_parser=["plain","markdown","html"])]
    notes_format: Option<String>,
    /// JSON array of {id,text,completed} checklist items
    #[arg(long)]
    checklist: Option<String>,
    #[arg(long)]
    section: Option<String>,
    /// Pin/unpin with true or false
    #[arg(long)]
    pinned: Option<bool>,
    /// Extra editable fields as a JSON object; explicit named options take precedence
    #[arg(long)]
    fields: Option<String>,
}
impl TaskFields {
    pub fn apply(self, map: &mut Map<String, Value>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(fields) = self.fields {
            let parsed: Value = serde_json::from_str(&fields)?;
            let Some(obj) = parsed.as_object() else {
                return Err("--fields must be a JSON object".into());
            };
            for (k, v) in obj {
                if k == "id" {
                    return Err("--fields cannot replace the task ID".into());
                }
                map.entry(k).or_insert_with(|| v.clone());
            }
        }
        for (k, v) in [
            ("start_date", self.start),
            ("start_time", self.start_time),
            ("timezone", self.timezone),
            ("notes_format", self.notes_format),
            ("section_id", self.section),
        ] {
            if let Some(v) = v {
                map.insert(k.into(), json!(v));
            }
        }
        if let Some(v) = self.is_note {
            map.insert("is_note".into(), json!(v));
        }
        if let Some(v) = self.duration {
            map.insert("duration_minutes".into(), json!(v));
        }
        if let Some(v) = self.pinned {
            map.insert("pinned".into(), json!(v));
        }
        for (k, v) in [("recurrence", self.repeat), ("checklist", self.checklist)] {
            if let Some(v) = v {
                map.insert(k.into(), serde_json::from_str(&v)?);
            }
        }
        for (k, v) in [("reminders", self.reminders), ("tag_ids", self.tags)] {
            if let Some(v) = v {
                let values: Vec<&str> = v
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                map.insert(k.into(), json!(values));
            }
        }
        Ok(())
    }
}
#[derive(Subcommand)]
pub enum TagCommand {
    Add {
        name: String,
        #[arg(long)]
        color: Option<String>,
    },
    Edit {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        color: Option<String>,
    },
    Delete {
        id: String,
    },
    /// Replace all tags on a task (omit tag IDs to clear)
    Set {
        id: String,
        tag_ids: Vec<String>,
        /// Reject if this task changed after the specified version
        #[arg(long)]
        expected_version: Option<u64>,
    },
}
#[derive(Subcommand)]
pub enum SectionCommand {
    Add {
        list_id: String,
        name: String,
    },
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
    },
    Reorder {
        #[arg(required = true)]
        ids: Vec<String>,
    },
}
#[derive(Subcommand)]
pub enum TemplateCommand {
    Get {
        id: String,
    },
    Add {
        name: String,
        task_id: String,
    },
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
    },
    Apply {
        id: String,
        #[arg(long = "list")]
        list_id: Option<String>,
    },
}
#[derive(Subcommand)]
pub enum AttachmentCommand {
    /// Copy a local file (maximum 20 MiB) into the task database
    Import {
        task_id: String,
        path: String,
    },
    /// Show attachment metadata for a task
    List {
        task_id: String,
    },
    /// Read a bounded chunk as base64 JSON; offsets are raw bytes
    Read {
        id: String,
        #[arg(long, default_value = "0")]
        offset: u64,
        #[arg(long, default_value = "262144")]
        length: u32,
    },
    Remove {
        id: String,
    },
}
#[derive(Subcommand)]
pub enum LinkCommand {
    Add { id: String, linked_task_id: String },
    Remove { id: String, linked_task_id: String },
}
#[derive(Subcommand)]
pub enum SettingsCommand {
    /// Persist one setting with a JSON-encoded value
    Set { key: String, value: String },
}
#[derive(Args)]
pub struct NavigationMetadata {
    /// List/folder icon, up to64 characters
    #[arg(long, conflicts_with = "clear_icon")]
    icon: Option<String>,
    /// Remove the custom icon
    #[arg(long)]
    clear_icon: bool,
    /// Pin/unpin in the sidebar
    #[arg(long)]
    pinned: Option<bool>,
}
impl NavigationMetadata {
    pub fn apply(self, map: &mut Map<String, Value>) {
        if self.clear_icon {
            map.insert("icon".into(), Value::Null);
        } else if let Some(icon) = self.icon {
            map.insert("icon".into(), json!(icon));
        }
        if let Some(value) = self.pinned {
            map.insert("pinned".into(), json!(value));
        }
    }
}
#[derive(Args)]
pub struct ListPreferences {
    #[command(flatten)]
    metadata: NavigationMetadata,
    #[arg(long)]
    show_in_smart_lists: Option<bool>,
    #[arg(long,value_parser=["task","note"])]
    list_type: Option<String>,
    #[arg(long,value_parser=["list","board"])]
    view_mode: Option<String>,
    #[arg(long,value_parser=["none","section","date","priority","list","tag"])]
    group_by: Option<String>,
    #[arg(long,value_parser=["manual","due_date","priority","title","created_at"])]
    sort_by: Option<String>,
    #[arg(long,value_parser=["asc","desc"])]
    sort_order: Option<String>,
}
impl ListPreferences {
    pub fn apply(self, map: &mut Map<String, Value>) {
        self.metadata.apply(map);
        if let Some(value) = self.show_in_smart_lists {
            map.insert("show_in_smart_lists".into(), json!(value));
        }
        if let Some(value) = self.list_type {
            map.insert("list_type".into(), json!(value));
        }
        for (k, v) in [
            ("view_mode", self.view_mode),
            ("group_by", self.group_by),
            ("sort_by", self.sort_by),
            ("sort_order", self.sort_order),
        ] {
            if let Some(v) = v {
                map.insert(k.into(), json!(v));
            }
        }
    }
}
pub fn tag(command: Option<TagCommand>) -> (String, Value) {
    match command {
        None => ("tags.list".into(), json!({})),
        Some(TagCommand::Add { name, color }) => {
            let mut m = json!({"name":name});
            if let Some(v) = color {
                m["color"] = json!(v);
            }
            ("tags.create".into(), m)
        }
        Some(TagCommand::Edit { id, name, color }) => {
            let mut m = json!({"id":id});
            if let Some(v) = name {
                m["name"] = json!(v);
            }
            if let Some(v) = color {
                m["color"] = json!(v);
            }
            ("tags.update".into(), m)
        }
        Some(TagCommand::Delete { id }) => ("tags.delete".into(), json!({"id":id})),
        Some(TagCommand::Set {
            id,
            tag_ids,
            expected_version,
        }) => {
            let mut data = json!({"id":id,"tag_ids":tag_ids});
            if let Some(version) = expected_version {
                data["expected_version"] = json!(version);
            }
            ("tasks.tags".into(), data)
        }
    }
}
pub fn section(command: SectionCommand) -> (String, Value) {
    match command {
        SectionCommand::Add { list_id, name } => (
            "sections.create".into(),
            json!({"list_id":list_id,"name":name}),
        ),
        SectionCommand::Rename { id, name } => {
            ("sections.update".into(), json!({"id":id,"name":name}))
        }
        SectionCommand::Delete { id } => ("sections.delete".into(), json!({"id":id})),
        SectionCommand::Reorder { ids } => ("sections.reorder".into(), json!({"ids":ids})),
    }
}
pub fn template(command: TemplateCommand) -> (String, Value) {
    match command {
        TemplateCommand::Get { id } => ("templates.get".into(), json!({"id":id})),
        TemplateCommand::Add { name, task_id } => (
            "templates.create".into(),
            json!({"name":name,"task_id":task_id}),
        ),
        TemplateCommand::Rename { id, name } => {
            ("templates.update".into(), json!({"id":id,"name":name}))
        }
        TemplateCommand::Delete { id } => ("templates.delete".into(), json!({"id":id})),
        TemplateCommand::Apply { id, list_id } => {
            let mut m = json!({"id":id});
            if let Some(v) = list_id {
                m["list_id"] = json!(v);
            }
            ("templates.apply".into(), m)
        }
    }
}
pub fn link(command: LinkCommand) -> (String, Value) {
    match command {
        LinkCommand::Add { id, linked_task_id } => (
            "links.add".into(),
            json!({"id":id,"linked_task_id":linked_task_id}),
        ),
        LinkCommand::Remove { id, linked_task_id } => (
            "links.remove".into(),
            json!({"id":id,"linked_task_id":linked_task_id}),
        ),
    }
}

#[derive(Subcommand)]
pub enum CommentCommand {
    List {
        task_id: String,
    },
    Add {
        task_id: String,
        #[arg(default_value = "")]
        body: String,
        #[arg(long, value_delimiter = ',')]
        attachment_ids: Option<Vec<String>>,
    },
    Edit {
        id: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long, value_delimiter = ',')]
        attachment_ids: Option<Vec<String>>,
        /// Version of the comment itself, returned by comments list
        #[arg(long)]
        expected_version: Option<u64>,
    },
    Delete {
        id: String,
        /// Version of the comment itself, returned by comments list
        #[arg(long)]
        expected_version: Option<u64>,
    },
}
pub fn comment(command: CommentCommand) -> (String, Value) {
    match command {
        CommentCommand::List { task_id } => ("comments.list".into(), json!({"id":task_id})),
        CommentCommand::Add {
            task_id,
            body,
            attachment_ids,
        } => {
            let mut data = json!({"id":task_id,"body":body});
            if let Some(ids) = attachment_ids {
                data["attachment_ids"] = json!(ids);
            }
            ("comments.add".into(), data)
        }
        CommentCommand::Edit {
            id,
            body,
            attachment_ids,
            expected_version,
        } => {
            let mut data = json!({"id":id});
            if let Some(body) = body {
                data["body"] = json!(body);
            }
            if let Some(ids) = attachment_ids {
                data["attachment_ids"] = json!(ids);
            }
            if let Some(version) = expected_version {
                data["expected_version"] = json!(version);
            }
            ("comments.update".into(), data)
        }
        CommentCommand::Delete {
            id,
            expected_version,
        } => {
            let mut data = json!({"id":id});
            if let Some(version) = expected_version {
                data["expected_version"] = json!(version);
            }
            ("comments.delete".into(), data)
        }
    }
}
