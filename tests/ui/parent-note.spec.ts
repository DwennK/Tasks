import { expect } from "@playwright/test";
import {
  test,
  addTask,
  call,
  getTask,
  notes,
  openTask,
  resetSettings,
  unique,
} from "./helpers";

test("More links a parent across lists, rejects cycles in the picker and converts notes without losing content", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call(page.request, "lists.create", {
    name: unique("Parent destination"),
  });
  const parent = await call(page.request, "tasks.create", {
    title: unique("Parent task"),
    list_id: list.id,
  });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  const child = await addTask(page, unique("Relink child"));
  const body = "Moving and converting this task must preserve these notes.";
  await notes(page).fill(body);
  await expect
    .poll(async () => (await getTask(page, child.id)).notes)
    .toContain(body);
  await page.getByRole("button", { name: "Task actions", exact: true }).click();
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Link Parent Task", exact: true })
    .click();
  const picker = page.getByRole("dialog", {
    name: "Link Parent Task",
    exact: true,
  });
  await expect(picker).toBeVisible();
  await page.screenshot({
    path: "output/playwright/parent-picker-integrated.png",
  });
  await picker
    .getByRole("textbox", { name: "Find parent task", exact: true })
    .fill(parent.title);
  await picker
    .getByRole("button", {
      name: `Link parent task ${parent.title}`,
      exact: true,
    })
    .click();
  await expect
    .poll(async () => {
      const task = await getTask(page, child.id);
      return [task.parent_id, task.list_id];
    })
    .toEqual([parent.id, list.id]);
  await page.getByRole("treeitem", { name: list.name, exact: true }).click();
  await openTask(page, parent.title);
  await page.getByRole("button", { name: "Task actions", exact: true }).click();
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Link Parent Task", exact: true })
    .click();
  await picker
    .getByRole("textbox", { name: "Find parent task", exact: true })
    .fill(child.title);
  await expect(
    picker.getByText("No matching tasks", { exact: true }),
  ).toBeVisible();
  await expect(
    picker.getByRole("button", {
      name: `Link parent task ${child.title}`,
      exact: true,
    }),
  ).toBeHidden();
  await picker
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await page
    .getByRole("button", {
      name: `Expand subtasks for ${parent.title}`,
      exact: true,
    })
    .click();
  await openTask(page, child.title);
  await page.getByRole("button", { name: "Task actions", exact: true }).click();
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Link Parent Task", exact: true })
    .click();
  await picker
    .getByRole("button", { name: "Move to top level", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, child.id)).parent_id)
    .toBeNull();
  await page.getByRole("button", { name: "Task actions", exact: true }).click();
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Convert to Note", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, child.id)).is_note)
    .toBe(true);
  await expect(
    page.getByRole("button", { name: "Complete task", exact: true }),
  ).toBeHidden();
  await page.reload();
  await page.getByRole("treeitem", { name: list.name, exact: true }).click();
  await openTask(page, child.title);
  await expect(notes(page)).toHaveText(body);
  await expect(
    page.getByRole("button", { name: "Complete task", exact: true }),
  ).toBeHidden();
  await page.getByRole("button", { name: "Task actions", exact: true }).click();
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Convert to Task", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, child.id)).is_note)
    .toBe(false);
  await expect(
    page.getByRole("button", { name: "Complete task", exact: true }),
  ).toBeVisible();
});
