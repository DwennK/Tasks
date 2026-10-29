import { expect, test } from "@playwright/test";
import { parseTaskInput, splitTaskLines } from "../../src/lib/task-input";
import type { Tag, TaskList } from "../../src/types";

// Pure tests use local calendar values rather than UTC instants, including
// around midnight and DST. They need no database or browser page.
const now = new Date(2026, 9, 29, 10, 15); // Thursday
const parse = (
  text: string,
  settings: Record<string, unknown> = {},
  date = now,
) => parseTaskInput(text, settings, [], [], date);

test("recognizes French and English dates with time before or after the day", () => {
  expect(parse("Meeting at 9 tomorrow")).toMatchObject({
    title: "Meeting",
    due_date: "2026-10-30",
    due_time: "09:00",
  });
  expect(parse("Appeler dans 3 jours à 14h")).toMatchObject({
    title: "Appeler",
    due_date: "2026-11-01",
    due_time: "14:00",
  });
  expect(parse("Rendez-vous vendredi prochain à 14h30")).toMatchObject({
    title: "Rendez-vous",
    due_date: "2026-10-30",
    due_time: "14:30",
  });
  expect(parse("Call today at 2:30pm")).toMatchObject({
    title: "Call",
    due_date: "2026-10-29",
    due_time: "14:30",
  });
  expect(parse("Call 3 clients at 9 tomorrow")).toMatchObject({
    title: "Call 3 clients",
    due_date: "2026-10-30",
    due_time: "09:00",
  });
  expect(parse("Livrer après-demain")).toMatchObject({
    title: "Livrer",
    due_date: "2026-10-31",
  });
  expect(parse("Relire aujourd’hui à midi")).toMatchObject({
    title: "Relire",
    due_date: "2026-10-29",
    due_time: "12:00",
  });
});

test("emits backend-compatible recurring schedules", () => {
  expect(parse("Réunion tous les lundis à 9h")).toMatchObject({
    title: "Réunion",
    due_date: "2026-11-02",
    due_time: "09:00",
    recurrence: {
      frequency: "weekly",
      interval: 1,
      weekdays: [1],
      anchor_date: "2026-11-02",
      mode: "due_date",
      count: null,
      end_date: null,
    },
  });
  expect(parse("Review every 2 weeks")).toMatchObject({
    title: "Review",
    due_date: "2026-10-29",
    recurrence: { frequency: "weekly", interval: 2, weekdays: [4] },
  });
  expect(parse("Review every 2 weeks 2026-11-02")).toMatchObject({
    due_date: "2026-11-02",
    recurrence: { weekdays: [1], anchor_date: "2026-11-02" },
  });
  expect(parse("Read every day")).toMatchObject({
    title: "Read",
    recurrence: { frequency: "daily" },
  });
  expect(
    parse("Lire tous les jours ouvrés", {}, new Date(2026, 9, 31, 9)),
  ).toMatchObject({
    title: "Lire",
    due_date: "2026-11-02",
    recurrence: { frequency: "weekly", weekdays: [1, 2, 3, 4, 5] },
  });
  expect(parse("Work every weekday")).toMatchObject({
    title: "Work",
    recurrence: { frequency: "weekly", weekdays: [1, 2, 3, 4, 5] },
  });
  expect(parse("Bilan toutes les 3 semaines")).toMatchObject({
    title: "Bilan",
    recurrence: { frequency: "weekly", interval: 3 },
  });
  expect(parse("Report monthly")).toMatchObject({
    title: "Report",
    recurrence: { frequency: "monthly", interval: 1 },
  });
});

test("validates calendar dates instead of rolling invalid days forward", () => {
  expect(parse("Livrer 31/10")).toMatchObject({
    title: "Livrer",
    due_date: "2026-10-31",
  });
  expect(parse("Livrer 01/02")).toMatchObject({
    title: "Livrer",
    due_date: "2027-02-01",
  });
  expect(parse("Livrer 29/02/2028")).toMatchObject({ due_date: "2028-02-29" });
  expect(parse("Livrer 31/02/2027")).toEqual({ title: "Livrer 31/02/2027" });
  expect(parse("Livrer 2027-02-29")).toEqual({ title: "Livrer 2027-02-29" });
  expect(parse("Livrer 2026-13-03")).toEqual({ title: "Livrer 2026-13-03" });
  expect(parse("Livrer 2026-11-02")).toMatchObject({
    title: "Livrer",
    due_date: "2026-11-02",
  });
});

test("calendar relative dates remain local and clamp short months", () => {
  expect(
    parse("Review in 1 month", {}, new Date(2026, 0, 31, 20)),
  ).toMatchObject({ due_date: "2026-02-28" });
  expect(
    parse("Review in 1 year", {}, new Date(2028, 1, 29, 20)),
  ).toMatchObject({ due_date: "2029-02-28" });
  expect(parse("Call demain", {}, new Date(2026, 2, 28, 23, 50))).toMatchObject(
    { due_date: "2026-03-29" },
  );
  expect(
    parse("Call dans 30 minutes", {}, new Date(2026, 9, 29, 23, 45)),
  ).toMatchObject({ title: "Call", due_date: "2026-10-30", due_time: "00:15" });
  expect(
    parse("Call in 2 hours", {}, new Date(2026, 9, 29, 23, 15)),
  ).toMatchObject({ due_date: "2026-10-30", due_time: "01:15" });
});

test("does not consume ordinary title numbers or invalid times", () => {
  for (const title of [
    "Read 3 chapters",
    "Version 2026-10-29-alpha",
    "Score 12/20 points",
    "Order 14h30parts",
    "Call at 25:99",
    "Call at 13pm",
    "Read every 999 days",
  ])
    expect(parse(title)).toEqual({ title });
  expect(parse("Call at 9")).toMatchObject({
    due_date: "2026-10-30",
    due_time: "09:00",
  });
  expect(parse("Call at 11")).toMatchObject({
    due_date: "2026-10-29",
    due_time: "11:00",
  });
  expect(parse("Call at 12am tomorrow")).toMatchObject({
    due_date: "2026-10-30",
    due_time: "00:00",
  });
  expect(parse("Call at 12pm tomorrow")).toMatchObject({
    due_date: "2026-10-30",
    due_time: "12:00",
  });
});

test("respects keep and off recognition settings", () => {
  expect(
    parse("Call tomorrow at 9", { date_recognition: "keep" }),
  ).toMatchObject({
    title: "Call tomorrow at 9",
    due_date: "2026-10-30",
    due_time: "09:00",
  });
  expect(parse("Call tomorrow at 9", { date_recognition: "off" })).toEqual({
    title: "Call tomorrow at 9",
  });
  expect(parse("Run every Monday", { date_recognition: "keep" })).toMatchObject(
    { title: "Run every Monday", recurrence: { frequency: "weekly" } },
  );
});

test("combines list, priority and tag shorthand with natural dates", () => {
  const lists = [
    { id: "work", name: "Work Projects", archived_at: null },
  ] as TaskList[];
  const tags = [{ id: "urgent", name: "urgent" }] as Tag[];
  expect(
    parseTaskInput(
      "Plan tomorrow !1 ~Work Projects #urgent",
      {},
      lists,
      tags,
      now,
    ),
  ).toEqual({
    title: "Plan",
    priority: 3,
    list_id: "work",
    tag_ids: ["urgent"],
    due_date: "2026-10-30",
  });
  expect(parseTaskInput("Plan #unknown", {}, lists, tags, now)).toEqual({
    title: "Plan #unknown",
  });
  expect(
    parseTaskInput(
      "Plan #urgent",
      { tag_recognition: "keep" },
      lists,
      tags,
      now,
    ),
  ).toMatchObject({ title: "Plan #urgent", tag_ids: ["urgent"] });
});

test("splits clipboard lines without empty tasks or destructive rewriting", () => {
  expect(
    splitTaskLines("  First tomorrow\r\n\r\n  - Second #tag\nThird\rFourth  "),
  ).toEqual(["First tomorrow", "- Second #tag", "Third", "Fourth"]);
  expect(splitTaskLines("\n \r\n")).toEqual([]);
});
