import { expect, test } from "@playwright/test";
import { structuredConflictPreview } from "../../src/lib/cloud-conflict-preview";

test("task detail conflicts expose schedules, progress and checklist alternatives as readable text", () => {
  const preview = structuredConflictPreview(
    "details_json",
    JSON.stringify({
      progress: 75,
      progress_mode: "checklist",
      start_date: "2026-10-29",
      start_time: "09:30",
      duration_minutes: 90,
      timezone: "Europe/Zurich",
      is_note: false,
      pinned: true,
      recurrence: {
        frequency: "weekly",
        interval: 2,
        weekdays: [1, 7],
        mode: "completion_date",
        count: 5,
      },
      checklist: [
        { id: "internal-check-id", text: "Publish <draft>", completed: true },
        { id: "other-id", text: "Review", completed: false },
      ],
    }),
  );
  expect(preview).toContain("Progress: 75% (from checklist)");
  expect(preview).toContain("Starts: 2026-10-29 09:30");
  expect(preview).toContain("Time zone: Europe/Zurich");
  expect(preview).toContain(
    "Every 2 weeks · on Monday, Sunday · from completion · 5 occurrences",
  );
  expect(preview).toContain("☑ Publish <draft>\n☐ Review");
  expect(preview).not.toContain("internal-check-id");
  expect(preview).not.toContain("progress_mode");
});

test("display preference conflicts name views, grouping and ordering", () => {
  expect(
    structuredConflictPreview("preferences_json", {
      view_mode: "timeline",
      group_by: "priority",
      sort_by: "due_date",
      sort_order: "desc",
    }),
  ).toBe(
    "View: Timeline\nGroup by: Priority\nSort by: Due date\nOrder: Descending",
  );
});

test("template conflicts preserve useful text while suppressing markup and internal IDs", () => {
  const preview = structuredConflictPreview(
    "task_json",
    JSON.stringify({
      title: "Release template",
      notes_format: "html",
      notes:
        "<p>Review &amp; publish</p><script>alert('never render')</script>",
      priority: 3,
      tag_ids: ["internal-tag-id"],
      subtasks: [{ id: "internal-subtask-id", title: "Proofread" }],
    }),
  );
  expect(preview).toContain("Title: Release template");
  expect(preview).toContain("Priority: High");
  expect(preview).toContain("Description: Review & publish");
  expect(preview).toContain("• Proofread");
  expect(preview).not.toContain("<script>");
  expect(preview).not.toContain("never render");
  expect(preview).not.toContain("internal-");
});

test("portable settings conflicts show saved names and meaningful date selections", () => {
  expect(
    structuredConflictPreview(
      "value",
      JSON.stringify([
        {
          id: "preset-id",
          name: "Weekly review",
          options: { range: "last_week", basis: "completed" },
        },
      ]),
      "settings",
      "summary_templates",
    ),
  ).toContain(
    "• Weekly review\n  Dates: Last week\n  Based on: Completion date",
  );
  expect(
    structuredConflictPreview(
      "value",
      [{ id: "filter-id", name: "Urgent", date: "overdue", priorities: [3] }],
      "settings",
      "saved_filters",
    ),
  ).toContain("• Urgent\n  Dates: Overdue\n  Priority: High");
});

test("shortened structured previews remain explicit and ordinary text retains its existing path", () => {
  expect(
    structuredConflictPreview(
      "details_json",
      '{"checklist":[{"text":"a long partial',
    ),
  ).toBe("This preview is incomplete. The full saved version is preserved.");
  expect(structuredConflictPreview("title", "Launch Monday")).toBeNull();
  expect(structuredConflictPreview("notes", "{literal text}")).toBeNull();
});
