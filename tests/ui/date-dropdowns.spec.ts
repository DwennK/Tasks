import { expect, type Locator, type Page } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  call,
  getTask,
  openTask,
  resetSettings,
  test,
  unique,
} from "./helpers";

async function expectWithinViewport(page: Page, popup: Locator) {
  await expect(popup).toBeVisible();
  const bounds = (await popup.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(bounds.width).toBeGreaterThan(0);
  expect(bounds.height).toBeGreaterThan(0);
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.y).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(viewport.height);
}

async function expectActiveOption(
  trigger: Locator,
  options: Locator,
  name: string,
) {
  const option = options.getByRole("option", { name, exact: true });
  await expect(option).toBeVisible();
  const id = await option.getAttribute("id");
  expect(id).toBeTruthy();
  await expect(trigger).toBeFocused();
  await expect(trigger).toHaveAttribute("aria-activedescendant", id!);
}

test("date dropdowns support keyboard selection and mobile bounds while Cancel preserves the saved schedule", async ({
  page,
}) => {
  await resetSettings(page);
  let list: TaskList | undefined;
  let task: Task | undefined;
  try {
    list = await call<TaskList>(page.request, "lists.create", {
      name: unique("Date dropdowns"),
    });
    task = await call<Task>(page.request, "tasks.create", {
      title: unique("Keep the saved schedule"),
      list_id: list.id,
      due_date: "2030-05-15",
      due_time: "09:30",
      timezone: "UTC",
      reminders: ["2030-05-15T09:15:00Z"],
      recurrence: {
        frequency: "monthly",
        interval: 1,
        mode: "due_date",
        month_days: [15],
      },
    });
    const original = await getTask(page, task.id);
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await openTask(page, task.title);
    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Task date and reminders",
      exact: true,
    });
    const custom = picker.getByRole("group", {
      name: "Custom repeat",
      exact: true,
    });
    const openCustom = async () => {
      await picker.getByRole("button", { name: "Repeat", exact: true }).click();
      await picker
        .getByRole("menu", { name: "Repeat frequency", exact: true })
        .getByRole("menuitem", { name: "Custom", exact: true })
        .click();
      await expect(custom).toBeVisible();
    };
    await openCustom();
    const basis = custom.getByRole("combobox", {
      name: "Repeat based on",
      exact: true,
    });
    const unit = custom.getByRole("combobox", {
      name: "Custom repeat unit",
      exact: true,
    });
    await expect(basis).toHaveText("By Due Dates");
    await basis.click();
    const basisOptions = picker.getByRole("listbox", {
      name: "Repeat based on",
      exact: true,
    });
    await expectActiveOption(basis, basisOptions, "By Due Dates");
    await page.keyboard.press("ArrowDown");
    await expectActiveOption(basis, basisOptions, "By Completion Dates");
    await page.keyboard.press("Enter");
    await expect(basisOptions).toBeHidden();
    await expect(basis).toHaveText("By Completion Dates");
    await expect(basis).toBeFocused();

    // Browsing another frequency and pressing Escape must not choose it.
    await unit.click();
    const unitOptions = picker.getByRole("listbox", {
      name: "Custom repeat unit",
      exact: true,
    });
    await expectActiveOption(unit, unitOptions, "Month");
    await page.keyboard.press("ArrowDown");
    await expectActiveOption(unit, unitOptions, "Year");
    await page.keyboard.press("Escape");
    await expect(unitOptions).toBeHidden();
    await expect(unit).toBeFocused();
    await expect(unit).toHaveText("Month");
    await expect(custom).toBeVisible();
    await custom.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(custom).toBeHidden();
    await expect(picker).toBeVisible();
    expect(await getTask(page, task.id)).toEqual(original);

    await openCustom();
    await expect(basis).toHaveText("By Due Dates");
    await expect(unit).toHaveText("Month");
    await page.setViewportSize({ width: 390, height: 844 });
    await unit.click();
    await expectWithinViewport(page, unitOptions);
    await page.screenshot({
      path: "output/playwright/date-repeat-dropdown-mobile.png",
    });
    await page.keyboard.press("Escape");
    await expect(unitOptions).toBeHidden();
    await expect(unit).toBeFocused();
    // Accessibility activation can open a menu without moving DOM focus.
    await page.evaluate(() => {
      if (document.activeElement instanceof HTMLElement)
        document.activeElement.blur();
    });
    await expect(page.locator("body")).toBeFocused();
    await unit.dispatchEvent("click");
    await expect(unitOptions).toBeVisible();
    await expect(page.locator("body")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(unitOptions).toBeHidden();
    await expect(custom).toBeVisible();
    await expect(picker).toBeVisible();
    await expect(unit).toBeFocused();

    const interval = custom.getByRole("spinbutton", {
      name: "Repeat interval",
      exact: true,
    });
    await interval.focus();
    await unit.dispatchEvent("click");
    await expect(unitOptions).toBeVisible();
    await expect(interval).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(unitOptions).toBeHidden();
    await expect(custom).toBeVisible();
    await expect(picker).toBeVisible();
    await expect(unit).toBeFocused();
    await custom.getByRole("button", { name: "Cancel", exact: true }).click();

    await picker.getByRole("button", { name: "Reminder", exact: true }).click();
    await picker
      .getByRole("group", { name: "Reminder options", exact: true })
      .getByRole("menuitem", { name: "Custom", exact: true })
      .click();
    const reminder = picker.getByRole("group", {
      name: "Custom reminder",
      exact: true,
    });
    const reminderUnit = reminder.getByRole("combobox", {
      name: "Reminder unit",
      exact: true,
    });
    await reminderUnit.click();
    const reminderOptions = picker.getByRole("listbox", {
      name: "Reminder unit",
      exact: true,
    });
    await expectWithinViewport(page, reminderOptions);
    await page.screenshot({
      path: "output/playwright/date-reminder-dropdown-mobile.png",
    });
    await reminderOptions
      .getByRole("option", { name: "Hour", exact: true })
      .click();
    await expect(reminderUnit).toHaveText("Hour");
    await reminder
      .getByRole("spinbutton", { name: "Reminder amount", exact: true })
      .fill("2");
    await reminder.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(reminder).toBeHidden();
    await expect(picker).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    expect(await getTask(page, task.id)).toEqual(original);
    await page.reload();
    await expect(
      page.getByRole("heading", { name: "Inbox", exact: true }),
    ).toBeVisible();
    expect(await getTask(page, task.id)).toEqual(original);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    if (task) await call(page.request, "tasks.delete", { id: task.id });
    if (list) await call(page.request, "lists.delete", { id: list.id });
  }
});
