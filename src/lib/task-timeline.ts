import type { Task } from "../types";

const DAY = 86_400_000;
export type TimelinePatch = Record<string, unknown>;
export type TimelineTask = Pick<Task,
  "id" | "version" | "due_date" | "due_time" | "start_date" | "start_time" |
  "duration_minutes" | "timezone" | "reminders" | "recurrence"
>;

/** Calendar arithmetic deliberately ignores the computer's time zone and DST. */
export function timelineDayNumber(day: string): number {
  const value = Date.parse(`${day}T00:00:00Z`);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day) || !Number.isFinite(value) ||
      new Date(value).toISOString().slice(0, 10) !== day)
    throw new Error("Choose a valid date.");
  return value / DAY;
}
export function timelineAddDays(day: string, days: number): string {
  const result = new Date((timelineDayNumber(day) + Math.round(days)) * DAY);
  if (result.getUTCFullYear() < 1 || result.getUTCFullYear() > 9999)
    throw new Error("The date is outside the supported range.");
  return result.toISOString().slice(0, 10);
}
export function timelineSpan(task: TimelineTask) {
  if (!task.due_date) return null;
  const end = timelineDayNumber(task.due_date);
  const start = task.start_date ? timelineDayNumber(task.start_date) : end;
  return { start, end: Math.max(start, end), days: Math.max(1, end - start + 1) };
}
function wallParts(instant: number, timezone: string) {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: timezone, year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit", hourCycle: "h23",
  }).formatToParts(new Date(instant));
  const get = (type: string) => parts.find((p) => p.type === type)!.value;
  const date = `${get("year")}-${get("month")}-${get("day")}`;
  const time = `${get("hour")}:${get("minute")}`;
  return { date, time, wall: Date.parse(`${date}T${time}:00Z`) };
}
/** Reject nonexistent local times; choose the first instant in a repeated hour. */
export function timelineInstant(day: string, clock: string, timezone: string) {
  timelineDayNumber(day);
  if (!/^(?:[01]\d|2[0-3]):[0-5]\d$/.test(clock))
    throw new Error("Choose a valid time.");
  const wall = Date.parse(`${day}T${clock}:00Z`);
  const offsets = new Set<number>();
  for (let i = -8; i <= 8; i++) {
    const instant = wall + i * 6 * 3_600_000;
    offsets.add(wallParts(instant, timezone).wall - instant);
  }
  const matches = [...offsets].map((offset) => wall - offset).filter((instant) => {
    const parts = wallParts(instant, timezone);
    return parts.date === day && parts.time === clock;
  }).sort((a, b) => a - b);
  if (!matches.length) throw new Error("This time does not exist on that date in the task's time zone. Open the task to choose another time.");
  return matches[0];
}
function shiftReminders(task: TimelineTask, dueDate: string, dueTime: string | null) {
  if (!task.due_date || !task.reminders?.length) return {};
  const timezone = task.timezone || "UTC";
  const before = timelineInstant(task.due_date, task.due_time || "09:00", timezone);
  const after = timelineInstant(dueDate, dueTime || "09:00", timezone);
  return { reminders: task.reminders.map((value) => new Date(Date.parse(value) + after - before).toISOString()) };
}

/** Move a whole task, preserving wall-clock times and recomputing elapsed duration at DST. */
export function moveTimelineTask(task: TimelineTask, firstDay: string): TimelinePatch {
  timelineDayNumber(firstDay);
  const oldStart = task.start_date || task.due_date;
  const offset = oldStart ? timelineDayNumber(firstDay) - timelineDayNumber(oldStart) : 0;
  const due = task.due_date ? timelineAddDays(task.due_date, offset) : firstDay;
  const patch: TimelinePatch = { due_date: due, expected_version: task.version };
  if (task.start_date) patch.start_date = firstDay;
  if (task.due_time) timelineInstant(due, task.due_time, task.timezone || "UTC");
  if (task.start_date && task.start_time)
    timelineInstant(firstDay, task.start_time, task.timezone || "UTC");
  if (task.start_date && task.duration_minutes != null) {
    const minutes = Math.round((timelineInstant(due, task.due_time || "23:59", task.timezone || "UTC") -
      timelineInstant(firstDay, task.start_time || "00:00", task.timezone || "UTC")) / 60_000);
    if (minutes < 1 || minutes > 525600) throw new Error("Duration must be between one minute and one year.");
    patch.duration_minutes = minutes;
  }
  Object.assign(patch, shiftReminders(task, due, task.due_time));
  // A moved recurring task becomes the anchor for future occurrences.
  if (task.recurrence) patch.recurrence = { ...task.recurrence, anchor_date: due };
  return patch;
}

export function resizeTimelineTask(task: TimelineTask, edge: "start" | "end", day: string): TimelinePatch {
  timelineDayNumber(day);
  if (!task.due_date) throw new Error("Schedule this task first.");
  const start = edge === "start" ? day : task.start_date || task.due_date;
  const due = edge === "end" ? day : task.due_date;
  if (start > due) throw new Error("The end must not be before the start.");
  const timezone = task.timezone || "UTC";
  const minutes = Math.round((timelineInstant(due, task.due_time || "23:59", timezone) -
    timelineInstant(start, task.start_time || "00:00", timezone)) / 60_000);
  if (minutes < 1 || minutes > 525600) throw new Error("Duration must be between one minute and one year.");
  const patch: TimelinePatch = { start_date: start, due_date: due, duration_minutes: minutes, expected_version: task.version };
  if (edge === "end") {
    Object.assign(patch, shiftReminders(task, due, task.due_time));
    if (task.recurrence) patch.recurrence = { ...task.recurrence, anchor_date: due };
  }
  return patch;
}
