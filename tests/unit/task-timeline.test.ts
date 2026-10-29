import test from "node:test";
import assert from "node:assert/strict";
import { moveTimelineTask, resizeTimelineTask, timelineAddDays, timelineDayNumber, timelineInstant, timelineSpan, type TimelineTask } from "../../src/lib/task-timeline.ts";

function task(overrides: Partial<TimelineTask> = {}): TimelineTask {
  return { id: "task-1", version: 7, start_date: null, start_time: null, duration_minutes: null,
    due_date: "2026-10-29", due_time: null, timezone: "Europe/Zurich", reminders: [], recurrence: null, ...overrides };
}

test("calendar movement remains one day across DST and rejects impossible dates", () => {
  assert.equal(timelineAddDays("2026-03-28", 1), "2026-03-29");
  assert.equal(timelineAddDays("2026-10-25", 1), "2026-10-26");
  assert.equal(timelineAddDays("2028-02-28", 1), "2028-02-29");
  assert.throws(() => timelineDayNumber("2026-02-29"), /valid date/);
});

test("moving a range preserves its calendar span, wall times and optimistic version", () => {
  const original = task({ start_date: "2026-10-29", start_time: "10:15", due_date: "2026-10-31", due_time: "11:15", duration_minutes: 2940 });
  const patch = moveTimelineTask(original, "2026-11-02");
  assert.deepEqual(patch, { start_date: "2026-11-02", due_date: "2026-11-04", duration_minutes: 2940, expected_version: 7 });
  assert.equal(original.start_date, "2026-10-29");
  assert.equal(original.due_time, "11:15");
});

test("moving across DST recomputes elapsed duration but preserves both wall-clock times", () => {
  const original = task({ start_date: "2026-03-27", start_time: "09:00", due_date: "2026-03-28", due_time: "09:00", duration_minutes: 1440 });
  const patch = moveTimelineTask(original, "2026-03-28");
  assert.equal(patch.start_date, "2026-03-28");
  assert.equal(patch.due_date, "2026-03-29");
  assert.equal(patch.duration_minutes, 1380);
  assert.equal("due_time" in patch, false);
  assert.equal("start_time" in patch, false);
});

test("moving a recurring task shifts reminder instants and the recurrence anchor", () => {
  const recurrence = { frequency: "weekly" as const, interval: 1, weekdays: [4], end_date: null, count: null, mode: "due_date" as const, anchor_date: "2026-10-29" };
  const patch = moveTimelineTask(task({ due_time: "09:00", reminders: ["2026-10-29T07:30:00Z"], recurrence }), "2026-10-30");
  assert.deepEqual(patch.reminders, ["2026-10-30T07:30:00.000Z"]);
  assert.deepEqual(patch.recurrence, { ...recurrence, anchor_date: "2026-10-30" });
  assert.equal(recurrence.anchor_date, "2026-10-29");
});

test("reminders follow local due time across spring DST", () => {
  const patch = moveTimelineTask(task({ due_date: "2026-03-28", due_time: "09:00", reminders: ["2026-03-28T07:45:00Z"] }), "2026-03-29");
  assert.deepEqual(patch.reminders, ["2026-03-29T06:45:00.000Z"]);
});

test("arranging an undated task only sets its date and expected version", () => {
  assert.deepEqual(moveTimelineTask(task({ due_date: null }), "2026-11-01"), { due_date: "2026-11-01", expected_version: 7 });
  assert.equal(timelineSpan(task({ due_date: null })), null);
});

test("edge resize creates a duration without changing the task's saved time", () => {
  const patch = resizeTimelineTask(task({ due_time: "17:00" }), "start", "2026-10-28");
  assert.deepEqual(patch, { start_date: "2026-10-28", due_date: "2026-10-29", duration_minutes: 2460, expected_version: 7 });
  assert.throws(() => resizeTimelineTask(task(), "start", "2026-10-30"), /end must not be before/);
  assert.throws(() => resizeTimelineTask(task({ start_date: "2026-10-29", start_time: "17:00", due_time: "16:00" }), "end", "2026-10-29"), /Duration/);
});

test("start resize keeps existing reminders, end resize shifts them", () => {
  const original = task({ due_time: "17:00", reminders: ["2026-10-29T15:30:00Z"] });
  assert.equal("reminders" in resizeTimelineTask(original, "start", "2026-10-28"), false);
  assert.deepEqual(resizeTimelineTask(original, "end", "2026-10-30").reminders, ["2026-10-30T15:30:00.000Z"]);
});

test("nonexistent local DST times are rejected instead of silently changing task time", () => {
  assert.throws(() => timelineInstant("2026-03-29", "02:30", "Europe/Zurich"), /does not exist/);
  assert.throws(() => moveTimelineTask(task({ due_date: "2026-03-28", due_time: "02:30" }), "2026-03-29"), /does not exist/);
  assert.equal(new Date(timelineInstant("2026-10-25", "02:30", "Europe/Zurich")).toISOString(), "2026-10-25T00:30:00.000Z");
});

test("a resize cannot exceed the supported one-year duration", () => {
  assert.throws(() => resizeTimelineTask(task(), "end", "2027-10-30"), /one year/);
});
