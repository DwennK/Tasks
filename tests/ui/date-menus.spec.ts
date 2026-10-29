import { expect } from "@playwright/test";
import {
  test,
  addTask,
  call,
  getTask,
  localDay,
  openTask,
  resetSettings,
  unique,
} from "./helpers";

test("date submenus preserve calendar geometry, cancel reminder edits and remain usable on mobile", async ({
  page,
}) => {
  await resetSettings(page);
  await page.goto("/");
  const task = await addTask(page, unique("Floating date menus"));
  try {
    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Task date and reminders",
      exact: true,
    });
    const original = (await picker.boundingBox())!;
    expect(original.width).toBe(260);
    expect(original.height).toBe(506);
    const sameHeight = async () =>
      expect((await picker.boundingBox())!.height).toBe(original.height);
    await picker.getByRole("button", { name: "Tomorrow", exact: true }).click();
    await picker.getByRole("button", { name: "Time", exact: true }).click();
    const times = picker.getByRole("listbox", {
      name: "Choose time",
      exact: true,
    });
    await expect(times).toBeVisible();
    await sameHeight();
    await page.screenshot({ path: "output/playwright/date-time-menu.png" });
    await times.getByRole("option", { name: "09:30", exact: true }).click();
    await expect(times).toBeHidden();
    await picker.getByRole("button", { name: "Time", exact: true }).click();
    await picker.getByLabel("Due time", { exact: true }).fill("10:45");
    await picker.getByLabel("Due time", { exact: true }).press("Enter");
    await expect(times).toBeHidden();
    await sameHeight();

    await picker.getByRole("button", { name: "Reminder", exact: true }).click();
    const reminder = picker.locator(".submenu-reminder");
    await reminder
      .getByRole("checkbox", { name: "On time", exact: true })
      .check();
    await reminder
      .getByRole("checkbox", { name: "30 minutes early", exact: true })
      .check();
    await sameHeight();
    await reminder.getByRole("button", { name: "Cancel", exact: true }).click();
    await picker.getByRole("button", { name: "Reminder", exact: true }).click();
    await expect(
      reminder.getByRole("checkbox", { name: "On time", exact: true }),
    ).not.toBeChecked();
    await expect(
      reminder.getByRole("checkbox", {
        name: "30 minutes early",
        exact: true,
      }),
    ).not.toBeChecked();
    await reminder
      .getByRole("checkbox", { name: "On time", exact: true })
      .check();
    await reminder
      .getByRole("checkbox", { name: "30 minutes early", exact: true })
      .check();
    await page.screenshot({ path: "output/playwright/date-reminder-menu.png" });
    await reminder.getByRole("button", { name: "OK", exact: true }).click();
    await expect(reminder).toBeHidden();
    await picker.getByRole("button", { name: "Repeat", exact: true }).click();
    const repeat = picker.getByRole("menu", {
      name: "Repeat frequency",
      exact: true,
    });
    await expect(repeat).toBeVisible();
    await sameHeight();
    await page.screenshot({ path: "output/playwright/date-repeat-menu.png" });
    await repeat.getByRole("menuitem", { name: /^Every Weekday\b/ }).click();
    await expect(repeat).toBeHidden();
    expect((await picker.boundingBox())!.height).toBe(original.height + 36);
    await picker
      .getByRole("button", { name: "End repeat", exact: true })
      .click();
    await picker
      .getByRole("menu", { name: "End repeat options", exact: true })
      .getByRole("menuitem", { name: "End by count", exact: true })
      .click();
    await picker
      .getByRole("spinbutton", { name: "Repeat occurrences", exact: true })
      .fill("3");
    await page.screenshot({
      path: "output/playwright/date-repeat-endcount.png",
    });
    await picker
      .locator(".end-repeat-form")
      .getByRole("button", { name: "OK", exact: true })
      .click();
    await picker.getByRole("button", { name: "OK", exact: true }).click();
    await expect(picker).toBeHidden();
    await expect
      .poll(async () => (await getTask(page, task.id)).due_time)
      .toBe("10:45");
    const saved = await getTask(page, task.id);
    expect(saved.due_date).toBe(localDay(1));
    expect(saved.recurrence).toMatchObject({
      frequency: "weekly",
      weekdays: [1, 2, 3, 4, 5],
      count: 3,
    });
    expect(saved.reminders).toHaveLength(2);
    expect(
      new Date(saved.reminders[1]!).getTime() -
        new Date(saved.reminders[0]!).getTime(),
    ).toBe(30 * 60_000);

    await page.reload();
    await openTask(page, task.title);
    await page.setViewportSize({ width: 390, height: 844 });
    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    await picker.getByRole("button", { name: "Time", exact: true }).click();
    await expect(times).toBeVisible();
    // An off-grid time remains editable while its nearest half-hour slot is
    // brought into view instead of leaving the menu at midnight.
    await expect(
      times.getByRole("option", { name: "10:30", exact: true }),
    ).toBeInViewport({ ratio: 1 });
    await expect(picker.getByLabel("Due time", { exact: true })).toHaveValue(
      "10:45",
    );
    const mobilePopup = (await times.boundingBox())!;
    expect(mobilePopup.x).toBeGreaterThanOrEqual(0);
    expect(mobilePopup.x + mobilePopup.width).toBeLessThanOrEqual(390);
    expect(mobilePopup.y).toBeGreaterThanOrEqual(0);
    expect(mobilePopup.y + mobilePopup.height).toBeLessThanOrEqual(844);
    await page.screenshot({
      path: "output/playwright/date-time-menu-mobile.png",
    });
    await page.keyboard.press("Escape");
    await expect(times).toBeHidden();
    await expect(picker).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(task.title);

    // Native accessibility activation can leave WebKit's focus on BODY.
    // Escape must still dismiss only the uppermost layer, never the editor.
    await page.setViewportSize({ width: 1470, height: 923 });
    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    await picker.getByRole("button", { name: "Repeat", exact: true }).click();
    await expect(repeat).toBeVisible();
    await page.evaluate(() =>
      (document.activeElement as HTMLElement | null)?.blur(),
    );
    await expect(page.locator("body")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(repeat).toBeHidden();
    await expect(picker).toBeVisible();
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(task.title);
    await page.evaluate(() =>
      (document.activeElement as HTMLElement | null)?.blur(),
    );
    await expect(page.locator("body")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(task.title);
  } finally {
    await call(page.request, "tasks.delete", { id: task.id });
  }
});
