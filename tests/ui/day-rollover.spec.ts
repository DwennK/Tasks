import { expect, type Page } from "@playwright/test";
import { test, call, resetSettings, snapshot, unique } from "./helpers";
import type { Snapshot, Task } from "../../src/types";

test.use({ timezoneId: "Europe/Zurich", reducedMotion: "reduce" });

const DAY = "2030-01-15";
const NEXT_DAY = "2030-01-16";
const ENTERING_DAY = "2030-01-22";

function scheduled(data: Snapshot) {
  return data.tasks.filter((task) => {
    const list = data.lists.find((item) => item.id === task.list_id);
    return (
      !task.completed_at &&
      !task.deleted_at &&
      !task.wont_do_at &&
      !list?.archived_at &&
      list?.show_in_smart_lists !== false
    );
  });
}

function count(data: Snapshot, view: "today" | "upcoming", date: string) {
  const end = date === DAY ? "2030-01-21" : ENTERING_DAY;
  return scheduled(data).filter(
    (task) =>
      task.due_date &&
      (view === "today"
        ? task.due_date <= date
        : task.due_date >= date && task.due_date <= end),
  ).length;
}

const nav = (page: Page, name: "Today" | "Next 7 Days") =>
  page
    .locator(".smart-navigation")
    .getByRole("button", { name: new RegExp(`^${name}(?:\\s|$)`) });
const row = (page: Page, task: Task) =>
  page.getByRole("group", { name: task.title, exact: true });

async function expectCalendarGlyphs(page: Page, day: string, weekday: RegExp) {
  await expect(
    nav(page, "Today").locator('[data-task-glyph="today"] text'),
  ).toHaveText(day);
  await expect(
    nav(page, "Next 7 Days").locator('[data-task-glyph="upcoming"] text'),
  ).toHaveText(weekday);
}

test("Today and Upcoming refresh lists, date labels, navigation glyphs and counts at midnight without a database change", async ({
  page,
  context,
}) => {
  await resetSettings(page);
  const created: Task[] = [];
  let upcoming: Page | undefined;
  const observed: { action: string; page: string }[] = [];
  const extraPageErrors: string[] = [];
  function observe(target: Page, name: string) {
    target.on("request", (request) => {
      if (request.method() !== "POST" || !request.url().endsWith("/__tasks"))
        return;
      const body = request.postDataJSON();
      if (typeof body?.action === "string")
        observed.push({ action: body.action, page: name });
    });
  }
  try {
    for (const [name, due_date] of [
      ["Rollover previous today", DAY],
      ["Rollover next today A", NEXT_DAY],
      ["Rollover next today B", NEXT_DAY],
      ["Rollover entering week A", ENTERING_DAY],
      ["Rollover entering week B", ENTERING_DAY],
    ]) {
      created.push(
        await call<Task>(page.request, "tasks.create", {
          title: unique(name!),
          due_date,
        }),
      );
    }
    const [previousToday, nextToday, , enteringWeek] = created as [
      Task,
      Task,
      Task,
      Task,
      Task,
    ];
    const before = await snapshot(page);
    // Playwright installs one clock for this whole context, so both already-open
    // smart views cross midnight together without changing their navigation state.
    await page.clock.install({ time: new Date("2030-01-15T23:58:00+01:00") });
    observe(page, "today");
    await page.goto("/");
    await nav(page, "Today").click();
    await expect(
      page.getByRole("heading", { name: "Today", exact: true }),
    ).toBeVisible();
    await expect(row(page, previousToday).locator(".due-label")).toHaveText(
      "Today",
    );
    await expect(row(page, nextToday)).toHaveCount(0);

    upcoming = await context.newPage();
    upcoming.on("pageerror", (error) => extraPageErrors.push(error.message));
    upcoming.on("console", (message) => {
      if (
        message.type() === "error" ||
        /\[vite\].*(hot updated|page reload)/.test(message.text())
      )
        extraPageErrors.push(message.text());
    });
    observe(upcoming, "upcoming");
    await upcoming.goto("/");
    await nav(upcoming, "Next 7 Days").click();
    await expect(
      upcoming.getByRole("heading", { name: "Next 7 Days", exact: true }),
    ).toBeVisible();
    await expect(row(upcoming, previousToday)).toBeVisible();
    await expect(row(upcoming, nextToday).locator(".due-label")).toHaveText(
      "Tomorrow",
    );
    await expect(row(upcoming, enteringWeek)).toHaveCount(0);
    await expect(nav(page, "Today").locator("small")).toHaveText(
      String(count(before, "today", DAY)),
    );
    await expect(nav(upcoming, "Next 7 Days").locator("small")).toHaveText(
      String(count(before, "upcoming", DAY)),
    );

    // Tuesday15 becomes Wednesday16 on both already-open surfaces. These
    // are the visible SVG labels, independent of task counters or aria names.
    await expectCalendarGlyphs(page, "15", /^TU$/i);
    await expectCalendarGlyphs(upcoming, "15", /^TU$/i);

    await page.clock.pauseAt(new Date("2030-01-15T23:59:50+01:00"));
    const readStart = observed.length;
    await page.clock.fastForward(15_000);
    await expect(row(page, nextToday)).toBeVisible();
    await expect(row(page, nextToday).locator(".due-label")).toHaveText(
      "Today",
    );
    await expect(row(page, previousToday).locator(".due-label")).toHaveText(
      "Yesterday",
    );
    await expect(row(page, previousToday).locator(".due-label")).toHaveClass(
      /overdue/,
    );
    await expect(row(upcoming, previousToday)).toHaveCount(0);
    await expect(row(upcoming, enteringWeek)).toBeVisible();
    await expect(row(upcoming, nextToday).locator(".due-label")).toHaveText(
      "Today",
    );
    await expect(nav(page, "Today").locator("small")).toHaveText(
      String(count(before, "today", NEXT_DAY)),
    );
    await expect(nav(upcoming, "Next 7 Days").locator("small")).toHaveText(
      String(count(before, "upcoming", NEXT_DAY)),
    );
    await expectCalendarGlyphs(page, "16", /^WE$/i);
    await expectCalendarGlyphs(upcoming, "16", /^WE$/i);
    const after = await snapshot(page);
    expect(after.revision).toBe(before.revision);
    expect(
      observed.slice(readStart).filter((entry) => entry.action !== "revision"),
    ).toEqual([]);
    expect(extraPageErrors).toEqual([]);
  } finally {
    await upcoming?.close();
    for (const task of created)
      await call(page.request, "tasks.delete", { id: task.id });
  }
});
