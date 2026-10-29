import { expect } from "@playwright/test";
import { test } from "./helpers";
import {
  addTask,
  call,
  getTask,
  localDay,
  notes,
  openTask,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";

test.beforeEach(async ({ page }) => {
  await resetSettings(page);
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
});

test("rich notes render headings and interactive checklists, and attachments round-trip offline", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const task = await addTask(page, unique("Rich editor"));
  const editor = notes(page);
  await editor.fill("/");
  const slash = page.getByRole("menu", { name: "Insert content", exact: true });
  await expect(slash).toBeVisible();
  await expect(slash.getByRole("menuitem")).toHaveCount(12);
  await slash.getByRole("menuitem", { name: "Heading 1", exact: true }).click();
  await editor.pressSequentially("Release checklist");
  await expect(editor.locator("h1")).toHaveText("Release checklist");
  await editor.press("End");
  await editor.press("Enter");
  await editor.pressSequentially("/");
  await slash
    .getByRole("menuitem", { name: "Check Item", exact: true })
    .click();
  await editor.pressSequentially("Verify offline persistence");
  await editor.locator('input[type="checkbox"]').check();
  await expect
    .poll(async () => (await getTask(page, task.id)).notes)
    .toContain('data-checked="true"');
  await editor.press("ControlOrMeta+End");
  await editor.press("Enter");
  await editor.press("Enter");
  await editor.pressSequentially("/");
  const chooserPromise = page.waitForEvent("filechooser");
  await slash
    .getByRole("menuitem", { name: "Attachment", exact: true })
    .click();
  const chooser = await chooserPromise;
  const content = "Offline task attachment — round-trip verified.";
  await chooser.setFiles({
    name: "release-checklist.txt",
    mimeType: "text/plain",
    buffer: Buffer.from(content),
  });
  await expect
    .poll(async () => (await getTask(page, task.id)).attachments.length)
    .toBe(1);
  const attachment = (await getTask(page, task.id)).attachments[0]!;
  const stored = await call(page.request, "attachments.get", {
    id: attachment.id,
  });
  expect(Buffer.from(stored.content_base64, "base64").toString("utf8")).toBe(
    content,
  );
  await page.reload();
  await openTask(page, task.title);
  await expect(notes(page).locator("h1")).toHaveText("Release checklist");
  await expect(notes(page).locator('input[type="checkbox"]')).toBeChecked();
  await expect(
    page.getByText("release-checklist.txt", { exact: true }),
  ).toBeVisible();
  await notes(page).locator('input[type="checkbox"]').uncheck();
  await expect
    .poll(async () => (await getTask(page, task.id)).notes)
    .toContain('data-checked="false"');
  await page.screenshot({ path: "output/playwright/rich-editor.png" });
  expect(errors).toEqual([]);
});

test("calendar duration, repeat and multiple reminders produce exactly one next occurrence", async ({
  page,
}) => {
  const task = await addTask(page, unique("Recurring task"));
  const date = localDay(1),
    nextDate = localDay(2);
  await page.getByRole("button", { name: "Set due date", exact: true }).click();
  const picker = page.getByRole("dialog", {
    name: "Task date and reminders",
    exact: true,
  });
  await expect(picker.getByRole("grid")).toBeVisible();
  const todayCell = picker.locator(`button[data-date="${localDay()}"]`);
  await todayCell.focus();
  await todayCell.press("ArrowRight");
  await expect(picker.locator(`button[data-date="${date}"]`)).toBeFocused();
  await picker.getByRole("tab", { name: "Duration", exact: true }).click();
  await picker.getByLabel("Start date", { exact: true }).fill(date);
  await picker.getByLabel("Start time", { exact: true }).fill("09:00");
  await picker.getByLabel("End date", { exact: true }).fill(date);
  await picker.getByLabel("End time", { exact: true }).fill("10:30");
  await picker.getByRole("button", { name: /^Reminder/ }).click();
  await picker.getByRole("checkbox", { name: "On time", exact: true }).check();
  await picker.getByRole("menuitem", { name: "Custom", exact: true }).click();
  const customReminder = picker.getByRole("group", {
    name: "Custom reminder",
    exact: true,
  });
  await customReminder
    .getByRole("spinbutton", { name: "Reminder amount", exact: true })
    .fill("15");
  await customReminder.getByRole("button", { name: "OK", exact: true }).click();
  await picker.getByRole("button", { name: /^Repeat/ }).click();
  await picker
    .getByRole("menu", { name: "Repeat frequency", exact: true })
    .getByRole("menuitem", { name: "Daily", exact: true })
    .click();
  await picker.getByRole("button", { name: "OK", exact: true }).click();
  await expect(picker).toBeHidden();
  const before = await getTask(page, task.id);
  expect(before.due_date).toBe(date);
  expect(before.duration_minutes).toBe(90);
  expect(before.recurrence?.frequency).toBe("daily");
  expect(before.reminders).toHaveLength(2);
  expect(
    new Date(before.reminders[1]!).getTime() -
      new Date(before.reminders[0]!).getTime(),
  ).toBe(15 * 60000);
  await page
    .getByRole("button", { name: "Complete task", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, task.id)).recurrence_next_id)
    .not.toBeNull();
  const completed = await getTask(page, task.id);
  const next = await getTask(page, completed.recurrence_next_id!);
  expect(next.due_date).toBe(nextDate);
  expect(next.start_date).toBe(nextDate);
  expect(next.completed_at).toBeNull();
  expect(next.duration_minutes).toBe(90);
  expect(next.reminders).toHaveLength(2);
  await call(page.request, "tasks.complete", { id: task.id });
  const matches = (await snapshot(page)).tasks.filter(
    (item) => item.title === task.title,
  );
  expect(matches).toHaveLength(2);
  await page.reload();
  await openTask(page, next.title);
  await page.getByRole("button", { name: "Set due date", exact: true }).click();
  await expect(
    picker.getByRole("tab", { name: "Duration", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(picker.getByLabel("End time", { exact: true })).toHaveValue(
    "10:30",
  );
  await page.screenshot({ path: "output/playwright/calendar-repeat.png" });
});
