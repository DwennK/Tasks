# CLI and MCP

Both binaries call the same `tasks-core` transaction service as the Tauri desktop app. They do not run a web server, access the network, require a login, or use a separate copy of your tasks.

## Build

```sh
cargo build --release -p tasks-cli -p tasks-mcp
```

The outputs are `target/release/tasks` and `target/release/tasks-mcp`.

On macOS the default SQLite file is `~/Library/Application Support/com.dwenn.tasks/tasks.sqlite3`. `TASKS_DB` overrides it; an explicit `--db /absolute/path/tasks.sqlite3` takes precedence. Database-backed commands create parent directories and the database on first use. The pure CLI `recurrence.preview` action is an exception: the invocation does not open or create SQLite, even when `--db` or `TASKS_DB` names a missing path. Point every entry point at the same file when you want shared data. Use another path for tests or a separate workspace.

## CLI

```sh
# Inspect all commands and options
target/release/tasks --help
target/release/tasks edit --help

# Create and find tasks
target/release/tasks add "Review release" --due 2026-10-28 --priority 3
target/release/tasks list --view today
target/release/tasks list --view upcoming --date 2026-10-27
target/release/tasks list --search "release" --include-subtasks

# IDs come from the preceding command output
target/release/tasks edit TASK_ID --notes "Check desktop and keyboard flows"
target/release/tasks add "Check keyboard flow" --parent TASK_ID
target/release/tasks edit TASK_ID --due 2026-10-29 --time 09:30
target/release/tasks edit TASK_ID --reminder 2026-10-29T09:00:00+01:00
target/release/tasks edit TASK_ID --clear-due --clear-reminder
target/release/tasks complete TASK_ID
target/release/tasks reopen TASK_ID
target/release/tasks delete TASK_ID
target/release/tasks list --view trash
target/release/tasks restore TASK_ID

# Reorder sibling tasks (same parent and list)
target/release/tasks reorder TASK_ID_2 TASK_ID_1

# Nested folders and lists
target/release/tasks folders add Work
target/release/tasks folders add Clients --parent FOLDER_ID
target/release/tasks lists add Release --folder FOLDER_ID
target/release/tasks lists
target/release/tasks folders
target/release/tasks edit TASK_ID --list LIST_ID

# JSON and advanced actions
target/release/tasks --json get TASK_ID
target/release/tasks --json call tasks.update '{"id":"TASK_ID","notes":"Ready","expected_version":2}'
target/release/tasks --json snapshot
target/release/tasks db-path
```

`raw` and `call` are aliases for the same advanced service command. Pass `-` as the JSON argument to read a single JSON object from stdin until EOF, with a strict 32 MiB byte limit. This avoids macOS command-line argument limits for attachment payloads and long notes; malformed or oversized input exits nonzero without executing the requested service action. Existing inline JSON arguments continue to work.

```sh
target/release/tasks --json call tasks.create - < task.json
target/release/tasks --json raw attachments.add - < attachment.json
```

The CLI stdin limit is separate from MCP's 1 MiB frame limit and SQLite's 20 MiB per-attachment limit. The action and JSON schema are documented in `CONTRACT.md`. JSON results go to stdout, diagnostics to stderr; errors exit nonzero. Titles and notes containing whitespace must be quoted according to your shell. `--json` can appear before or after the subcommand.

Dates are local `YYYY-MM-DD` values, due times are `HH:MM`, and reminders are absolute RFC3339 timestamps. Today includes overdue tasks. Upcoming includes today and the next six days. Omitted edit fields remain unchanged; explicit clear flags remove nullable values. Setting `expected_version` rejects stale writes rather than overwriting another writer's edit. `tags set --expected-version N` guards against the task version; `comments edit/delete --expected-version N` guards against the comment’s own version returned by `comments list`. MCP `set_task_tags`, `update_comment` and `delete_comment` expose the same optional guard. A conflict leaves the current data intact; reread it before deciding how to retry. The desktop row and detail title editors share a guarded draft: external CLI/MCP edits can produce a visible conflict, and Restore retains that draft until an explicit Retry instead of silently publishing stale title text.

Deleting a task moves its subtree to Trash. Deleting a list preserves tasks by moving them to Inbox; deleting a folder lifts its child folders/lists to its parent. There is no permanent task-purge command. Removing an attachment permanently removes that attachment’s stored bytes.

## Advanced task fields and commands

Task creation/editing supports task/note conversion (`--is-note true/false`), duration, start date/time, IANA timezone, recurrence, multiple reminders, tags, Markdown/HTML/plain notes, checklist items, pinning, and list sections. `--fields` accepts additional editable fields as JSON, including `null` to clear nullable values; explicit named options win. It cannot replace the positional task ID.

```sh
target/release/tasks add "Daily review" --due 2026-10-28 --time 09:00 --timezone Europe/Zurich --duration 30 --repeat '{"frequency":"daily","interval":1,"count":5}'
target/release/tasks edit TASK_ID --reminders '2026-10-28T07:30:00Z,2026-10-28T07:55:00Z' --notes-format markdown
target/release/tasks edit TASK_ID --fields '{"recurrence":null,"duration_minutes":null}'
target/release/tasks edit TASK_ID --checklist '[{"id":"item-1","text":"Check build","completed":false}]'
target/release/tasks pin TASK_ID
target/release/tasks duplicate TASK_ID --title "Another review"
target/release/tasks wont-do TASK_ID
target/release/tasks wont-do TASK_ID --undo
target/release/tasks list --view wont-do

target/release/tasks tags add important
target/release/tasks tags set TASK_ID TAG_ID
target/release/tasks links add TASK_ID RELATED_TASK_ID
target/release/tasks folders add Work --icon "briefcase" --pinned true
target/release/tasks folders edit FOLDER_ID --clear-icon --pinned false
target/release/tasks lists add Notes --list-type note --icon "notebook" --pinned true --color None --show-in-smart-lists false
target/release/tasks lists edit LIST_ID --view-mode board --group-by section --sort-by priority
target/release/tasks lists duplicate LIST_ID --name "Notes copy" --root
target/release/tasks edit TASK_ID --is-note true
target/release/tasks lists archive LIST_ID
target/release/tasks lists archive LIST_ID --undo
target/release/tasks sections add LIST_ID Review
target/release/tasks edit TASK_ID --section SECTION_ID
target/release/tasks templates add "Review template" TASK_ID
target/release/tasks templates apply TEMPLATE_ID --list inbox
target/release/tasks templates get TEMPLATE_ID
target/release/tasks comments add TASK_ID "Reviewed the latest build"
target/release/tasks comments list TASK_ID
target/release/tasks settings
target/release/tasks settings set theme_color '"teal"'

# Copy an explicit local file into SQLite (up to 20 MiB)
target/release/tasks attachments import TASK_ID /absolute/path/brief.pdf
target/release/tasks attachments list TASK_ID
target/release/tasks attachments read ATTACHMENT_ID --offset 0 --length 262144
```

New items inherit their list’s task/note type unless `is_note` is supplied. Converting an item preserves its dates, notes, reminders and relationships. Lists and folders support an icon and sidebar pin; lists additionally control smart-view visibility. `--color None` or `--color ''` clears a list color; tags still require a hexadecimal color. `--clear-icon` removes a list/folder icon.

List duplication preserves item completion/checklist states and copies nontrashed items, sections, tags and attachments with new IDs; internal relationships are remapped. Its copy starts unpinned and unarchived. Task duplication retains its separate behavior of resetting completion/checklist state.

Search matches rendered note text across plain, Markdown and HTML notes, including decoded HTML entities and normalized whitespace. Responses include the read-only `notes_text` projection; write the original `notes` and `notes_format` fields instead. Hidden smart-view lists remain reachable through an explicit list filter or global search.

Completing a recurring task creates at most one next occurrence atomically. Repeating completion does not create extra copies. Recurrence supports daily/weekly/monthly/yearly intervals, selected ISO weekdays, count/end-date limits and due-date or completion-date bases. Count includes the current occurrence; only generated occurrences consume it, and the end date is inclusive. The service preserves local times across daylight saving changes.

### Calendar recurrence JSON

The following fields are optional. Newly normalized rules return `month_days: []`, `ordinal_weekday: null`, `workday: null`, `year_month: null` and `skip_weekends: false` when unused. Older persisted rules can omit those fields; clients should use those same defaults. No schema migration is required for this extension.

| Field | Meaning and limits |
| --- | --- |
| `month_days` | Monthly selected calendar dates 1–31, or `-1` for Last Day. Values are sorted/deduplicated; at most 32 input values. Yearly rules accept one date 1–31. |
| `ordinal_weekday` | `{ "ordinal": 1..5 or -1, "weekday": 1..7 }`; ISO Monday=1 through Sunday=7, `-1` means Last. Monthly or yearly. |
| `workday` | `"first"` or `"last"` Monday–Friday of a month; regional holidays are not inferred. Monthly only. |
| `year_month` | Annual month 1–12, paired with a yearly calendar-date or ordinal-weekday pattern. |
| `skip_weekends` | Monthly calendar-date occurrences on Saturday/Sunday are omitted, not moved to Friday/Monday. It cannot combine with `ordinal_weekday` or `workday`. |

Use only one of `month_days`, `ordinal_weekday` or `workday`. A monthly date set can produce several occurrences in one eligible month before advancing by the month interval. Annual rules advance by the year interval. Completion-date mode reanchors the eligible calendar periods to the actual completion date.

**Explicit dates skip short months; legacy rules clamp.** For example, an explicit monthly `month_days: [31]` sequence from 31 January skips February and next occurs on 31 March. A legacy monthly rule with no custom pattern keeps its existing month-end behavior: 31 January → 28/29 February → 31 March. `month_days: [-1]` explicitly requests the last day of each month. A missing fifth weekday skips its period. Explicit 29 February skips non-leap years; impossible fixed month/day combinations, such as 31 April, are rejected.

New due-date configurations also reject patterns that can never produce a date in the calendar phases reachable by their interval. For example, 29 February every four years anchored to 2025 never reaches a leap year. This check is independent of count/end exhaustion. Previously stored impossible schedules remain editable and completable: they end without a successor instead of preventing completion. Completion-based rules are checked against the actual completion calendar and also end cleanly when no successor exists.

```sh
# Every selected monthly date; February has no 31st occurrence
target/release/tasks add "Month close" --due 2026-01-31 --repeat '{"frequency":"monthly","month_days":[31]}'
# First and fifteenth, omitting weekend occurrences
target/release/tasks add "Review" --due 2026-01-01 --repeat '{"frequency":"monthly","month_days":[1,15],"skip_weekends":true}'
# Last Monday of each month
target/release/tasks edit TASK_ID --repeat '{"frequency":"monthly","ordinal_weekday":{"ordinal":-1,"weekday":1}}'
# Last workday (Monday–Friday; no holiday calendar)
target/release/tasks edit TASK_ID --repeat '{"frequency":"monthly","workday":"last"}'
```

The CLI passes these fields through `--repeat` JSON. MCP `create_task` and `update_task` expose the same recurrence object and validation.

### Preview future occurrences without saving

`recurrence.preview` calculates dates for a draft rule using the same successor engine as task completion. It does not open or create SQLite, migrate a database, save a task or change its revision. The CLI handles this action before constructing its database service. MCP exposes the same calculation as `preview_recurrence`. That tool call performs no database reads or writes, but the MCP server process opens the database during startup. Only the pure CLI preview path guarantees that the entire invocation creates no database or parent directory.

```sh
target/release/tasks --json call recurrence.preview '{"due_date":"2026-01-31","recurrence":{"frequency":"monthly","interval":1},"range_start":"2026-02-01","range_end":"2026-04-30","limit":12}'
```

The required inputs are `due_date`, `recurrence` (which may be null), `range_start` and `range_end`; dates use `YYYY-MM-DD`. Optional `timezone` must be an IANA name. The inclusive visible range is at most 366 days. `limit` defaults to 128 and accepts 1–512.

The result has `dates`, `truncated` and `assumption`. It contains future occurrences only, excluding the initial due date. Empty, nonrecurring or exhausted schedules return an empty list. `assumption` is `due_date`, or `completion_on_due_date` for completion-based rules: future completion dates cannot be known, so that preview assumes each occurrence completes on its due date. This is a projection, not a persisted schedule.

The preview honors count/end limits, explicit calendar patterns and the original `anchor_date`, using the same semantics as completion. A preview of an existing rule should preserve its returned anchor; omitting it uses the supplied due date as a new anchor. Enumeration stops after 10,000 successor steps; `truncated: true` also indicates more visible dates than the requested limit. Clients should display the bound rather than presenting a truncated result as complete.

Inline JSON and stdin (`call recurrence.preview -`) both work. The release smoke check verifies successful and invalid preview requests without creating the requested database or its parent directory.

## Connect an MCP client

Use an absolute executable path in your client's stdio MCP configuration. Replace the example path with your checkout:

```json
{
  "mcpServers": {
    "tasks": {
      "command": "/absolute/path/Tasks/target/release/tasks-mcp",
      "args": []
    }
  }
}
```

Optional separate database:

```json
{
  "mcpServers": {
    "tasks-test": {
      "command": "/absolute/path/tasks-mcp",
      "args": ["--db", "/absolute/path/test-tasks.sqlite3"]
    }
  }
}
```

The client owns the process lifetime. Closing stdin stops the server cleanly. The server uses newline-delimited UTF-8 JSON-RPC, flushes one response per line, and reserves stdout for protocol output. Requests are limited to 1 MiB per line; malformed/oversized messages return protocol errors without corrupting later frames. No ports are opened.

Supported negotiated protocol revisions: `2025-11-25`, `2025-06-18`, `2024-11-05`. A client requesting an unsupported revision receives `2025-11-25` and may disconnect if incompatible. Initialize first, then send `notifications/initialized`; `ping` works before initialization. There are no resources, prompts, subscriptions, or background protocol jobs.

Tools (51 total):

| Tool | Behavior |
| --- | --- |
| `list_tasks` | Views, list filter, search, date scope, parent filter, optional subtasks |
| `get_task` | Full persisted task, including completed/trashed tasks |
| `preview_recurrence` | Pure calculation of future draft dates in a bounded range; no database reads/writes during the call, with an explicit completion-date assumption |
| `create_task` | Create task or subtask |
| `update_task` | Partial edits and optional optimistic version check |
| `complete_task` | Complete subtree; `completed: false` reopens |
| `delete_task` | Move subtree to recoverable Trash |
| `restore_task` | Restore subtree and required ancestors |
| `reorder_tasks` | Reorder sibling task IDs transactionally |
| `list_lists`, `list_folders` | Discover navigation hierarchy and IDs |
| `create_list`, `update_list`, `delete_list` | Manage lists while preserving tasks |
| `create_folder`, `update_folder`, `delete_folder` | Manage nested folders |
| `duplicate_task`, `dismiss_task` | Clone tasks or mark Won’t Do; dismiss supports undo |
| `set_task_tags`, `list_tags`, `create_tag`, `update_tag`, `delete_tag` | Manage tags and assignments |
| `link_tasks`, `unlink_tasks` | Bidirectional related-task links |
| `add_attachment`, `import_attachment`, `read_attachment_chunk`, `remove_attachment` | Local BLOB attachments with bounded MCP payloads |
| `archive_list`, `duplicate_list`, `reorder_lists`, `reorder_folders` | Archive, duplicate and order navigation |
| `list_sections`, `create_section`, `update_section`, `delete_section`, `reorder_sections` | List sections |
| `list_templates`, `create_template`, `update_template`, `delete_template`, `apply_template` | Reusable tasks and subtrees |
| `get_template` | Read a template preview |
| `list_comments`, `add_comment`, `update_comment`, `delete_comment` | Local task comments and attachment associations |
| `get_settings`, `set_setting` | Persisted desktop/task preferences, including saved filters |

`create_task` and `update_task` expose all editable advanced fields. List tools accept `view_mode`, `group_by`, `sort_by`, `sort_order`, `icon`, `pinned`, `show_in_smart_lists` and `list_type`; folder tools accept `icon` and `pinned`. Empty string clears list color; null or empty string clears icons. Use `get_task` to discover attachment metadata and related task IDs.

Attachment transport deliberately stays bounded: `add_attachment` allows at most 512 KiB decoded (699,052 base64 characters), so its request fits the 1 MiB frame limit. Use `import_attachment` for files up to 20 MiB: it reads only the explicitly supplied local path. `read_attachment_chunk` returns at most 256 KiB decoded per call; pass the returned `next_offset` until it is null. Offsets count raw bytes, not base64 characters. There is no unbounded whole-file attachment-read MCP tool. The shared service retains its full 20 MiB storage limit for desktop and local file imports.

Tool schemas reject unknown fields and wrong types. Domain/validation failures are returned as tool results with `isError: true`, so clients can correct inputs. Unknown methods/tools and invalid JSON-RPC envelopes use JSON-RPC errors. Each successful call includes serialized JSON in text content and a structured `{ "data": ... }` object. Read-only and mutation tools advertise their corresponding annotations; all tools declare `openWorldHint: false`.

This is local database access with the permissions of the launched process. Only configure the server in clients you intend to allow to manage your tasks. The MCP client remains responsible for presenting its approval controls.

Notifications are produced by the running desktop app, not by the CLI/MCP subprocess. Creating a reminder through either interface persists it for the desktop scheduler.

`launch_at_login`, `launch_minimized`, `global_quick_add_shortcut` and `global_show_hide_shortcut` are shared settings. Defaults are false, false, empty and empty (disabled). CLI/MCP changes persist preferences; the running desktop applies native effects and reports registration failures. A global shortcut requires Command/Control/Option, for example `ctrl+shift+a`; sequences such as `g t` are invalid globally. Set an empty string to unregister either global action. The two actions cannot share a chord; duplicate/invalid changes are rejected and native registration failures preserve the previous valid binding/settings. Launch at Login is managed only by the packaged release app using its default database; development and `TASKS_DB`-isolated runs never modify the production login item.

The `hotkeys` object also supports `toggle_all_groups` and `toggle_all_subtasks`, both unassigned by default. Return Add task below and Shift+Return Add subtask are task-list keyboard actions. macOS recording normalizes Option-produced characters to their physical letter key and retains both Command and Control when pressed. Native registration and persistence were observed; actual global callback delivery remains unverified.

`show_list_color` defaults to `false` for new databases. Setting it to `true` explicitly enables task-row color accents and remains preserved across process restarts. No migration resets an existing saved choice. Sidebar list-icon colors are independent.

Protocol references: [stdio transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports), [lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle), [tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).

## Verification

```sh
cargo test -p tasks-cli -p tasks-mcp
```

The tests start real executable processes against isolated temporary SQLite databases. They cover persistent CRUD, list preservation, malformed input, protocol initialization, discovery, recovery after oversized frames, and clean shutdown. Core transaction and concurrency tests live in `tasks-core`.

The retained 133-test workspace baseline includes 10 CLI and 16 MCP tests. All 30 Rust source files remain unchanged in the installed inline-title/window-drag release; this lot adds no CLI command or MCP tool. Both global shortcut settings, duplicate-chord rejection, clearing and persistence remain covered, and the inventory is 51 MCP tools. Evidence: `output/inline-title-lot/rust-source-recheck.json`, referencing `output/settings-shortcuts-lot/rust-20261028T002624Z/receipt.json`. Earlier border/default and comprehensive CLI/MCP smoke receipts remain historical binary-specific evidence. The MCP server opens SQLite at startup even when a later tool call is a pure preview; only the pure CLI preview invocation guarantees no database creation.
