import { expect, test } from "@playwright/test";
import type { Task, TaskList, Tag } from "../../src/types";
import {
  buildSummary,
  defaultSummaryOptions,
  summaryLocalDate,
  summaryMarkdown,
  summaryRange,
  type SummaryOptions,
} from "../../src/lib/task-summary";

function task(id: string, overrides: Partial<Task> = {}): Task {
  return {
    id,
    title: id,
    is_note: false,
    progress: 0,
    progress_mode: "manual",
    notes: "",
    notes_text: "",
    notes_format: "plain",
    list_id: "inbox",
    parent_id: null,
    due_date: "2026-10-28",
    due_time: null,
    start_date: null,
    start_time: null,
    duration_minutes: null,
    timezone: "Europe/Zurich",
    reminder_at: null,
    reminders: [],
    recurrence: null,
    recurrence_next_id: null,
    tag_ids: [],
    linked_task_ids: [],
    attachments: [],
    checklist: [],
    comments: [],
    pinned: false,
    wont_do_at: null,
    section_id: null,
    priority: 0,
    position: 0,
    completed_at: null,
    deleted_at: null,
    created_at: "2026-10-28T12:00:00Z",
    updated_at: "2026-10-28T12:00:00Z",
    version: 1,
    ...overrides,
  };
}
const lists: TaskList[] = [
  { id: "inbox", name: "Inbox" },
  { id: "work", name: "Work" },
].map((list) => ({
  ...list,
  pinned: false,
  icon: "",
  list_type: "task",
  show_in_smart_lists: true,
  folder_id: null,
  color: "#000000",
  position: 0,
  archived_at: null,
  created_at: "2026-10-28T12:00:00Z",
  updated_at: "2026-10-28T12:00:00Z",
  view_mode: "list",
  group_by: "none",
  sort_by: "manual",
  sort_order: "asc",
}));
function options(overrides: Partial<SummaryOptions> = {}): SummaryOptions {
  return {
    ...defaultSummaryOptions(new Date(2026, 9, 28)),
    basis: "due",
    statuses: [],
    ...overrides,
  };
}

test("summary calendar ranges respect the preferred week start and month boundaries", () => {
  const today = new Date(2026, 9, 28, 12);
  expect(summaryRange("this_week", today, 1)).toEqual({
    from: "2026-10-26",
    to: "2026-11-01",
  });
  expect(summaryRange("last_week", today, 0)).toEqual({
    from: "2026-10-18",
    to: "2026-10-24",
  });
  expect(summaryRange("this_month", new Date(2028, 1, 29))).toEqual({
    from: "2028-02-01",
    to: "2028-02-29",
  });
  expect(summaryLocalDate("2026-10-28")).toBe("2026-10-28");
  expect(summaryLocalDate(new Date(2026, 9, 28, 0, 15).toISOString())).toBe(
    "2026-10-28",
  );
  expect(summaryLocalDate("invalid")).toBe("");
});

test("summary counts each matching task and nested task once and names its ancestry", () => {
  const root = task("root", { title: "Launch", list_id: "work" });
  const child = task("child", {
    title: "Review",
    list_id: "work",
    parent_id: "root",
  });
  const leaf = task("leaf", {
    title: "Check links",
    list_id: "work",
    parent_id: "child",
  });
  const groups = buildSummary(
    [
      root,
      child,
      leaf,
      leaf,
      task("deleted", { deleted_at: "2026-10-28" }),
      task("note", { is_note: true }),
    ],
    lists,
    options(),
  );
  expect(groups).toHaveLength(1);
  expect(groups[0].entries).toHaveLength(3);
  expect(
    groups[0].entries.find((entry) => entry.task.id === "leaf")?.parentPath,
  ).toBe("Launch › Review");
  expect(
    buildSummary(
      [root, child, leaf],
      lists,
      options({ include_subtasks: false }),
    )[0].entries.map((entry) => entry.task.id),
  ).toEqual(["root"]);
});

test("summary filters combine list, any selected tag, priority and status with inclusive dates", () => {
  const matches = task("matches", {
    list_id: "work",
    priority: 3,
    tag_ids: ["other", "tag"],
    completed_at: "2026-10-26T12:00:00Z",
  });
  const otherTasks = [
    task("wrong-list", { ...matches, id: "wrong-list", list_id: "inbox" }),
    task("wrong-tag", { ...matches, id: "wrong-tag", tag_ids: ["other"] }),
    task("wrong-priority", { ...matches, id: "wrong-priority", priority: 1 }),
    task("active", { ...matches, id: "active", completed_at: null }),
    task("outside", {
      ...matches,
      id: "outside",
      completed_at: "2026-10-25T12:00:00Z",
    }),
  ];
  const filter = options({
    basis: "completed",
    statuses: ["completed"],
    priorities: [3],
    list_ids: ["work"],
    tag_ids: ["tag", "missing"],
  });
  expect(
    buildSummary([matches, ...otherTasks], lists, filter).flatMap((group) =>
      group.entries.map((entry) => entry.task.id),
    ),
  ).toEqual(["matches"]);
  const atEnd = task("end", { completed_at: "2026-11-01T12:00:00Z" });
  expect(
    buildSummary([atEnd], lists, options({ basis: "completed" }))[0].entries[0]
      .task.id,
  ).toBe("end");
  expect(
    buildSummary(
      [matches],
      lists,
      options({ from: "2026-11-02", to: "2026-11-01" }),
    ),
  ).toEqual([]);
});

test("summary date basis and status grouping remain independent", () => {
  const source = [
    task("active", { due_date: null }),
    task("completed", { completed_at: "2026-10-27T12:00:00Z" }),
    task("declined", { wont_do_at: "2026-10-28T12:00:00Z" }),
  ];
  const grouped = buildSummary(
    source,
    lists,
    options({ basis: "created", group_by: "status" }),
  );
  expect(grouped.map((group) => group.id)).toEqual([
    "active",
    "completed",
    "wont_do",
  ]);
  expect(
    buildSummary(source, lists, options({ basis: "completed" })).flatMap(
      (group) => group.entries,
    ),
  ).toHaveLength(1);
  expect(
    buildSummary(source, lists, options({ basis: "due" })).flatMap(
      (group) => group.entries,
    ),
  ).toHaveLength(2);
});

test("summary group identity and parent traversal tolerate duplicate labels and malformed ancestry", () => {
  const sameNames = lists.map((list) => ({ ...list, name: "Same name" }));
  const groups = buildSummary(
    [
      task("a", { parent_id: "b" }),
      task("b", { parent_id: "a", list_id: "work" }),
    ],
    sameNames,
    options(),
  );
  expect(groups).toHaveLength(2);
  expect(groups.map((group) => group.id).sort()).toEqual(["inbox", "work"]);
  expect(groups[0].entries[0].parentPath).toBe("b");
});

test("summary export escapes task content and preserves effective progress from note checklists", () => {
  const source = task("task", {
    title: "[Link](example)\n# heading",
    notes_text: "<script> text",
    progress: 75,
    progress_mode: "checklist",
    checklist: [{ id: "one", text: "Done", completed: true }],
    tag_ids: ["tag"],
  });
  const tags: Tag[] = [
    {
      id: "tag",
      name: "A*B",
      color: "",
      position: 0,
      created_at: "",
      updated_at: "",
    },
  ];
  const filter = options({
    fields: ["tags", "progress", "notes"],
    group_by: "none",
  });
  const output = summaryMarkdown(
    buildSummary([source], lists, filter),
    lists,
    tags,
    filter,
  );
  expect(output).toContain("1 task (including subtasks)");
  expect(output).toContain("\\[Link\\](example) \\# heading");
  expect(output).toContain("Tags: A\\*B · Progress: 75%");
  expect(output).toContain("\\<script\\> text");
  expect(output).not.toContain("\n# heading");
});
