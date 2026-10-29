import type { Task, TaskList, Tag } from "../types";

export type SummaryStatus = "active" | "completed" | "wont_do";
export type SummaryField =
  "list" | "due" | "completed" | "priority" | "tags" | "progress" | "notes";
export interface SummaryOptions {
  range: "this_week" | "last_week" | "this_month" | "custom";
  from: string;
  to: string;
  basis: "completed" | "due" | "created";
  list_ids: string[];
  tag_ids: string[];
  statuses: SummaryStatus[];
  priorities: number[];
  group_by: "none" | "list" | "status";
  sort_by: "date" | "title" | "priority";
  include_subtasks: boolean;
  fields: SummaryField[];
}
export interface SummaryPreset {
  id: string;
  name: string;
  options: SummaryOptions;
}
export interface SummaryEntry {
  task: Task;
  date: string;
  parentPath: string;
}
export interface SummaryGroup {
  id: string;
  label: string;
  entries: SummaryEntry[];
}
export const summaryStatusLabels: Record<SummaryStatus, string> = {
  active: "Active",
  completed: "Completed",
  wont_do: "Won’t Do",
};
const day = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;

/** Date-only values never pass through a UTC parser. Timestamp dates use the local calendar. */
export function summaryLocalDate(value: string | null): string {
  if (!value) return "";
  if (/^\d{4}-\d{2}-\d{2}$/.test(value)) return value;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "" : day(date);
}
export function summaryRange(
  range: SummaryOptions["range"],
  today = new Date(),
  weekStart = 1,
): { from: string; to: string } {
  const from = new Date(
    today.getFullYear(),
    today.getMonth(),
    today.getDate(),
    12,
  );
  const to = new Date(from);
  if (range === "this_week" || range === "last_week") {
    from.setDate(
      from.getDate() -
        ((from.getDay() - weekStart + 7) % 7) -
        (range === "last_week" ? 7 : 0),
    );
    to.setTime(from.getTime());
    to.setDate(to.getDate() + 6);
  } else if (range === "this_month") {
    from.setDate(1);
    to.setMonth(to.getMonth() + 1, 0);
  }
  return { from: day(from), to: day(to) };
}
export function defaultSummaryOptions(
  today = new Date(),
  weekStart = 1,
): SummaryOptions {
  return {
    range: "this_week",
    ...summaryRange("this_week", today, weekStart),
    basis: "completed",
    list_ids: [],
    tag_ids: [],
    statuses: ["completed"],
    priorities: [],
    group_by: "list",
    sort_by: "date",
    include_subtasks: true,
    fields: ["completed"],
  };
}
export function summaryStatus(task: Task): SummaryStatus {
  return task.wont_do_at
    ? "wont_do"
    : task.completed_at
      ? "completed"
      : "active";
}
export function buildSummary(
  tasks: Task[],
  lists: TaskList[],
  options: SummaryOptions,
): SummaryGroup[] {
  if (!options.from || !options.to || options.from > options.to) return [];
  const byId = new Map(tasks.map((task) => [task.id, task]));
  const listNames = new Map(lists.map((list) => [list.id, list.name]));
  const unique = new Map<string, SummaryEntry>();
  for (const task of tasks) {
    if (
      task.deleted_at ||
      task.is_note ||
      (!options.include_subtasks && task.parent_id)
    )
      continue;
    if (options.list_ids.length && !options.list_ids.includes(task.list_id))
      continue;
    if (
      options.tag_ids.length &&
      !options.tag_ids.some((id) => task.tag_ids.includes(id))
    )
      continue;
    if (
      options.statuses.length &&
      !options.statuses.includes(summaryStatus(task))
    )
      continue;
    if (
      options.priorities.length &&
      !options.priorities.includes(task.priority)
    )
      continue;
    const date = summaryLocalDate(
      options.basis === "completed"
        ? task.completed_at
        : options.basis === "due"
          ? task.due_date
          : task.created_at,
    );
    if (!date || date < options.from || date > options.to) continue;
    const parents: string[] = [];
    const seen = new Set([task.id]);
    let parentId = task.parent_id;
    while (parentId && !seen.has(parentId)) {
      seen.add(parentId);
      const parent = byId.get(parentId);
      if (!parent || parent.deleted_at) break;
      parents.unshift(parent.title);
      parentId = parent.parent_id;
    }
    unique.set(task.id, { task, date, parentPath: parents.join(" › ") });
  }
  const entries = [...unique.values()].sort((a, b) => {
    if (options.sort_by === "title")
      return (
        a.task.title.localeCompare(b.task.title) ||
        a.task.id.localeCompare(b.task.id)
      );
    if (options.sort_by === "priority" && a.task.priority !== b.task.priority)
      return b.task.priority - a.task.priority;
    return (
      a.date.localeCompare(b.date) ||
      a.task.position - b.task.position ||
      a.task.id.localeCompare(b.task.id)
    );
  });
  const groups = new Map<string, SummaryGroup>();
  for (const entry of entries) {
    const id =
      options.group_by === "list"
        ? entry.task.list_id
        : options.group_by === "status"
          ? summaryStatus(entry.task)
          : "all";
    if (!groups.has(id))
      groups.set(id, {
        id,
        label:
          options.group_by === "list"
            ? listNames.get(id) || "Unknown list"
            : options.group_by === "status"
              ? summaryStatusLabels[id as SummaryStatus]
              : "Tasks",
        entries: [],
      });
    groups.get(id)!.entries.push(entry);
  }
  if (options.group_by === "status")
    return ["active", "completed", "wont_do"].flatMap((id) =>
      groups.has(id) ? [groups.get(id)!] : [],
    );
  return [...groups.values()];
}
const singleLine = (text: string) => text.replace(/[\r\n]+/g, " ").trim();
const escapeMarkdown = (text: string) =>
  singleLine(text).replace(/[\\`*_{}\[\]<>#|]/g, "\\$&");
export function summaryMarkdown(
  groups: SummaryGroup[],
  lists: TaskList[],
  tags: Tag[],
  options: SummaryOptions,
): string {
  const listNames = new Map(lists.map((list) => [list.id, list.name]));
  const tagNames = new Map(tags.map((tag) => [tag.id, tag.name]));
  const count = groups.reduce(
    (total, group) => total + group.entries.length,
    0,
  );
  const lines = [
    `# Task Summary`,
    "",
    `${options.from} – ${options.to}`,
    `${count} ${count === 1 ? "task" : "tasks"}${options.include_subtasks ? " (including subtasks)" : " (top-level only)"}`,
    "",
  ];
  for (const group of groups) {
    if (options.group_by !== "none")
      lines.push(`## ${escapeMarkdown(group.label)}`, "");
    for (const { task, parentPath } of group.entries) {
      const status = summaryStatus(task);
      lines.push(
        `- [${status === "completed" ? "x" : " "}] ${escapeMarkdown(task.title)}${status === "wont_do" ? " — Won’t Do" : ""}`,
      );
      if (parentPath) lines.push(`  - Parent: ${escapeMarkdown(parentPath)}`);
      const fields: string[] = [];
      if (options.fields.includes("list"))
        fields.push(
          `List: ${escapeMarkdown(listNames.get(task.list_id) || "Inbox")}`,
        );
      if (options.fields.includes("due") && task.due_date)
        fields.push(
          `Due: ${task.due_date}${task.due_time ? " " + task.due_time : ""}`,
        );
      if (options.fields.includes("completed") && task.completed_at)
        fields.push(`Completed: ${summaryLocalDate(task.completed_at)}`);
      if (options.fields.includes("priority"))
        fields.push(
          `Priority: ${["None", "Low", "Medium", "High"][task.priority] || "None"}`,
        );
      if (options.fields.includes("tags") && task.tag_ids.length)
        fields.push(
          `Tags: ${task.tag_ids
            .map((id) => tagNames.get(id))
            .filter(Boolean)
            .map((name) => escapeMarkdown(name!))
            .join(", ")}`,
        );
      if (options.fields.includes("progress")) {
        const value = task.completed_at ? 100 : task.progress || 0;
        fields.push(`Progress: ${value}%`);
      }
      if (fields.length) lines.push(`  - ${fields.join(" · ")}`);
      if (options.fields.includes("notes") && task.notes_text?.trim())
        lines.push(`  - ${escapeMarkdown(task.notes_text)}`);
    }
    lines.push("");
  }
  if (!count) lines.push("No tasks match these filters.", "");
  return lines.join("\n");
}
