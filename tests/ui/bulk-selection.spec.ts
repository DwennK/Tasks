import { expect, type Page } from "@playwright/test";
import {
  test,
  call,
  getTask,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";
import type { Task, TaskList } from "../../src/types";

const taskRow = (page: Page, task: Task) =>
  page.getByRole("group", { name: task.title, exact: true });
const openButton = (page: Page, task: Task) =>
  page.getByRole("button", { name: `Open task ${task.title}`, exact: true });

async function createList(page: Page, label: string) {
  return call<TaskList>(page.request, "lists.create", { name: unique(label) });
}
async function createTask(
  page: Page,
  list: TaskList,
  label: string,
  parent?: Task,
) {
  return call<Task>(page.request, "tasks.create", {
    title: unique(label),
    list_id: list.id,
    ...(parent ? { parent_id: parent.id } : {}),
  });
}
async function showList(page: Page, list: TaskList) {
  await page.goto("/");
  await page.getByRole("treeitem", { name: list.name, exact: true }).click();
  await expect(
    page.getByRole("heading", { name: list.name, exact: true }),
  ).toBeVisible();
}
async function expand(page: Page, task: Task) {
  await page
    .getByRole("button", {
      name: `Expand subtasks for ${task.title}`,
      exact: true,
    })
    .click();
}
async function selectParentAndChild(page: Page, parent: Task, child: Task) {
  await expand(page, parent);
  await openButton(page, parent).click();
  await openButton(page, child).click({ modifiers: ["Meta"] });
  await expect(page.locator(".multi-actions")).toContainText("2 selected");
}
async function contextFor(page: Page, task: Task) {
  await taskRow(page, task).click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Task actions", exact: true });
  await expect(menu).toContainText("2 tasks selected");
  return menu;
}
async function cleanup(page: Page, lists: TaskList[]) {
  const listIds = new Set(lists.map((list) => list.id));
  const tasks = (await snapshot(page)).tasks.filter(
    (task) => listIds.has(task.list_id) && !task.deleted_at,
  );
  const taskIds = new Set(tasks.map((task) => task.id));
  for (const task of tasks.filter(
    (task) => !task.parent_id || !taskIds.has(task.parent_id),
  ))
    await call(page.request, "tasks.delete", { id: task.id });
  for (const list of lists)
    await call(page.request, "lists.delete", { id: list.id });
}

test.beforeEach(async ({ page }) => {
  await resetSettings(page);
});

test("bulk completion of a selected parent and child does not reopen the completed subtree", async ({
  page,
}) => {
  const list = await createList(page, "Bulk completion");
  try {
    const parent = await createTask(page, list, "Completion parent");
    const child = await createTask(page, list, "Completion child", parent);
    const grandchild = await createTask(
      page,
      list,
      "Completion grandchild",
      child,
    );
    await showList(page, list);
    await selectParentAndChild(page, parent, child);
    await page
      .getByRole("button", { name: "Complete selected tasks", exact: true })
      .click();
    // Let every request in the bulk operation settle, including an erroneous
    // second descendant operation, before reading the final persisted state.
    await page.waitForLoadState("networkidle");
    for (const task of [parent, child, grandchild]) {
      const saved = await getTask(page, task.id);
      expect(saved.completed_at, task.title).not.toBeNull();
      expect(saved.version, `${task.title} should be completed once`).toBe(
        task.version + 1,
      );
    }
    await expect(taskRow(page, parent)).toHaveCount(0);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, [list]);
  }
});

test("bulk duplication of a selected parent and child clones the subtree only once", async ({
  page,
}) => {
  const list = await createList(page, "Bulk duplication");
  try {
    const parent = await createTask(page, list, "Duplicate parent");
    const child = await createTask(page, list, "Duplicate child", parent);
    const grandchild = await createTask(
      page,
      list,
      "Duplicate grandchild",
      child,
    );
    await showList(page, list);
    await selectParentAndChild(page, parent, child);
    const duplication = page.waitForResponse((response) => {
      if (!response.url().endsWith("/__tasks")) return false;
      const request = response.request().postDataJSON();
      return (
        request?.action === "tasks.duplicate" && request.input.id === parent.id
      );
    });
    await (
      await contextFor(page, parent)
    )
      .getByRole("menuitem", { name: "Duplicate", exact: true })
      .click();
    const response = await duplication;
    expect(response.ok()).toBe(true);
    const duplicated = (await response.json()) as Task;
    // networkidle alone may already be satisfied before the async action
    // starts. First observe the real mutation and its selected clone.
    await expect(
      page.locator(
        `[data-task-id="${duplicated.id}"] > .task-tree-line > .task-row`,
      ),
    ).toHaveClass(/selected/);
    await page.waitForLoadState("networkidle");
    const tasks = (await snapshot(page)).tasks.filter(
      (task) => task.list_id === list.id && !task.deleted_at,
    );
    expect(tasks).toHaveLength(6);
    for (const original of [parent, child, grandchild])
      expect(
        tasks.filter((task) => task.title === original.title),
      ).toHaveLength(2);
    const clone = tasks.find(
      (task) => task.title === parent.title && task.id !== parent.id,
    )!;
    const clonedChild = tasks.find(
      (task) => task.title === child.title && task.id !== child.id,
    )!;
    const clonedGrandchild = tasks.find(
      (task) => task.title === grandchild.title && task.id !== grandchild.id,
    )!;
    expect(clone.parent_id).toBeNull();
    expect(clonedChild.parent_id).toBe(clone.id);
    expect(clonedGrandchild.parent_id).toBe(clonedChild.id);
    expect((await getTask(page, child.id)).parent_id).toBe(parent.id);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, [list]);
  }
});

test("bulk moving a selected parent and child preserves descendant relationships", async ({
  page,
}) => {
  const source = await createList(page, "Bulk move source");
  const target = await createList(page, "Bulk move target");
  try {
    const parent = await createTask(page, source, "Move parent");
    const child = await createTask(page, source, "Move child", parent);
    const grandchild = await createTask(page, source, "Move grandchild", child);
    await showList(page, source);
    await selectParentAndChild(page, parent, child);
    const menu = await contextFor(page, parent);
    await menu.getByRole("menuitem", { name: "Move to", exact: true }).click();
    await menu.getByRole("button", { name: target.name, exact: true }).click();
    await page.waitForLoadState("networkidle");
    const parentAfter = await getTask(page, parent.id);
    const childAfter = await getTask(page, child.id);
    const grandchildAfter = await getTask(page, grandchild.id);
    for (const task of [parentAfter, childAfter, grandchildAfter])
      expect(task.list_id).toBe(target.id);
    expect(parentAfter.parent_id).toBeNull();
    expect(childAfter.parent_id).toBe(parent.id);
    expect(grandchildAfter.parent_id).toBe(child.id);
    await page
      .getByRole("treeitem", { name: target.name, exact: true })
      .click();
    await expect(taskRow(page, parent)).toBeVisible();
    await expand(page, parent);
    await expect(taskRow(page, child)).toBeVisible();
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, [source, target]);
  }
});

test("Shift selection follows visible expanded descendants and excludes collapsed descendants", async ({
  page,
}) => {
  const list = await createList(page, "Visible range selection");
  try {
    const parent = await createTask(page, list, "Range parent");
    const child = await createTask(page, list, "Range expanded child", parent);
    const grandchild = await createTask(
      page,
      list,
      "Range visible grandchild",
      child,
    );
    const collapsed = await createTask(
      page,
      list,
      "Range collapsed child",
      parent,
    );
    const hidden = await createTask(
      page,
      list,
      "Range hidden grandchild",
      collapsed,
    );
    const tail = await createTask(page, list, "Range last root");
    await showList(page, list);
    await expand(page, parent);
    await expand(page, child);
    await expect(taskRow(page, hidden)).toHaveCount(0);
    await openButton(page, parent).click();
    await openButton(page, tail).click({ modifiers: ["Shift"] });
    await expect(page.locator(".multi-actions")).toContainText("5 selected");
    const expected = [parent, child, grandchild, collapsed, tail];
    await expect(
      page.locator(".task-list-scroll .task-row.selected"),
    ).toHaveCount(expected.length);
    for (const task of expected)
      await expect(taskRow(page, task)).toHaveClass(/selected/);
    await expect(taskRow(page, hidden)).toHaveCount(0);
    const visibleSelected = await page
      .locator(".task-list-scroll .task-row.selected")
      .evaluateAll((elements) =>
        elements.map((element) => element.getAttribute("aria-label")),
      );
    expect(visibleSelected).toEqual(expected.map((task) => task.title));
    await page.keyboard.press("v");
    await page.keyboard.press("k");
    await expect(page.locator(".task-list-scroll")).toHaveClass(/task-board/);
    await expect
      .poll(
        async () =>
          (await snapshot(page)).lists.find((item) => item.id === list.id)
            ?.view_mode,
      )
      .toBe("board");
    await page.keyboard.press("v");
    await page.keyboard.press("l");
    await expect(page.locator(".task-list-scroll")).not.toHaveClass(
      /task-board/,
    );
    await expect
      .poll(
        async () =>
          (await snapshot(page)).lists.find((item) => item.id === list.id)
            ?.view_mode,
      )
      .toBe("list");
  } finally {
    await cleanup(page, [list]);
  }
});

test("dropping below the final sibling puts a task last and persists that order", async ({
  page,
}) => {
  const list = await createList(page, "Drop after final task");
  try {
    const first = await createTask(page, list, "Order first");
    const middle = await createTask(page, list, "Order middle");
    const last = await createTask(page, list, "Order last");
    await showList(page, list);
    const source = taskRow(page, first),
      target = taskRow(page, last);
    const bounds = (await target.boundingBox())!;
    await source.dragTo(target, {
      targetPosition: { x: bounds.width / 2, y: bounds.height - 2 },
    });
    await expect
      .poll(async () => {
        const tasks = (await snapshot(page)).tasks.filter(
          (task) =>
            task.list_id === list.id && !task.parent_id && !task.deleted_at,
        );
        return tasks
          .sort((a, b) => a.position - b.position)
          .map((task) => task.id);
      })
      .toEqual([middle.id, last.id, first.id]);
    await page.reload();
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    const titles = await page
      .locator(".task-list-scroll .task-row")
      .evaluateAll((elements) =>
        elements.map((element) => element.getAttribute("aria-label")),
      );
    expect(titles).toEqual([middle.title, last.title, first.title]);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, [list]);
  }
});
