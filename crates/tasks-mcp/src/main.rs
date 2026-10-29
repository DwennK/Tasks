use clap::Parser;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    process::ExitCode,
};
use tasks_core::{default_db_path, TaskService};
mod extended_schema;
mod schema;

const MAX_MESSAGE_BYTES: usize = 1_048_576;
const SUPPORTED_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2024-11-05"];
#[derive(Parser)]
#[command(
    name = "tasks-mcp",
    version,
    about = "Local Tasks MCP server over newline-delimited JSON-RPC on stdio"
)]
struct Args {
    /// SQLite path (defaults to TASKS_DB, then the application's shared database)
    #[arg(long)]
    db: Option<PathBuf>,
}
struct Server {
    service: TaskService,
    initialized: bool,
    ready: bool,
}
fn error(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.into()}})
}
fn result(id: Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":value})
}
impl Server {
    fn handle(&mut self, message: Value) -> Option<Value> {
        if !message.is_object() {
            return Some(error(
                Value::Null,
                -32600,
                "Expected a JSON-RPC object (batch requests are unsupported)",
            ));
        }
        let id = message.get("id").cloned();
        if id
            .as_ref()
            .is_some_and(|id| !(id.is_string() || id.is_i64() || id.is_u64()))
        {
            return Some(error(
                Value::Null,
                -32600,
                "Request id must be a string or integer",
            ));
        }
        if message["jsonrpc"] != "2.0" || !message["method"].is_string() {
            return Some(error(
                id.unwrap_or(Value::Null),
                -32600,
                "Invalid JSON-RPC request",
            ));
        }
        let method = message["method"].as_str().unwrap_or_default();
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        if id.is_none() {
            // Notifications never receive a response. Unknown notifications are safely ignored.
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            return None;
        }
        let id = id.unwrap();
        if !params.is_object() {
            return Some(error(id, -32602, "params must be an object"));
        }
        if method == "initialize" {
            if self.initialized {
                return Some(error(id, -32600, "Already initialized"));
            }
            let Some(version) = params["protocolVersion"].as_str() else {
                return Some(error(id, -32602, "protocolVersion is required"));
            };
            if !params["capabilities"].is_object() || !params["clientInfo"].is_object() {
                return Some(error(
                    id,
                    -32602,
                    "capabilities and clientInfo objects are required",
                ));
            }
            self.initialized = true;
            let version = if SUPPORTED_VERSIONS.contains(&version) {
                version
            } else {
                SUPPORTED_VERSIONS[0]
            };
            return Some(result(
                id,
                json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"tasks-local","version":env!("CARGO_PKG_VERSION")},"instructions":"Manage the same local SQLite tasks as the Tasks macOS app. No network or login. Dates are local calendar dates; pass date to list_tasks for Today/Upcoming. Deletion is recoverable through restore_task. Changes are visible in the GUI automatically."}),
            ));
        }
        if method == "ping" {
            return Some(result(id, json!({})));
        }
        if !self.initialized || !self.ready {
            return Some(error(
                id,
                -32002,
                "Complete initialize and notifications/initialized before using tools",
            ));
        }
        match method {
            "tools/list" => {
                if params.get("cursor").is_some_and(|v| !v.is_null()) {
                    return Some(error(
                        id,
                        -32602,
                        "This server returns all tools in one page; cursor must be omitted",
                    ));
                }
                Some(result(id, json!({"tools":schema::tools()})))
            }
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return Some(error(id, -32602, "Tool name is required"));
                };
                let mut arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let Some(definition) = schema::tools().into_iter().find(|v| v["name"] == name)
                else {
                    return Some(error(id, -32602, format!("Unknown tool: {name}")));
                };
                if let Err(message) = schema::validate(&definition["inputSchema"], &arguments) {
                    return Some(result(
                        id,
                        json!({"content":[{"type":"text","text":message}],"isError":true}),
                    ));
                }
                if name == "add_attachment" {
                    let encoded = arguments["content_base64"].as_str().unwrap_or_default();
                    let padding = encoded
                        .bytes()
                        .rev()
                        .take_while(|b| *b == b'=')
                        .count()
                        .min(2);
                    if (encoded.len() / 4 * 3).saturating_sub(padding) > 524288 {
                        return Some(result(
                            id,
                            json!({"content":[{"type":"text","text":"Attachment exceeds512 KiB decoded; use import_attachment for local files up to20 MiB."}],"isError":true}),
                        ));
                    }
                }
                let (action, projection) = match name {
                    "list_tasks" => {
                        if arguments.get("date").is_none() {
                            arguments["date"] =
                                json!(chrono::Local::now().format("%Y-%m-%d").to_string());
                        }
                        ("tasks.list", None)
                    }
                    "get_task" => ("tasks.get", None),
                    "create_task" => ("tasks.create", None),
                    "update_task" => ("tasks.update", None),
                    "complete_task" => ("tasks.complete", None),
                    "delete_task" => ("tasks.delete", None),
                    "restore_task" => ("tasks.restore", None),
                    "reorder_tasks" => ("tasks.reorder", None),
                    "list_lists" => ("snapshot", Some("lists")),
                    "list_folders" => ("snapshot", Some("folders")),
                    "create_list" => ("lists.create", None),
                    "update_list" => ("lists.update", None),
                    "delete_list" => ("lists.delete", None),
                    "create_folder" => ("folders.create", None),
                    "update_folder" => ("folders.update", None),
                    "delete_folder" => ("folders.delete", None),
                    _ => match extended_schema::action(name) {
                        Some(route) => route,
                        None => return Some(error(id, -32602, "Unknown tool")),
                    },
                };
                match self.service.execute(action, arguments) {
                    Ok(mut data) => {
                        if let Some(key) = projection {
                            data = data.get(key).cloned().unwrap_or(Value::Null);
                        }
                        let text = serde_json::to_string(&data).unwrap_or_default();
                        // Structured content must be an object, including array results.
                        Some(result(
                            id,
                            json!({"content":[{"type":"text","text":text}],"structuredContent":{"data":data},"isError":false}),
                        ))
                    }
                    Err(e) => Some(result(
                        id,
                        json!({"content":[{"type":"text","text":e.to_string()}],"isError":true}),
                    )),
                }
            }
            _ => Some(error(id, -32601, format!("Method not found: {method}"))),
        }
    }
}
// Read through an oversized message without allocating more than the bounded buffer.
fn read_message(reader: &mut impl BufRead, buffer: &mut Vec<u8>) -> io::Result<Option<bool>> {
    buffer.clear();
    let mut oversized = false;
    let mut any = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if any { Ok(Some(oversized)) } else { Ok(None) };
        }
        any = true;
        let end = available.iter().position(|b| *b == b'\n');
        let consumed = end.map(|p| p + 1).unwrap_or(available.len());
        if !oversized {
            if buffer.len() + consumed > MAX_MESSAGE_BYTES {
                oversized = true;
                buffer.clear();
            } else {
                buffer.extend_from_slice(&available[..consumed]);
            }
        }
        reader.consume(consumed);
        if end.is_some() {
            return Ok(Some(oversized));
        }
    }
}
fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let service = TaskService::open(args.db.unwrap_or_else(default_db_path))?;
    let mut server = Server {
        service,
        initialized: false,
        ready: false,
    };
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let mut buffer = Vec::with_capacity(4096);
    while let Some(oversized) = read_message(&mut reader, &mut buffer)? {
        let response = if oversized {
            Some(error(Value::Null, -32700, "Message exceeds 1 MiB limit"))
        } else if buffer.iter().all(u8::is_ascii_whitespace) {
            continue;
        } else {
            match serde_json::from_slice::<Value>(&buffer) {
                Ok(message) => server.handle(message),
                Err(_) => Some(error(
                    Value::Null,
                    -32700,
                    "Parse error: expected valid UTF-8 JSON",
                )),
            }
        };
        if let Some(response) = response {
            if let Err(e) = writeln!(writer, "{response}").and_then(|_| writer.flush()) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(e.into());
            }
        }
    }
    Ok(())
}
fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tasks-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}
