import { expect } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  test,
  call,
  getTask,
  notes,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";

test("Save as Template uses its context task while another selected task is open", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Template context"),
  });
  const first = await call<Task>(page.request, "tasks.create", {
    title: unique("Context template source"),
    list_id: list.id,
    notes: "The context target contains the correct template instructions.",
    priority: 3,
  });
  const second = await call<Task>(page.request, "tasks.create", {
    title: unique("Other open selection"),
    list_id: list.id,
    notes: "Do not capture this other selected task.",
    priority: 1,
  });
  const child = await call<Task>(page.request, "tasks.create", {
    title: unique("Context source child"),
    parent_id: first.id,
  });
  let templateId: string | undefined;
  try {
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await page
      .getByRole("button", { name: `Open task ${first.title}`, exact: true })
      .click();
    await page
      .getByRole("button", { name: `Open task ${second.title}`, exact: true })
      .click({ modifiers: ["Meta"] });
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(second.title);
    await page
      .getByRole("group", { name: first.title, exact: true })
      .click({ button: "right" });
    const menu = page.getByRole("menu", { name: "Task actions", exact: true });
    await expect(menu).toContainText("2 tasks selected");
    await menu
      .getByRole("menuitem", { name: "Save as Template", exact: true })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "Save as Template",
      exact: true,
    });
    await expect(
      dialog.getByRole("textbox", { name: "Template name", exact: true }),
    ).toHaveValue(first.title);
    const name = unique("Correct context template");
    await dialog
      .getByRole("textbox", { name: "Template name", exact: true })
      .fill(name);
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    await expect(dialog).toBeHidden();
    templateId = (await snapshot(page)).templates.find(
      (item) => item.name === name,
    )!.id;
    await page
      .getByRole("button", { name: "List actions", exact: true })
      .click();
    await page
      .getByRole("menu", { name: "List actions", exact: true })
      .getByRole("button", { name, exact: true })
      .click();
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(first.title);
    await expect(notes(page)).toHaveText(first.notes);
    const data = await snapshot(page);
    const applied = data.tasks.find(
      (task) =>
        task.list_id === list.id &&
        task.title === first.title &&
        task.id !== first.id,
    )!;
    expect(applied.priority).toBe(3);
    const children = data.tasks.filter(
      (task) => task.parent_id === applied.id && !task.deleted_at,
    );
    expect(children.map((task) => task.title)).toEqual([child.title]);
    expect((await getTask(page, second.id)).notes).toBe(second.notes);
    expect(
      data.tasks.filter(
        (task) => task.list_id === list.id && task.title === second.title,
      ),
    ).toHaveLength(1);
  } finally {
    if (templateId)
      await call(page.request, "templates.delete", { id: templateId });
    for (const task of (await snapshot(page)).tasks.filter(
      (item) => item.list_id === list.id && !item.parent_id && !item.deleted_at,
    ))
      await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
