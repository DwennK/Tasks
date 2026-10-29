import { expect } from "@playwright/test";
import type { Task, TaskList, Tag } from "../../src/types";
import {
  test,
  call,
  getTask,
  localDay,
  resetSettings,
  unique,
} from "./helpers";

test("bulk date updates parent and child while mixed tag edits preserve each task's other tags", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Bulk metadata"),
  });
  const tags: Tag[] = [];
  for (const label of ["Parent-only tag", "Child-only tag", "Common tag"])
    tags.push(
      await call<Tag>(page.request, "tags.create", { name: unique(label) }),
    );
  const [firstTag, secondTag, commonTag] = tags as [Tag, Tag, Tag];
  const parent = await call<Task>(page.request, "tasks.create", {
    title: unique("Bulk metadata parent"),
    list_id: list.id,
    notes: "Keep the parent's independent note.",
    priority: 3,
    tag_ids: [firstTag.id],
  });
  const child = await call<Task>(page.request, "tasks.create", {
    title: unique("Bulk metadata child"),
    parent_id: parent.id,
    notes: "Keep the child's independent note.",
    priority: 1,
    tag_ids: [secondTag.id],
  });
  const openContext = async () => {
    await page
      .getByRole("group", { name: parent.title, exact: true })
      .click({ button: "right" });
    const menu = page.getByRole("menu", { name: "Task actions", exact: true });
    await expect(menu).toContainText("2 tasks selected");
    return menu;
  };
  try {
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await page
      .getByRole("button", {
        name: `Expand subtasks for ${parent.title}`,
        exact: true,
      })
      .click();
    await page
      .getByRole("button", { name: `Open task ${parent.title}`, exact: true })
      .click();
    await page
      .getByRole("button", { name: `Open task ${child.title}`, exact: true })
      .click({ modifiers: ["Meta"] });
    await page.keyboard.press("Meta+d");
    const bulkDate = page.getByRole("dialog", {
      name: "Set date for 2 tasks",
      exact: true,
    });
    const picker = bulkDate.getByRole("dialog", {
      name: "Task date and reminders",
      exact: true,
    });
    await expect(picker).toBeVisible();
    await picker.getByRole("button", { name: "Tomorrow", exact: true }).click();
    await picker.getByRole("button", { name: "Time", exact: true }).click();
    await picker.getByLabel("Due time", { exact: true }).fill("14:30");
    await picker.getByLabel("Due time", { exact: true }).press("Enter");
    await picker.getByRole("button", { name: "OK", exact: true }).click();
    await expect(bulkDate).toBeHidden();
    for (const task of [parent, child]) {
      await expect
        .poll(async () => (await getTask(page, task.id)).due_date)
        .toBe(localDay(1));
      expect(await getTask(page, task.id)).toMatchObject({
        due_time: "14:30",
        parent_id: task.parent_id,
        notes: task.notes,
        priority: task.priority,
      });
    }
    await (
      await openContext()
    )
      .getByRole("menuitem", { name: "Tags", exact: true })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "Tags for 2 tasks",
      exact: true,
    });
    await expect(
      dialog.getByRole("checkbox", { name: firstTag.name, exact: true }),
    ).toHaveJSProperty("indeterminate", true);
    await expect(
      dialog.getByRole("checkbox", { name: secondTag.name, exact: true }),
    ).toHaveJSProperty("indeterminate", true);
    await dialog
      .getByRole("checkbox", { name: commonTag.name, exact: true })
      .check();
    await dialog
      .getByRole("button", { name: "Apply Tags", exact: true })
      .click();
    await expect(dialog).toBeHidden();
    expect((await getTask(page, parent.id)).tag_ids.sort()).toEqual(
      [firstTag.id, commonTag.id].sort(),
    );
    expect((await getTask(page, child.id)).tag_ids.sort()).toEqual(
      [secondTag.id, commonTag.id].sort(),
    );
    await (
      await openContext()
    )
      .getByRole("menuitem", { name: "Tags", exact: true })
      .click();
    await expect(
      dialog.getByRole("checkbox", { name: commonTag.name, exact: true }),
    ).toBeChecked();
    await dialog
      .getByRole("checkbox", { name: commonTag.name, exact: true })
      .uncheck();
    await dialog
      .getByRole("button", { name: "Apply Tags", exact: true })
      .click();
    await expect(dialog).toBeHidden();
    await page.reload();
    expect((await getTask(page, parent.id)).tag_ids).toEqual([firstTag.id]);
    expect((await getTask(page, child.id)).tag_ids).toEqual([secondTag.id]);
    expect(await getTask(page, child.id)).toMatchObject({
      due_date: localDay(1),
      due_time: "14:30",
      parent_id: parent.id,
    });
  } finally {
    await call(page.request, "tasks.delete", { id: parent.id });
    await call(page.request, "lists.delete", { id: list.id });
    for (const tag of tags)
      await call(page.request, "tags.delete", { id: tag.id });
  }
});
