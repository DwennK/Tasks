import { expect } from "@playwright/test";
import { test } from "./helpers";
import {
  addTask,
  getTask,
  notes,
  openTask,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";

test("command navigation, editable comments and reusable templates survive a reload", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await resetSettings(page);
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Meta+k");
  const commands = page.getByRole("dialog", {
    name: "Command Menu",
    exact: true,
  });
  await commands
    .getByRole("textbox", { name: "Search commands", exact: true })
    .fill("Today");
  await commands
    .getByRole("button", { name: "Go to Today", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Today", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Meta+3");
  const task = await addTask(page, unique("Template source"));
  await notes(page).fill("Reusable offline launch instructions.");
  await expect
    .poll(async () => (await getTask(page, task.id)).notes)
    .toContain("Reusable offline launch instructions.");
  await page.getByRole("button", { name: "Comments", exact: true }).click();
  const comment = unique("Local review");
  await page
    .getByRole("textbox", { name: "Write a comment", exact: true })
    .fill(comment);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect
    .poll(async () =>
      (await getTask(page, task.id)).comments.map((item) => item.body),
    )
    .toContain(comment);
  await page
    .getByRole("button", { name: `Edit comment ${comment}`, exact: true })
    .click();
  const revised = `${comment} revised`;
  await page
    .getByRole("textbox", { name: "Write a comment", exact: true })
    .fill(revised);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect
    .poll(async () =>
      (await getTask(page, task.id)).comments.map((item) => item.body),
    )
    .toEqual([revised]);
  await page.reload();
  await openTask(page, task.title);
  await page.getByRole("button", { name: "Comments", exact: true }).click();
  await expect(page.getByText(revised, { exact: true })).toBeVisible();
  await page
    .getByRole("button", { name: `Delete comment ${revised}`, exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, task.id)).comments.length)
    .toBe(0);

  await page
    .getByRole("group", { name: task.title, exact: true })
    .click({ button: "right" });
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Save as Template", exact: true })
    .click();
  const template = page.getByRole("dialog", {
    name: "Save as Template",
    exact: true,
  });
  const templateName = unique("Offline release");
  await template
    .getByRole("textbox", { name: "Template name", exact: true })
    .fill(templateName);
  await template.getByRole("button", { name: "Save", exact: true }).click();
  await expect
    .poll(async () =>
      (await snapshot(page)).templates.some(
        (item) => item.name === templateName,
      ),
    )
    .toBe(true);
  await page.reload();
  await page.getByRole("button", { name: "List actions", exact: true }).click();
  await page
    .getByRole("menu", { name: "List actions", exact: true })
    .getByRole("button", { name: templateName, exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(task.title);
  await expect(notes(page)).toHaveText("Reusable offline launch instructions.");
  const applied = (await snapshot(page)).tasks.find(
    (item) => item.title === task.title && item.id !== task.id,
  )!;
  expect(applied).toBeTruthy();
  await notes(page).fill("Changed only this instance.");
  await expect
    .poll(async () => (await getTask(page, applied.id)).notes)
    .toContain("Changed only this instance.");
  expect((await getTask(page, task.id)).notes).toContain(
    "Reusable offline launch instructions.",
  );
  expect(errors).toEqual([]);
});
