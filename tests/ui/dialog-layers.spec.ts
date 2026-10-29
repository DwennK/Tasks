import { expect, type Locator } from "@playwright/test";
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

test("foreground dialogs consume Escape before the date picker or formatting toolbar behind them", async ({
  page,
}) => {
  await resetSettings(page);
  let list: TaskList | undefined;
  let task: Task | undefined;
  try {
    list = await call<TaskList>(page.request, "lists.create", {
      name: unique("Dialog layers"),
    });
    task = await call<Task>(page.request, "tasks.create", {
      title: unique("Preserve background editor layers"),
      list_id: list.id,
      notes: "Opening and dismissing dialogs must preserve this task.",
      due_date: "2030-05-15",
    });
    const original = await getTask(page, task.id);
    const originalContainers = await snapshot(page);
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await openTask(page, task.title);
    const title = page.getByRole("textbox", {
      name: "Task title",
      exact: true,
    });
    const commands = page.getByRole("dialog", {
      name: "Command Menu",
      exact: true,
    });
    const commandInput = commands.getByRole("textbox", {
      name: "Search commands",
      exact: true,
    });
    const closeCommandAbove = async (background: Locator) => {
      await expect(background).toBeVisible();
      // Pointer activation would dismiss the background before this regression.
      await page.keyboard.press("Meta+k");
      await expect(commands).toBeVisible();
      await expect(commandInput).toBeFocused();
      await expect(background).toBeVisible();
      await expect(title).toHaveValue(task!.title);
      await page.keyboard.press("Escape");
      await expect(commands).toBeHidden();
      await expect(background).toBeVisible();
      await expect(title).toHaveValue(task!.title);
    };

    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Task date and reminders",
      exact: true,
    });
    await closeCommandAbove(picker);
    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    await expect(title).toHaveValue(task.title);

    const formattingToggle = page.getByRole("button", {
      name: "Text formatting",
      exact: true,
    });
    const toolbar = page.getByRole("toolbar", {
      name: "Text formatting",
      exact: true,
    });
    await formattingToggle.click();
    await expect(formattingToggle).toHaveAttribute("aria-pressed", "true");
    await closeCommandAbove(toolbar);
    await expect(formattingToggle).toHaveAttribute("aria-pressed", "true");
    await page.keyboard.press("Escape");
    await expect(toolbar).toBeHidden();
    await expect(formattingToggle).toHaveAttribute("aria-pressed", "false");
    await expect(title).toHaveValue(task.title);

    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    await expect(picker).toBeVisible();
    await page.keyboard.press("Meta+,");
    const settings = page.getByRole("dialog", {
      name: "Settings",
      exact: true,
    });
    await expect(settings).toBeVisible();
    await expect(picker).toBeVisible();
    await expect(title).toHaveValue(task.title);
    await page.keyboard.press("Escape");
    await expect(settings).toBeHidden();
    await expect(picker).toBeVisible();
    await expect(title).toHaveValue(task.title);
    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    await expect(title).toHaveValue(task.title);

    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    await expect(picker).toBeVisible();
    await page
      .getByRole("button", { name: "Create list or folder", exact: true })
      .press("Enter");
    await expect(picker).toBeVisible();
    await page
      .getByRole("menuitem", { name: "New List", exact: true })
      .press("Enter");
    const createList = page.getByRole("dialog", {
      name: "Add List",
      exact: true,
    });
    await expect(createList).toBeVisible();
    await expect(
      createList.getByRole("textbox", { name: "List name", exact: true }),
    ).toBeFocused();
    await expect(picker).toBeVisible();
    await expect(title).toHaveValue(task.title);
    await page.keyboard.press("Escape");
    await expect(createList).toBeHidden();
    await expect(picker).toBeVisible();
    await expect(title).toHaveValue(task.title);
    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    await expect(title).toHaveValue(task.title);

    const after = await snapshot(page);
    expect(after.lists.map((item) => item.id).sort()).toEqual(
      originalContainers.lists.map((item) => item.id).sort(),
    );
    expect(after.folders.map((item) => item.id).sort()).toEqual(
      originalContainers.folders.map((item) => item.id).sort(),
    );
    expect(await getTask(page, task.id)).toEqual(original);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    if (task) await call(page.request, "tasks.delete", { id: task.id });
    if (list) await call(page.request, "lists.delete", { id: list.id });
  }
});
