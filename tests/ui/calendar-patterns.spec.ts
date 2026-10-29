import { expect, type Page } from "@playwright/test";
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

async function customRepeat(page: Page) {
  await page.getByRole("button", { name: "Set due date", exact: true }).click();
  const picker = page.getByRole("dialog", {
    name: "Task date and reminders",
    exact: true,
  });
  await picker.getByRole("button", { name: "Repeat", exact: true }).click();
  await picker
    .getByRole("menu", { name: "Repeat frequency", exact: true })
    .getByRole("menuitem", { name: "Custom", exact: true })
    .click();
  return { picker, custom: picker.locator(".custom-repeat-panel") };
}

test("custom monthly dates and a yearly leap-day rule survive reload and generate correct next occurrences", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Calendar patterns"),
  });
  const monthly = await call<Task>(page.request, "tasks.create", {
    title: unique("Three days monthly"),
    list_id: list.id,
    due_date: "2030-01-01",
  });
  const yearly = await call<Task>(page.request, "tasks.create", {
    title: unique("Annual leap day"),
    list_id: list.id,
    due_date: "2028-02-29",
  });
  try {
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await openTask(page, monthly.title);
    let { picker, custom } = await customRepeat(page);
    await custom
      .getByRole("combobox", { name: "Custom repeat unit", exact: true })
      .click();
    await page.getByRole("option", { name: "Month", exact: true }).click();
    await custom.getByRole("tab", { name: "Each", exact: true }).click();
    await expect(
      custom.getByRole("button", { name: "Repeat on day 1", exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    await custom
      .getByRole("button", { name: "Repeat on day 15", exact: true })
      .click();
    await custom
      .getByRole("button", { name: "Repeat on last day of month", exact: true })
      .click();
    await page.screenshot({
      path: "output/playwright/custom-repeat-monthly.png",
    });
    await custom.getByRole("button", { name: "OK", exact: true }).click();
    await picker.getByRole("button", { name: "OK", exact: true }).click();
    await expect(picker).toBeHidden();
    await expect
      .poll(async () =>
        (await getTask(page, monthly.id)).recurrence?.month_days
          ?.slice()
          .sort((a, b) => a - b),
      )
      .toEqual([-1, 1, 15]);
    expect((await getTask(page, monthly.id)).recurrence).toMatchObject({
      frequency: "monthly",
      interval: 1,
      mode: "due_date",
    });

    await openTask(page, yearly.title);
    ({ picker, custom } = await customRepeat(page));
    await custom
      .getByRole("combobox", { name: "Custom repeat unit", exact: true })
      .click();
    await page.getByRole("option", { name: "Year", exact: true }).click();
    await custom.getByRole("tab", { name: "Each", exact: true }).click();
    await expect(
      custom.getByRole("button", { name: "Repeat on day 29", exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    await page.screenshot({
      path: "output/playwright/custom-repeat-yearly.png",
    });
    await custom.getByRole("button", { name: "OK", exact: true }).click();
    await picker.getByRole("button", { name: "OK", exact: true }).click();
    await expect(picker).toBeHidden();
    await expect
      .poll(async () => (await getTask(page, yearly.id)).recurrence?.year_month)
      .toBe(2);
    expect((await getTask(page, yearly.id)).recurrence).toMatchObject({
      frequency: "yearly",
      month_days: [29],
      year_month: 2,
    });

    await page.reload();
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await openTask(page, monthly.title);
    ({ picker, custom } = await customRepeat(page));
    await expect(
      custom.getByRole("combobox", { name: "Custom repeat unit", exact: true }),
    ).toHaveText("Month");
    for (const name of [
      "Repeat on day 1",
      "Repeat on day 15",
      "Repeat on last day of month",
    ])
      await expect(
        custom.getByRole("button", { name, exact: true }),
      ).toHaveAttribute("aria-pressed", "true");
    await custom.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    await page
      .getByRole("button", { name: "Complete task", exact: true })
      .click();
    await expect
      .poll(async () => (await getTask(page, monthly.id)).recurrence_next_id)
      .not.toBeNull();
    const nextMonthly = await getTask(
      page,
      (await getTask(page, monthly.id)).recurrence_next_id!,
    );
    expect(nextMonthly.due_date).toBe("2030-01-15");
    expect(
      nextMonthly.recurrence?.month_days?.slice().sort((a, b) => a - b),
    ).toEqual([-1, 1, 15]);

    await openTask(page, yearly.title);
    ({ picker, custom } = await customRepeat(page));
    await expect(
      custom.getByRole("combobox", { name: "Custom repeat unit", exact: true }),
    ).toHaveText("Year");
    await expect(
      custom.getByRole("button", { name: "Repeat on day 29", exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    await custom.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    await page
      .getByRole("button", { name: "Complete task", exact: true })
      .click();
    await expect
      .poll(async () => (await getTask(page, yearly.id)).recurrence_next_id)
      .not.toBeNull();
    const nextYearly = await getTask(
      page,
      (await getTask(page, yearly.id)).recurrence_next_id!,
    );
    expect(nextYearly.due_date).toBe("2032-02-29");
    expect(nextYearly.recurrence).toMatchObject({
      frequency: "yearly",
      month_days: [29],
      year_month: 2,
    });
  } finally {
    for (const task of (await snapshot(page)).tasks.filter(
      (item) => item.list_id === list.id && !item.deleted_at,
    ))
      await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
