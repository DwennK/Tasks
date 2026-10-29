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

test.beforeEach(async ({ page }) => {
  await resetSettings(page);
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
});

test("slash tags and linked tasks work, and a saved filter selects the matching task after reload", async ({
  page,
}) => {
  const tagName = unique("Release");
  await page.getByRole("button", { name: "Add tag", exact: true }).click();
  const tagDialog = page.getByRole("dialog", { name: "Add Tag", exact: true });
  await tagDialog
    .getByRole("textbox", { name: "Tag name", exact: true })
    .fill(tagName);
  await tagDialog.getByRole("button", { name: "Save", exact: true }).click();
  await expect(tagDialog).toBeHidden();
  const tag = (await snapshot(page)).tags.find((tag) => tag.name === tagName)!;
  const first = await addTask(page, unique("Tagged task"));
  const second = await addTask(page, unique("Linked reference"));
  await openTask(page, first.title);
  await notes(page).fill("");
  await notes(page).pressSequentially("/Tag", { delay: 30 });
  await page
    .getByRole("menu", { name: "Insert content", exact: true })
    .getByRole("menuitem", { name: "Tag", exact: true })
    .click();
  const tags = page.getByRole("dialog", { name: "Task tags", exact: true });
  await tags
    .getByRole("textbox", { name: "Search tags", exact: true })
    .fill(tagName);
  await tags.getByRole("button", { name: tagName, exact: true }).click();
  await expect
    .poll(async () => (await getTask(page, first.id)).tag_ids)
    .toContain(tag.id);
  await page.getByRole("textbox", { name: "Task title", exact: true }).click();
  await notes(page).fill("");
  await notes(page).pressSequentially("/Linked", { delay: 30 });
  await page
    .getByRole("menu", { name: "Insert content", exact: true })
    .getByRole("menuitem", { name: "Linked task/note", exact: true })
    .click();
  const link = page.getByRole("dialog", { name: "Link Task", exact: true });
  await link
    .getByRole("textbox", { name: "Search tasks to link", exact: true })
    .fill(second.title);
  await link.getByRole("button", { name: new RegExp(second.title) }).click();
  await expect(link).toBeHidden();
  await expect
    .poll(async () => (await getTask(page, first.id)).linked_task_ids)
    .toContain(second.id);
  expect((await getTask(page, second.id)).linked_task_ids).toContain(first.id);
  await page.getByRole("button", { name: "Set priority", exact: true }).click();
  await page
    .getByRole("menu", { name: "Priority", exact: true })
    .getByRole("menuitem", { name: "High Priority", exact: true })
    .click();
  const filterName = unique("High release work");
  await page.getByRole("button", { name: "Add filter", exact: true }).click();
  const filter = page.getByRole("dialog", { name: "Add Filter", exact: true });
  await filter
    .getByRole("textbox", { name: "Filter name", exact: true })
    .fill(filterName);
  await filter.getByRole("checkbox", { name: tagName, exact: true }).check();
  await filter.getByRole("checkbox", { name: "High", exact: true }).check();
  await filter.getByRole("button", { name: "Save", exact: true }).click();
  await expect(filter).toBeHidden();
  await page.getByRole("button", { name: filterName, exact: true }).click();
  await expect(
    page.getByRole("heading", { name: filterName, exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: `Open task ${first.title}`, exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", {
      name: `Open task ${second.title}`,
      exact: true,
    }),
  ).toBeHidden();
  await page.reload();
  await page.getByRole("button", { name: filterName, exact: true }).click();
  await openTask(page, first.title);
  await expect(
    page.getByRole("button", { name: `Unlink ${second.title}`, exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: `Unlink ${second.title}`, exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, second.id)).linked_task_ids)
    .not.toContain(first.id);
});

test("task context menu pins, duplicates and dismisses tasks without changing the original", async ({
  page,
}) => {
  const task = await addTask(page, unique("Context source"));
  await notes(page).fill("Duplicate must preserve this note.");
  await expect
    .poll(async () => (await getTask(page, task.id)).notes)
    .toContain("Duplicate must preserve this note.");
  const sourceRow = page.getByRole("group", { name: task.title, exact: true });
  await sourceRow.click({ button: "right" });
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Pin", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, task.id)).pinned)
    .toBe(true);
  await sourceRow.click({ button: "right" });
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Duplicate", exact: true })
    .click();
  await expect
    .poll(
      async () =>
        (await snapshot(page)).tasks.filter((item) => item.title === task.title)
          .length,
    )
    .toBe(2);
  const clone = (await snapshot(page)).tasks.find(
    (item) => item.title === task.title && item.id !== task.id,
  )!;
  const cloneTitle = unique("Context duplicate");
  await page
    .getByRole("textbox", { name: "Task title", exact: true })
    .fill(cloneTitle);
  await expect
    .poll(async () => (await getTask(page, clone.id)).title)
    .toBe(cloneTitle);
  expect((await getTask(page, clone.id)).notes).toContain(
    "Duplicate must preserve this note.",
  );
  await page
    .getByRole("group", { name: cloneTitle, exact: true })
    .click({ button: "right" });
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Won’t Do", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, clone.id)).wont_do_at)
    .not.toBeNull();
  expect((await getTask(page, task.id)).wont_do_at).toBeNull();
  await page.getByRole("button", { name: /^Won’t Do(?: \d+)?$/ }).click();
  await expect(
    page.getByRole("button", { name: `Open task ${cloneTitle}`, exact: true }),
  ).toBeVisible();
  await page
    .getByRole("group", { name: cloneTitle, exact: true })
    .click({ button: "right" });
  await page
    .getByRole("menu", { name: "Task actions", exact: true })
    .getByRole("menuitem", { name: "Mark as Undone", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, clone.id)).wont_do_at)
    .toBeNull();
  await page.keyboard.press("Meta+3");
  await expect(
    page.getByRole("button", { name: `Open task ${cloneTitle}`, exact: true }),
  ).toBeVisible();
});

test("list color, sections and board view persist and can move a task between sections", async ({
  page,
}) => {
  const listName = unique("Board list");
  await page.locator(".tree-heading").hover();
  await page
    .getByRole("button", { name: "Create list or folder", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "New List", exact: true }).click();
  const create = page.getByRole("dialog", { name: "Add List", exact: true });
  await create
    .getByRole("textbox", { name: "List name", exact: true })
    .fill(listName);
  await create.getByRole("button", { name: "Blue", exact: true }).click();
  await create.getByRole("button", { name: "Save", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: listName, exact: true }),
  ).toBeVisible();
  const list = (await snapshot(page)).lists.find(
    (list) => list.name === listName,
  )!;
  expect(list.color).toBe("#509bef");
  const task = await addTask(page, unique("Section task"));
  const sections = [unique("Ready"), unique("Finished")];
  for (const name of sections) {
    await page
      .getByRole("button", { name: "List actions", exact: true })
      .click();
    await page
      .getByRole("menu", { name: "List actions", exact: true })
      .getByRole("button", { name: "Add Section", exact: true })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "Add Section",
      exact: true,
    });
    await dialog
      .getByRole("textbox", { name: "Section name", exact: true })
      .fill(name);
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    await expect(dialog).toBeHidden();
  }
  await page.getByRole("button", { name: "Move to list", exact: true }).click();
  await page
    .getByRole("menu", { name: "Move to list", exact: true })
    .getByRole("button", { name: sections[0]!, exact: true })
    .click();
  const firstSection = (await snapshot(page)).sections.find(
    (section) => section.name === sections[0],
  )!;
  await expect
    .poll(async () => (await getTask(page, task.id)).section_id)
    .toBe(firstSection.id);
  await page.getByRole("treeitem", { name: list.name, exact: true }).click();
  await page.getByRole("button", { name: "List actions", exact: true }).click();
  await page
    .getByRole("menu", { name: "List actions", exact: true })
    .getByRole("button", { name: "Kanban View", exact: true })
    .click();
  await expect
    .poll(
      async () =>
        (await snapshot(page)).lists.find((item) => item.id === list.id)
          ?.view_mode,
    )
    .toBe("board");
  await expect(page.locator(".app-frame")).toHaveClass(/board-layout/);
  const target = page
    .locator(".task-group")
    .filter({ has: page.getByText(sections[1]!, { exact: true }) });
  await page
    .getByRole("group", { name: task.title, exact: true })
    .dragTo(target, { targetPosition: { x: 50, y: 100 } });
  const secondSection = (await snapshot(page)).sections.find(
    (section) => section.name === sections[1],
  )!;
  await expect
    .poll(async () => (await getTask(page, task.id)).section_id)
    .toBe(secondSection.id);
  await page.reload();
  await page.getByRole("treeitem", { name: listName, exact: true }).click();
  await expect(page.locator(".app-frame")).toHaveClass(/board-layout/);
  await expect(
    target.getByRole("button", {
      name: `Open task ${task.title}`,
      exact: true,
    }),
  ).toBeVisible();
  await page.screenshot({ path: "output/playwright/board-sections.png" });
});
