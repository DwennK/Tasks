# Development and usage guide

A local-first macOS task manager built with Tauri 2, Rust, Vue 3, TypeScript, Tailwind CSS and SQLite. The interface combines a navigation rail, nested lists, a task list and a detail editor.

## Run and build

Requirements: macOS 12+, Xcode command line tools, stable Rust, Node.js and pnpm 12.15.0.

```sh
pnpm install --frozen-lockfile
pnpm desktop          # Native app with development frontend
pnpm bundle           # Release .app in target/release/bundle/macos/Tasks.app
cargo build --release -p tasks-cli -p tasks-mcp
```

`pnpm dev` starts only the frontend; production data access requires Tauri. Browser verification uses an explicit, isolated SQLite test database:

```sh
cargo build -p tasks-cli
TASKS_DEV_DB="$PWD/output/qa.sqlite3" pnpm dev
```

The browser bridge exists only in the Vite development server. It listens on loopback, requires JSON same-origin requests, and is absent from the built app. OAuth sign-in, when explicitly started from Cloud Sync settings, temporarily listens on 127.0.0.1:49764 for the provider callback; no task API is exposed on that listener.

## Everyday use

- Inbox, Today (including overdue), Tomorrow, Next 7 Days, All, Completed, Won't Do and recoverable Trash, with configurable sidebar visibility.
- Nested folders and lists; expandable subtasks; due dates, start/end times, time zones, multiple reminders and priorities. Custom recurrence supports selected weekdays, multiple month days, ordinal weekdays, first/last workdays, yearly month rules and end dates or occurrence counts. The date picker previews future occurrences from the same Rust engine, including unsaved changes, without saving the task or changing its database revision.
- Rich-text notes with headings, interactive checklists, lists, links, code and a slash menu. Existing plain-text and Markdown notes remain readable and can be edited in the rich editor.
- Tags, saved filters, sections, list/board/Timeline views, local attachments, linked tasks, comments and reusable task templates.
- Drag tasks before or after sibling rows, or move them between lists and sections. Sidebar moves between folders preserve the selected before/after position. Context menus provide task and list actions; sidebar and task panes can be resized.
- Shift-click, modifier-click and keyboard navigation include expanded subtasks. Bulk completion, duplication and moves process a selected subtree once; bulk Date and Tags also update explicitly selected children. Tag changes preserve each task’s other tags.
- With a task selected in the list, Return opens an inline Add task below row and Shift+Return adds a subtask. Group/subtask expansion shortcuts are configurable. Press ⌘N for compact Quick Add with date, priority and list controls, ⌘K for the command menu, ⌘F to search, ⌘D to schedule the selection and ⌘P to print the selected task. V then L/K switches list/board view. Escape dismisses an editor or dialog. Settings includes editable local shortcuts; actions without a verified default start unassigned.
- Export a task as Markdown/plain text, or open the native macOS print panel from its actions menu or Task → Print.
- Overdue task details show the elapsed days in red. Postpone moves only the displayed overdue group to today.
- Click a desktop row title to edit it directly; the row and detail editor share one draft. Escape leaves the task selected. Mobile uses the detail editor. Changes save automatically, and failed/conflicting saves retain the draft for an explicit retry.
- Quick Add waits for the initial saved settings before applying defaults; repeated ⌘N presses preserve its open draft. The top-right toolbar and the empty detail pane’s top 57 px provide window drag regions, excluding interactive controls.
- A separate native Settings window controls smart lists, reminders, week/time display, appearance, task defaults, text recognition and shortcuts. Smart Recognition has separate date-recognition and text/tag-removal controls. Show(Hide Note) changes sidebar counts without hiding notes. List Color hides task-row color markers by default for new databases; Show enables them explicitly, and saved choices are preserved. Sidebar icon colors remain independent.
- Launch at Login, minimized automatic launch, global Quick Add and Show/Hide Main Window bindings are optional macOS settings, disabled by default. Registration errors are shown without replacing the prior shortcut or saving the rejected value. Development and custom-database sessions never manage the production login item.

- Task Activities records changes after this version is installed. Earlier title/description versions can be restored without reverting dates or other fields. Task progress is manual or calculated from actual check items.
- Summary filters tasks by dates, lists, tags, status and priority, with saved templates, editable reports and Markdown/text export.
- Paste multiple lines into Add Task or Quick Add to review them before creating tasks. English and French date/time and recurrence phrases respect the recognition preferences.

## Data and reliability

The app starts empty except for Inbox and works locally without a login. Optional personal synchronization connects directly to Google Drive or Dropbox; there is no Tasks account server or telemetry. Provider connections are disabled until configured and authorized. See [Cloud sync setup](cloud-sync.md) for publisher OAuth registration, connection steps and current limits. Shared-list collaboration, assignment and mentions are not part of personal file synchronization.

The default database is `~/Library/Application Support/com.dwenn.tasks/tasks.sqlite3`. All three entry points use this file. `TASKS_DB` overrides the path; CLI/MCP also accept `--db`. Never point QA at the real database.

`crates/tasks-core` owns validation, numbered migrations and business logic. GUI commands run on a blocking worker, while CLI and MCP call the same service directly. WAL allows concurrent readers; SQLite IMMEDIATE transactions serialize writes, foreign keys preserve references, FULL synchronization protects committed changes, and a busy timeout handles contention. Task versions reject stale editor saves. A persistent revision lets the UI pick up CLI/MCP writes without reloading unchanged task collections.

Date views and counts refresh when the local day changes, including after the app becomes active again, without reloading an unchanged database snapshot.

Attachments are stored as SQLite BLOBs alongside their metadata, so a consistent database backup includes their bytes. Snapshots contain metadata only. A reminder generation token prevents an old delivery acknowledgement from suppressing a newly scheduled reminder at the same time.

Quitting requests a flush from every open webview, including Settings, and waits for pending database/file writes, multi-step task actions and native integration-setting changes. In-flight cloud operations observe the quit request before applying another local or remote step. Failed saves cancel the quit and retain the draft.

Deleting a task moves its subtree to Trash. Deleting a list moves its tasks to Inbox. Deleting a folder lifts its children to the parent. No permanent task-purge action is exposed.

For a consistent backup while the app is running, use SQLite's backup API, for example:

```sh
sqlite3 "$HOME/Library/Application Support/com.dwenn.tasks/tasks.sqlite3" ".backup '/absolute/path/tasks-backup.sqlite3'"
```

Do not copy only the main SQLite file while writers are active; WAL sidecars may contain recent transactions. To restore, quit Tasks and stop CLI/MCP clients, retain a backup of the existing database and sidecars, then replace the database with the backup.

## Notifications

Enable notifications in Settings and select one or more reminders in the task’s date picker. The running desktop app checks due reminders every 15 seconds and records submitted reminders in SQLite. Closing the window keeps the app running; click the Dock icon to reopen. ⌘Q quits the app and stops its reminder scheduler. Missed reminders are checked on the next launch. macOS notification settings and Focus may suppress banners. A queued OS notification is not proof that it was seen. A separate reminder window offers Done, Dismiss and Snooze. Its pending reminders persist across restarts, and stale controls cannot change a rescheduled, deleted or completed task.

## CLI and MCP

See [CLI and MCP documentation](cli-mcp.md) for commands, tool schemas, and a ready-to-use stdio configuration. Neither interface opens a port.

## Validation

```sh
pnpm check
pnpm build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p tasks-cli
PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 pnpm test:ui
```

UI tests use the installed Google Chrome with an isolated temporary profile and an isolated database. No browser is downloaded. Screenshots and test reports are written under `output/`. Keep source and documentation files unchanged during a run: Tailwind/Vite can reload the document when those files change. Native notification delivery, physical window dragging and real cloud-provider transfers need separate macOS verification. Source changes or a successful build alone do not establish that an installed app has been updated.

## Source map

- `src/`: Vue interface and native IPC adapter.
- `src-tauri/`: native window, single-instance handling and notification scheduler.
- `crates/tasks-core/`: SQLite storage and shared domain service.
- `crates/tasks-cli/`: command-line client.
- `crates/tasks-mcp/`: bounded newline-delimited JSON-RPC stdio server.
- `CONTRACT.md`: transport-neutral actions and data shape.

This is a local development build. Distribution outside this Mac requires your Apple signing identity and notarization. Tasks is an independent project inspired by familiar task-manager workflows; it is not affiliated with TickTick and does not import TickTick account data.
