import { expect, type Page } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  test,
  call,
  closeSettings,
  localDay,
  resetSettings,
  settingsCategory,
  snapshot,
  unique,
} from "./helpers";

const count = async (page: Page, name: string) =>
  Number(
    await page
      .locator(".smart-navigation")
      .getByRole("button", { name: new RegExp(`^${name}(?:\\s|$)`) })
      .locator("small")
      .textContent(),
  );

test("Show Hide Note changes sidebar counts while notes remain accessible and persists across reload", async ({
  page,
}) => {
  await resetSettings(page);
  const folder = await call<{ id: string; name: string }>(
    page.request,
    "folders.create",
    { name: unique("Note count folder") },
  );
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Note count list"),
    folder_id: folder.id,
  });
  const tasks: Task[] = [];
  try {
    for (const [label, is_note, due_date] of [
      ["Dated task", false, localDay()],
      ["Undated task", false, null],
      ["Dated note", true, localDay()],
      ["Undated note", true, null],
    ] as const)
      tasks.push(
        await call<Task>(page.request, "tasks.create", {
          title: unique(label),
          is_note,
          due_date,
          list_id: list.id,
        }),
      );
    await page.goto("/");
    const listRow = page.getByRole("treeitem", {
      name: list.name,
      exact: true,
    });
    const folderRow = page.getByRole("treeitem", {
      name: folder.name,
      exact: true,
    });
    await expect(folderRow).toBeVisible();
    if (!(await listRow.isVisible())) await folderRow.click();
    await expect(listRow.locator(".tree-count")).toHaveText("4");
    await expect(folderRow.locator(".tree-count")).toHaveText("4");
    await listRow.click();
    const allBefore = await count(page, "All");
    const todayBefore = await count(page, "Today");
    const revisionBefore = (await snapshot(page)).revision;
    const settings = await settingsCategory(page, "Appearance");
    await settings.getByRole("tab", { name: "Display", exact: true }).click();
    await settings.getByRole("button", { name: /Show\(Hide Note\)$/ }).click();
    await expect
      .poll(async () => (await snapshot(page)).settings.sidebar_count)
      .toBe("tasks");
    await closeSettings(page);
    await expect(listRow.locator(".tree-count")).toHaveText("2");
    await expect(folderRow.locator(".tree-count")).toHaveText("2");
    expect(await count(page, "All")).toBeLessThan(allBefore);
    expect(await count(page, "Today")).toBeLessThan(todayBefore);
    for (const task of tasks)
      await expect(
        page.getByRole("group", { name: task.title, exact: true }),
      ).toBeVisible();
    await page.reload();
    await expect(folderRow).toBeVisible();
    if (!(await listRow.isVisible())) await folderRow.click();
    await expect(listRow.locator(".tree-count")).toHaveText("2");
    await listRow.click();
    for (const task of tasks)
      await expect(
        page.getByRole("group", { name: task.title, exact: true }),
      ).toBeVisible();
    const persisted = await snapshot(page);
    expect(persisted.revision).toBeGreaterThan(revisionBefore);
    expect(
      persisted.tasks
        .filter((task) => tasks.some((original) => original.id === task.id))
        .map((task) => [task.id, task.version, task.is_note])
        .sort(),
    ).toEqual(
      tasks.map((task) => [task.id, task.version, task.is_note]).sort(),
    );
    const after = await settingsCategory(page, "Appearance");
    await after.getByRole("tab", { name: "Display", exact: true }).click();
    await after
      .locator("section")
      .filter({
        has: page.getByRole("heading", { name: "Sidebar Count", exact: true }),
      })
      .getByRole("button", { name: /Hide\(All\)$/ })
      .click();
    await closeSettings(page);
    await expect(listRow.locator(".tree-count")).toHaveCount(0);
    await expect(page.locator(".smart-navigation small")).toHaveCount(0);
    for (const task of tasks)
      await expect(
        page.getByRole("group", { name: task.title, exact: true }),
      ).toBeVisible();
  } finally {
    for (const task of tasks)
      await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
    await call(page.request, "folders.delete", { id: folder.id });
  }
});
