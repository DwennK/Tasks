use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Map, Value};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};
use tasks_core::{default_db_path, TaskService};
mod extended;
use extended::*;

const MAX_STDIN_JSON_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Parser)]
#[command(
    name = "tasks",
    version,
    about = "Your local Tasks database, from the terminal",
    long_about = "Manage the same offline SQLite database as the Tasks macOS app and MCP server.\nChanges are transactional. Deleted tasks can be restored. No account or network is used."
)]
struct Cli {
    /// SQLite database path (defaults to TASKS_DB, then application support directory)
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// Print machine-readable JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// List tasks (top-level active tasks by default)
    List(ListArgs),
    /// Get a task by ID, including notes and scheduling fields
    Get { id: String },
    /// Add a task; put its title in quotes
    Add(CreateArgs),
    /// Edit only the supplied fields
    Edit(EditArgs),
    /// Complete a task and all its subtasks
    Complete { id: String },
    /// Reopen a task and its required ancestors
    Reopen { id: String },
    /// Move a task and its subtasks to recoverable Trash
    Delete { id: String },
    /// Restore a task from Trash
    Restore { id: String },
    /// Reorder sibling tasks in the given ID order
    Reorder {
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Manage task lists
    Lists {
        #[command(subcommand)]
        command: Option<ListCommand>,
    },
    /// Manage nested folders
    Folders {
        #[command(subcommand)]
        command: Option<FolderCommand>,
    },
    /// Duplicate task and subtasks, resetting completed/trash states
    Duplicate {
        id: String,
        #[arg(long = "list")]
        list_id: Option<String>,
        #[arg(long)]
        title: Option<String>,
    },
    /// Dismiss a task and its subtree; --undo reactivates
    WontDo {
        id: String,
        #[arg(long)]
        undo: bool,
    },
    /// Pin a task; --undo unpins
    Pin {
        id: String,
        #[arg(long)]
        undo: bool,
    },
    /// Manage tags or replace a task's tag assignments
    Tags {
        #[command(subcommand)]
        command: Option<TagCommand>,
    },
    /// Manage list sections; no subcommand prints all sections
    Sections {
        #[command(subcommand)]
        command: Option<SectionCommand>,
    },
    /// Save and reuse task templates
    Templates {
        #[command(subcommand)]
        command: Option<TemplateCommand>,
    },
    /// Import, inspect or remove local file attachments
    Attachments {
        #[command(subcommand)]
        command: AttachmentCommand,
    },
    /// Link related tasks
    Links {
        #[command(subcommand)]
        command: LinkCommand,
    },
    /// Manage task comments stored locally
    Comments {
        #[command(subcommand)]
        command: CommentCommand,
    },
    /// Inspect or update persisted application settings
    Settings {
        #[command(subcommand)]
        command: Option<SettingsCommand>,
    },
    /// Print the full persisted snapshot as JSON
    Snapshot,
    /// Print the resolved database path
    DbPath,
    /// Execute an advanced shared service action with a JSON object
    #[command(alias = "call")]
    Raw {
        action: String,
        /// JSON object, or - to read one JSON object from stdin (maximum 32 MiB)
        #[arg(default_value = "{}")]
        input: String,
    },
}
#[derive(Clone, ValueEnum)]
enum View {
    Inbox,
    Today,
    Tomorrow,
    Upcoming,
    All,
    Completed,
    Trash,
    WontDo,
}
impl View {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Today => "today",
            Self::Tomorrow => "tomorrow",
            Self::Upcoming => "upcoming",
            Self::All => "all",
            Self::Completed => "completed",
            Self::Trash => "trash",
            Self::WontDo => "wont_do",
        }
    }
}
#[derive(Args)]
struct ListArgs {
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    section: Option<String>,
    #[arg(long, value_enum, default_value = "all")]
    view: View,
    #[arg(long = "list")]
    list_id: Option<String>,
    #[arg(long)]
    search: Option<String>,
    /// Local date used for Today / Upcoming (YYYY-MM-DD; defaults to local today)
    #[arg(long)]
    date: Option<String>,
    #[arg(long)]
    parent: Option<String>,
    #[arg(long)]
    include_subtasks: bool,
}
#[derive(Args)]
struct CreateArgs {
    title: String,
    #[command(flatten)]
    fields: TaskFields,
    #[arg(long = "list")]
    list_id: Option<String>,
    #[arg(long)]
    parent: Option<String>,
    #[arg(long)]
    notes: Option<String>,
    /// Due date in YYYY-MM-DD
    #[arg(long)]
    due: Option<String>,
    /// Due time in HH:MM (requires a due date)
    #[arg(long)]
    time: Option<String>,
    /// Reminder instant in RFC3339, for example 2026-10-28T09:00:00+01:00
    #[arg(long)]
    reminder: Option<String>,
    /// 0 none, 1 low, 2 medium, 3 high
    #[arg(long,value_parser=clap::value_parser!(u8).range(0..=3))]
    priority: Option<u8>,
}
#[derive(Args)]
struct EditArgs {
    id: String,
    #[command(flatten)]
    fields: TaskFields,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long = "list")]
    list_id: Option<String>,
    #[arg(long, conflicts_with = "clear_parent")]
    parent: Option<String>,
    #[arg(long)]
    clear_parent: bool,
    #[arg(long, conflicts_with = "clear_due")]
    due: Option<String>,
    #[arg(long)]
    clear_due: bool,
    #[arg(long, conflicts_with = "clear_time")]
    time: Option<String>,
    #[arg(long)]
    clear_time: bool,
    #[arg(long, conflicts_with = "clear_reminder")]
    reminder: Option<String>,
    #[arg(long)]
    clear_reminder: bool,
    #[arg(long,value_parser=clap::value_parser!(u8).range(0..=3))]
    priority: Option<u8>,
    /// Reject edits if another writer has updated this version
    #[arg(long)]
    expected_version: Option<u64>,
}
#[derive(Subcommand)]
enum ListCommand {
    Add {
        name: String,
        #[command(flatten)]
        preferences: ListPreferences,
        #[arg(long)]
        folder: Option<String>,
        #[arg(long)]
        color: Option<String>,
    },
    Edit {
        id: String,
        #[command(flatten)]
        preferences: ListPreferences,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "root")]
        folder: Option<String>,
        #[arg(long)]
        root: bool,
        #[arg(long)]
        color: Option<String>,
    },
    /// Copy a list, its sections and items, preserving item states
    Duplicate {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "root")]
        folder: Option<String>,
        #[arg(long)]
        root: bool,
    },
    /// Archive a list; --undo unarchives
    Archive {
        id: String,
        #[arg(long)]
        undo: bool,
    },
    /// Delete list and move its tasks to Inbox
    Delete { id: String },
    Reorder {
        #[arg(required = true)]
        ids: Vec<String>,
    },
}
#[derive(Subcommand)]
enum FolderCommand {
    Add {
        name: String,
        #[command(flatten)]
        metadata: NavigationMetadata,
        #[arg(long)]
        parent: Option<String>,
    },
    Edit {
        id: String,
        #[command(flatten)]
        metadata: NavigationMetadata,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "root")]
        parent: Option<String>,
        #[arg(long)]
        root: bool,
    },
    /// Delete folder and lift its contents to its parent
    Delete { id: String },
    Reorder {
        #[arg(required = true)]
        ids: Vec<String>,
    },
}
fn put(map: &mut Map<String, Value>, key: &str, value: Option<impl Into<Value>>) {
    if let Some(v) = value {
        map.insert(key.into(), v.into());
    }
}
fn nullable(map: &mut Map<String, Value>, key: &str, value: Option<String>, clear: bool) {
    if clear {
        map.insert(key.into(), Value::Null);
    } else {
        put(map, key, value);
    }
}
fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let path = cli.db.unwrap_or_else(default_db_path);
    if matches!(cli.command, Command::DbPath) {
        println!("{}", path.display());
        return Ok(());
    }
    let mut projection = None;
    let force_json = matches!(
        cli.command,
        Command::Snapshot
            | Command::Raw { .. }
            | Command::Get { .. }
            | Command::Attachments {
                command: AttachmentCommand::Read { .. }
            }
            | Command::Templates {
                command: Some(TemplateCommand::Get { .. })
            }
    );
    let (action, input): (String, Value) = match cli.command {
        Command::List(a) => {
            let mut m = Map::new();
            m.insert("view".into(), json!(a.view.as_str()));
            m.insert(
                "date".into(),
                json!(a
                    .date
                    .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string())),
            );
            if a.include_subtasks {
                m.insert("include_subtasks".into(), json!(true));
            }
            put(&mut m, "list_id", a.list_id);
            put(&mut m, "tag_id", a.tag);
            put(&mut m, "section_id", a.section);
            put(&mut m, "search", a.search);
            put(&mut m, "parent_id", a.parent);
            ("tasks.list".into(), Value::Object(m))
        }
        Command::Get { id } => ("tasks.get".into(), json!({"id":id})),
        Command::Add(a) => {
            let mut m = Map::new();
            m.insert("title".into(), json!(a.title));
            put(&mut m, "list_id", a.list_id);
            put(&mut m, "parent_id", a.parent);
            put(&mut m, "notes", a.notes);
            put(&mut m, "due_date", a.due);
            put(&mut m, "due_time", a.time);
            put(&mut m, "reminder_at", a.reminder);
            put(&mut m, "priority", a.priority);
            a.fields.apply(&mut m)?;
            ("tasks.create".into(), Value::Object(m))
        }
        Command::Edit(a) => {
            let mut m = Map::new();
            m.insert("id".into(), json!(a.id));
            put(&mut m, "title", a.title);
            put(&mut m, "notes", a.notes);
            put(&mut m, "list_id", a.list_id);
            nullable(&mut m, "parent_id", a.parent, a.clear_parent);
            nullable(&mut m, "due_date", a.due, a.clear_due);
            nullable(&mut m, "due_time", a.time, a.clear_time || a.clear_due);
            nullable(&mut m, "reminder_at", a.reminder, a.clear_reminder);
            put(&mut m, "priority", a.priority);
            put(&mut m, "expected_version", a.expected_version);
            a.fields.apply(&mut m)?;
            ("tasks.update".into(), Value::Object(m))
        }
        Command::Complete { id } => ("tasks.complete".into(), json!({"id":id,"completed":true})),
        Command::Reopen { id } => ("tasks.complete".into(), json!({"id":id,"completed":false})),
        Command::Delete { id } => ("tasks.delete".into(), json!({"id":id})),
        Command::Restore { id } => ("tasks.restore".into(), json!({"id":id})),
        Command::Reorder { ids } => ("tasks.reorder".into(), json!({"ids":ids})),
        Command::Lists { command } => match command {
            None => {
                projection = Some("lists");
                ("snapshot".into(), json!({}))
            }
            Some(ListCommand::Add {
                name,
                preferences,
                folder,
                color,
            }) => {
                let mut m = Map::new();
                m.insert("name".into(), json!(name));
                put(&mut m, "folder_id", folder);
                put(
                    &mut m,
                    "color",
                    color.map(|v| {
                        if v.eq_ignore_ascii_case("none") {
                            String::new()
                        } else {
                            v
                        }
                    }),
                );
                preferences.apply(&mut m);
                ("lists.create".into(), Value::Object(m))
            }
            Some(ListCommand::Edit {
                id,
                preferences,
                name,
                folder,
                root,
                color,
            }) => {
                let mut m = Map::new();
                m.insert("id".into(), json!(id));
                put(&mut m, "name", name);
                nullable(&mut m, "folder_id", folder, root);
                put(
                    &mut m,
                    "color",
                    color.map(|v| {
                        if v.eq_ignore_ascii_case("none") {
                            String::new()
                        } else {
                            v
                        }
                    }),
                );
                preferences.apply(&mut m);
                ("lists.update".into(), Value::Object(m))
            }
            Some(ListCommand::Duplicate {
                id,
                name,
                folder,
                root,
            }) => {
                let mut m = Map::new();
                m.insert("id".into(), json!(id));
                put(&mut m, "name", name);
                nullable(&mut m, "folder_id", folder, root);
                ("lists.duplicate".into(), Value::Object(m))
            }
            Some(ListCommand::Archive { id, undo }) => {
                ("lists.archive".into(), json!({"id":id,"archived":!undo}))
            }
            Some(ListCommand::Delete { id }) => ("lists.delete".into(), json!({"id":id})),
            Some(ListCommand::Reorder { ids }) => ("lists.reorder".into(), json!({"ids":ids})),
        },
        Command::Folders { command } => match command {
            None => {
                projection = Some("folders");
                ("snapshot".into(), json!({}))
            }
            Some(FolderCommand::Add {
                name,
                parent,
                metadata,
            }) => {
                let mut m = Map::new();
                m.insert("name".into(), json!(name));
                put(&mut m, "parent_id", parent);
                metadata.apply(&mut m);
                ("folders.create".into(), Value::Object(m))
            }
            Some(FolderCommand::Edit {
                id,
                metadata,
                name,
                parent,
                root,
            }) => {
                let mut m = Map::new();
                m.insert("id".into(), json!(id));
                put(&mut m, "name", name);
                nullable(&mut m, "parent_id", parent, root);
                metadata.apply(&mut m);
                ("folders.update".into(), Value::Object(m))
            }
            Some(FolderCommand::Delete { id }) => ("folders.delete".into(), json!({"id":id})),
            Some(FolderCommand::Reorder { ids }) => ("folders.reorder".into(), json!({"ids":ids})),
        },
        Command::Duplicate { id, list_id, title } => {
            let mut m = json!({"id":id});
            if let Some(v) = list_id {
                m["list_id"] = json!(v);
            }
            if let Some(v) = title {
                m["title"] = json!(v);
            }
            ("tasks.duplicate".into(), m)
        }
        Command::WontDo { id, undo } => ("tasks.wont_do".into(), json!({"id":id,"wont_do":!undo})),
        Command::Pin { id, undo } => ("tasks.update".into(), json!({"id":id,"pinned":!undo})),
        Command::Tags { command } => extended::tag(command),
        Command::Sections { command } => match command {
            Some(c) => extended::section(c),
            None => {
                projection = Some("sections");
                ("snapshot".into(), json!({}))
            }
        },
        Command::Templates { command } => match command {
            Some(c) => extended::template(c),
            None => ("templates.list".into(), json!({})),
        },
        Command::Links { command } => extended::link(command),
        Command::Attachments { command } => match command {
            AttachmentCommand::Import { task_id, path } => (
                "attachments.import".into(),
                json!({"task_id":task_id,"path":path}),
            ),
            AttachmentCommand::List { task_id } => {
                projection = Some("attachments");
                ("tasks.get".into(), json!({"id":task_id}))
            }
            AttachmentCommand::Read { id, offset, length } => (
                "attachments.read_chunk".into(),
                json!({"id":id,"offset":offset,"length":length}),
            ),
            AttachmentCommand::Remove { id } => ("attachments.remove".into(), json!({"id":id})),
        },
        Command::Comments { command } => extended::comment(command),
        Command::Settings { command } => match command {
            Some(SettingsCommand::Set { key, value }) => (
                "settings.set".into(),
                json!({"key":key,"value":serde_json::from_str::<Value>(&value)?}),
            ),
            None => ("settings.get".into(), json!({})),
        },
        Command::Snapshot => ("snapshot".into(), json!({})),
        Command::Raw { action, input } => {
            let parsed: Value = if input == "-" {
                let mut bytes = Vec::new();
                io::stdin()
                    .lock()
                    .take(MAX_STDIN_JSON_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_STDIN_JSON_BYTES {
                    return Err("Stdin JSON exceeds the 32 MiB limit".into());
                }
                serde_json::from_slice(&bytes)?
            } else {
                serde_json::from_str(&input)?
            };
            if !parsed.is_object() {
                return Err("Input must be a JSON object".into());
            }
            (action, parsed)
        }
        Command::DbPath => unreachable!(),
    };
    let mut result = if action == "recurrence.preview" {
        tasks_core::preview_recurrence(input)?
    } else {
        TaskService::open(&path)?.execute(&action, input)?
    };
    if let Some(key) = projection {
        result = result.get(key).cloned().unwrap_or(Value::Null);
    }
    let text = if cli.json || force_json {
        serde_json::to_string_pretty(&result)?
    } else {
        human(&result)
    };
    let mut stdout = io::stdout().lock();
    match writeln!(stdout, "{text}") {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn human(v: &Value) -> String {
    if let Some(rows) = v.as_array() {
        if rows.is_empty() {
            return "No tasks or entries.".into();
        }
        return rows.iter().map(human).collect::<Vec<_>>().join("\n");
    }
    if v.get("ok") == Some(&Value::Bool(true)) {
        return "Done.".into();
    }
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
    if v.get("title").is_some() {
        let status = if !v["deleted_at"].is_null() {
            "trash"
        } else if !v["wont_do_at"].is_null() {
            "skip"
        } else if !v["completed_at"].is_null() {
            "done"
        } else {
            "open"
        };
        return format!(
            "{:<36}  [{:<5}]  {}{}",
            text("id"),
            status,
            text("title"),
            if text("due_date").is_empty() {
                String::new()
            } else {
                format!("  ({})", text("due_date"))
            }
        );
    }
    if v.get("name").is_some() {
        return format!("{:<36}  {}", text("id"), text("name"));
    }
    serde_json::to_string_pretty(v).unwrap_or_default()
}
fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tasks: {e}");
            ExitCode::FAILURE
        }
    }
}
