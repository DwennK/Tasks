type ValueObject = Record<string, unknown>;
const object = (value: unknown): ValueObject | null =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as ValueObject)
    : null;
const text = (value: unknown) =>
  typeof value === "string" ? value.trim() : "";
const shortText = (value: unknown, limit = 180) => {
  const result = text(value).replace(/\s+/g, " ");
  return result.length > limit ? `${result.slice(0, limit)}…` : result;
};
const names = (
  value: unknown,
  choices: Record<string, string>,
  fallback = "Not set",
) => choices[String(value)] || fallback;
const priority = (value: unknown) =>
  names(value, { 0: "None", 1: "Low", 2: "Medium", 3: "High" });
const weekdays = [
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
  "Sunday",
];
const months = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

function repeatDescription(value: unknown): string {
  const rule = object(value);
  if (!rule) return "Does not repeat";
  const unit = names(
    rule.frequency,
    { daily: "day", weekly: "week", monthly: "month", yearly: "year" },
    "period",
  );
  const interval =
    typeof rule.interval === "number" && rule.interval > 0 ? rule.interval : 1;
  const parts = [`Every ${interval === 1 ? unit : `${interval} ${unit}s`}`];
  if (Array.isArray(rule.weekdays) && rule.weekdays.length) {
    const days = rule.weekdays
      .map((day) => weekdays[Number(day) - 1])
      .filter(Boolean);
    if (days.length) parts.push(`on ${days.join(", ")}`);
  }
  if (Array.isArray(rule.month_days) && rule.month_days.length)
    parts.push(`on day ${rule.month_days.join(", ")}`);
  const ordinal = object(rule.ordinal_weekday);
  if (ordinal) {
    const order = names(
      ordinal.ordinal,
      {
        1: "first",
        2: "second",
        3: "third",
        4: "fourth",
        5: "fifth",
        "-1": "last",
      },
      "selected",
    );
    const day = weekdays[Number(ordinal.weekday) - 1];
    if (day) parts.push(`on the ${order} ${day}`);
  }
  if (rule.workday === "first" || rule.workday === "last")
    parts.push(`on the ${rule.workday} weekday`);
  if (rule.year_month && months[Number(rule.year_month) - 1])
    parts.push(`in ${months[Number(rule.year_month) - 1]}`);
  if (rule.mode === "completion_date") parts.push("from completion");
  else parts.push("from due date");
  if (rule.skip_weekends === true) parts.push("skip weekends");
  if (text(rule.end_date)) parts.push(`until ${text(rule.end_date)}`);
  else if (typeof rule.count === "number")
    parts.push(`${rule.count} occurrences`);
  return parts.join(" · ");
}
function checklistLines(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const items = value.map(object).filter((item): item is ValueObject => !!item);
  const lines = [
    `Checklist: ${items.filter((item) => item.completed === true).length}/${items.length} completed`,
  ];
  lines.push(
    ...items
      .slice(0, 6)
      .map(
        (item) =>
          `${item.completed === true ? "☑" : "☐"} ${shortText(item.text) || "Untitled item"}`,
      ),
  );
  if (items.length > 6)
    lines.push(`… ${items.length - 6} more checklist items`);
  return lines;
}
function taskDetails(value: ValueObject): string[] {
  const lines: string[] = [];
  if ("progress" in value)
    lines.push(
      `Progress: ${Number(value.progress) || 0}%${value.progress_mode === "checklist" ? " (from checklist)" : ""}`,
    );
  if ("due_date" in value)
    lines.push(
      `Due: ${[text(value.due_date), text(value.due_time)].filter(Boolean).join(" ") || "No due date"}`,
    );
  if ("start_date" in value)
    lines.push(
      `Starts: ${[text(value.start_date), text(value.start_time)].filter(Boolean).join(" ") || "No start date"}`,
    );
  if (typeof value.duration_minutes === "number")
    lines.push(`Duration: ${value.duration_minutes} minutes`);
  if (text(value.timezone)) lines.push(`Time zone: ${text(value.timezone)}`);
  if ("recurrence" in value)
    lines.push(`Repeat: ${repeatDescription(value.recurrence)}`);
  if ("priority" in value) lines.push(`Priority: ${priority(value.priority)}`);
  if ("is_note" in value)
    lines.push(`Type: ${value.is_note ? "Note" : "Task"}`);
  if ("pinned" in value) lines.push(`Pinned: ${value.pinned ? "Yes" : "No"}`);
  if ("wont_do_at" in value)
    lines.push(`Won’t Do: ${text(value.wont_do_at) || "No"}`);
  lines.push(...checklistLines(value.checklist));
  return lines;
}
function displayPreferences(value: ValueObject): string {
  return [
    `View: ${names(value.view_mode, { list: "List", board: "Kanban board", timeline: "Timeline" }, "List")}`,
    `Group by: ${names(value.group_by, { none: "None", section: "Section", date: "Date", priority: "Priority", list: "List", tag: "Tag" }, "None")}`,
    `Sort by: ${names(value.sort_by, { manual: "Manual order", due_date: "Due date", priority: "Priority", title: "Title", created_at: "Creation date" }, "Manual order")}`,
    `Order: ${value.sort_order === "desc" ? "Descending" : "Ascending"}`,
  ].join("\n");
}
function templateContent(value: ValueObject): string {
  const lines = [
    `Title: ${shortText(value.title) || "Untitled"}`,
    ...taskDetails(value),
  ];
  if (text(value.notes)) {
    // This is a text-only preview. Never insert provider content into the DOM as HTML.
    const notes =
      value.notes_format === "html"
        ? text(value.notes)
            .replace(/<(script|style)\b[^>]*>[\s\S]*?<\/\1>/gi, "")
            .replace(/<[^>]*>/g, " ")
            .replace(/&nbsp;/g, " ")
            .replace(/&lt;/g, "<")
            .replace(/&gt;/g, ">")
            .replace(/&amp;/g, "&")
        : value.notes;
    lines.push(`Description: ${shortText(notes, 260)}`);
  }
  if (Array.isArray(value.subtasks)) {
    lines.push(`Subtasks: ${value.subtasks.length}`);
    lines.push(
      ...value.subtasks
        .slice(0, 5)
        .map(
          (item) => `• ${shortText(object(item)?.title) || "Untitled subtask"}`,
        ),
    );
    if (value.subtasks.length > 5)
      lines.push(`… ${value.subtasks.length - 5} more subtasks`);
  }
  if (Array.isArray(value.tag_ids))
    lines.push(`Tags: ${value.tag_ids.length} selected`);
  return lines.join("\n");
}
function portablePreferences(id: string, value: unknown): string {
  if (!Array.isArray(value)) return "Saved preferences preview is incomplete.";
  const heading =
    id === "saved_filters" ? "Saved filters" : "Summary templates";
  const lines = [`${heading}: ${value.length}`];
  for (const item of value.slice(0, 8)) {
    const data = object(item);
    if (!data) continue;
    lines.push(`• ${shortText(data.name) || "Untitled"}`);
    const options = object(data.options) || data;
    if (id === "summary_templates") {
      lines.push(
        `  Dates: ${names(options.range, { this_week: "This week", last_week: "Last week", this_month: "This month", custom: `${text(options.from)} – ${text(options.to)}` })}`,
      );
      lines.push(
        `  Based on: ${names(options.basis, { completed: "Completion date", due: "Due date", created: "Creation date" })}`,
      );
    } else {
      lines.push(
        `  Dates: ${names(options.date, { all: "Any date", today: "Today", tomorrow: "Tomorrow", week: "Next 7 days", overdue: "Overdue", undated: "No due date" })}`,
      );
    }
    if (Array.isArray(options.priorities) && options.priorities.length)
      lines.push(`  Priority: ${options.priorities.map(priority).join(", ")}`);
  }
  if (value.length > 8) lines.push(`… ${value.length - 8} more saved items`);
  return lines.join("\n");
}

/** Returns readable text for structured fields, or null for ordinary text fields. */
export function structuredConflictPreview(
  field: string,
  value: unknown,
  kind = "",
  id = "",
): string | null {
  const portable =
    kind === "settings" &&
    field === "value" &&
    ["saved_filters", "summary_templates"].includes(id);
  if (
    !["details_json", "preferences_json", "task_json"].includes(field) &&
    !portable
  )
    return null;
  let parsed = value;
  if (typeof value === "string") {
    try {
      parsed = JSON.parse(value);
    } catch {
      return "This preview is incomplete. The full saved version is preserved.";
    }
  }
  if (portable) return portablePreferences(id, parsed);
  const data = object(parsed);
  if (!data) return "No saved details";
  if (field === "preferences_json") return displayPreferences(data);
  if (field === "task_json") return templateContent(data);
  return taskDetails(data).join("\n") || "No additional task details";
}
