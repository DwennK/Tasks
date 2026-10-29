import { expect } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  call,
  getTask,
  openTask,
  resetSettings,
  snapshot,
  test,
  unique,
} from "./helpers";

test.use({ timezoneId: "Europe/Zurich", reducedMotion: "reduce" });

test("overdue labels use local calendar days across daylight saving and update an open editor at midnight", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Local overdue days"),
  });
  const late = await call<Task>(page.request, "tasks.create", {
    title: unique("Spring clock overdue"),
    list_id: list.id,
    due_date: "2030-03-30",
    due_time: "09:30",
    timezone: "Europe/Zurich",
  });
  const future = await call<Task>(page.request, "tasks.create", {
    title: unique("Future stays neutral"),
    list_id: list.id,
    due_date: "2030-04-02",
  });
  try {
    // Zurich moves from UTC+1 to UTC+2 on March 31. At April 1 the local
    // midnights are 47 hours apart but the task is two calendar days overdue.
    await page.clock.install({ time: new Date("2030-03-31T23:58:00+02:00") });
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    const rowDate = page.getByRole("group", { name: late.title, exact: true }).locator(".due-label");
    await expect(rowDate).toHaveText("Yesterday");
    const rowColor = await rowDate.evaluate((node) => getComputedStyle(node).color);
    const rowChannels = rowColor.match(/\d+/g)!.map(Number);
    expect(rowChannels[0]).toBeGreaterThan(rowChannels[1]! * 1.3);
    await openTask(page, late.title);
    const due = page.getByRole("button", { name: "Set due date", exact: true });
    await expect(due).toHaveText("30 Mar, 09:30, 1d overdue");
    await expect(due).toHaveAttribute("title", "30 Mar, 09:30, 1d overdue");
    await expect(due).toHaveAttribute(
      "aria-description",
      "30 Mar, 09:30, 1d overdue",
    );
    const overdueColor = await due.evaluate(
      (node) => getComputedStyle(node).color,
    );
    const channels = overdueColor.match(/\d+/g)!.map(Number);
    expect(channels[0]).toBeGreaterThan(channels[1]! * 1.3);
    await page.screenshot({ path: "output/playwright/overdue-desktop.png" });
    const before = await snapshot(page);
    await page.clock.pauseAt(new Date("2030-03-31T23:59:50+02:00"));
    await page.clock.fastForward(15_000);
    await expect(due).toHaveText("30 Mar, 09:30, 2d overdue");
    await expect(rowDate).toHaveText("Mar 30");
    const after = await snapshot(page);
    expect(after.revision).toBe(before.revision);
    expect(await getTask(page, late.id)).toEqual(late);

    await openTask(page, future.title);
    await expect(due).not.toContainText("overdue");
    expect(await due.evaluate((node) => getComputedStyle(node).color)).not.toBe(
      overdueColor,
    );
    await openTask(page, late.title);
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(due).toHaveText("30 Mar, 09:30, 2d overdue");
    await page.screenshot({ path: "output/playwright/overdue-mobile.png" });
    const bounds = await due.boundingBox();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(390);
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
    ).toBeLessThanOrEqual(390);
    await page
      .getByRole("button", { name: "Complete task", exact: true })
      .click();
    await expect
      .poll(async () => (await getTask(page, late.id)).completed_at)
      .not.toBeNull();
    await expect(due).not.toContainText("overdue");
    expect(await due.evaluate((node) => getComputedStyle(node).color)).not.toBe(
      overdueColor,
    );
  } finally {
    await call(page.request, "tasks.delete", { id: late.id });
    await call(page.request, "tasks.delete", { id: future.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});

test("Postpone moves only the overdue group to today and preserves future descendants and task metadata", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Postpone overdue only"),
    group_by: "date",
  });
  try {
    const late = await call<Task>(page.request, "tasks.create", {
      title: unique("Late parent"),
      list_id: list.id,
      due_date: "2030-04-14",
      due_time: "10:45",
      timezone: "Europe/Zurich",
      priority: 3,
      notes: "Keep this note and the existing reminder",
      reminders: ["2030-04-14T08:30:00Z"],
      recurrence: { frequency: "monthly", interval: 1, mode: "due_date" },
    });
    const anotherLate = await call<Task>(page.request, "tasks.create", {
      title: unique("Second overdue"),
      list_id: list.id,
      due_date: "2030-04-10",
      priority: 1,
    });
    const futureChild = await call<Task>(page.request, "tasks.create", {
      title: unique("Future child untouched"),
      parent_id: late.id,
      due_date: "2030-04-18",
      due_time: "16:00",
    });
    const untouched: Task[] = [futureChild];
    for (const [title, due_date, pinned] of [
      ["Already today", "2030-04-15", false],
      ["Future root", "2030-04-16", false],
      ["Pinned overdue outside group", "2030-04-12", true],
    ] as const)
      untouched.push(
        await call<Task>(page.request, "tasks.create", {
          title: unique(title),
          list_id: list.id,
          due_date,
          pinned,
        }),
      );
    await page.clock.install({ time: new Date("2030-04-15T12:00:00+02:00") });
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    const postpone = page.getByRole("button", {
      name: "Postpone",
      exact: true,
    });
    await expect(postpone).toHaveCount(1);
    await expect(
      postpone.locator("..").getByRole("button", { name: /^Overdue/ }),
    ).toHaveText("Overdue2");
    await page.screenshot({
      path: "output/playwright/postpone-overdue-before.png",
    });
    await postpone.click();
    await expect
      .poll(async () => [
        (await getTask(page, late.id)).due_date,
        (await getTask(page, anotherLate.id)).due_date,
      ])
      .toEqual(["2030-04-15", "2030-04-15"]);
    await expect(postpone).toHaveCount(0);
    for (const original of [late, anotherLate]) {
      const changed = await getTask(page, original.id);
      expect(changed).toEqual({
        ...original,
        due_date: "2030-04-15",
        version: original.version + 1,
        updated_at: changed.updated_at,
      });
    }
    for (const task of untouched)
      expect(await getTask(page, task.id)).toEqual(task);
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await openTask(page, late.title);
    await expect(
      page.getByRole("button", { name: "Set due date", exact: true }),
    ).toHaveText("Today, 10:45");
    await page.screenshot({
      path: "output/playwright/postpone-overdue-after.png",
    });
    await page.reload();
    await expect(postpone).toHaveCount(0);
    expect((await getTask(page, late.id)).due_date).toBe("2030-04-15");
    for (const task of untouched)
      expect(await getTask(page, task.id)).toEqual(task);
  } finally {
    const fixtures = (await snapshot(page)).tasks.filter(
      (task) => task.list_id === list.id && !task.deleted_at,
    );
    for (const task of fixtures.filter((task) => !task.parent_id))
      await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
