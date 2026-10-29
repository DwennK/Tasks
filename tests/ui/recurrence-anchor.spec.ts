import { expect } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  test,
  call,
  getTask,
  openTask,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";

test("editing a February occurrence preserves the original month-end recurrence anchor", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Month-end series"),
  });
  const january = await call<Task>(page.request, "tasks.create", {
    title: unique("Last day monthly"),
    list_id: list.id,
    due_date: "2030-01-31",
    due_time: "09:30",
    timezone: "UTC",
    recurrence: { frequency: "monthly", interval: 1, mode: "due_date" },
  });
  try {
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await openTask(page, january.title);
    await page
      .getByRole("button", { name: "Complete task", exact: true })
      .click();
    await expect
      .poll(async () => (await getTask(page, january.id)).recurrence_next_id)
      .not.toBeNull();
    const february = await getTask(
      page,
      (await getTask(page, january.id)).recurrence_next_id!,
    );
    expect(february.due_date).toBe("2030-02-28");
    expect(february.recurrence).toMatchObject({ anchor_date: "2030-01-31" });
    await page
      .locator(`[data-task-id="${february.id}"]`)
      .getByRole("button", { name: `Open task ${february.title}`, exact: true })
      .click();
    const beforePreview = (await snapshot(page)).revision;
    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Task date and reminders",
      exact: true,
    });
    await picker
      .getByRole("button", { name: "Next month", exact: true })
      .click();
    const marchCalendar = picker.getByRole("grid", {
      name: "March 2030",
      exact: true,
    });
    await expect(
      marchCalendar.locator('[data-date="2030-03-31"]'),
    ).toHaveAttribute("data-recurrence-preview", "true");
    await expect(
      marchCalendar.locator('[data-date="2030-03-28"]'),
    ).not.toHaveAttribute("data-recurrence-preview", "true");
    expect((await snapshot(page)).revision).toBe(beforePreview);
    await page.screenshot({ path: "output/playwright/month-end-preview.png" });

    await picker.getByRole("button", { name: "Time", exact: true }).click();
    await picker.getByLabel("Due time", { exact: true }).fill("14:45");
    await picker.getByLabel("Due time", { exact: true }).press("Enter");
    await picker.getByRole("button", { name: "OK", exact: true }).click();
    await expect(picker).toBeHidden();
    await expect
      .poll(async () => (await getTask(page, february.id)).due_time)
      .toBe("14:45");
    const edited = await getTask(page, february.id);
    expect(edited.due_date).toBe("2030-02-28");
    expect(edited.recurrence).toMatchObject({ anchor_date: "2030-01-31" });

    await page
      .getByRole("button", { name: "Complete task", exact: true })
      .click();
    await expect
      .poll(async () => (await getTask(page, february.id)).recurrence_next_id)
      .not.toBeNull();
    const march = await getTask(
      page,
      (await getTask(page, february.id)).recurrence_next_id!,
    );
    expect(march.due_date).toBe("2030-03-31");
    expect(march.due_time).toBe("14:45");
    expect(march.recurrence).toMatchObject({ anchor_date: "2030-01-31" });
    await page.reload();
    expect(await getTask(page, march.id)).toEqual(march);
  } finally {
    for (const task of (await snapshot(page)).tasks.filter(
      (item) => item.list_id === list.id && !item.deleted_at,
    ))
      await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
