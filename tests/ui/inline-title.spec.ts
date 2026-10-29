import { expect, type Page } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  test,
  call,
  cliCall,
  getTask,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";

const row = (page: Page, task: Task) =>
  page.locator(`[data-task-id="${task.id}"] > .task-tree-line > .task-row`);
const inlineTitle = (page: Page, task: Task) =>
  row(page, task).getByRole("textbox", {
    name: "Edit task title",
    exact: true,
  });
const detailTitle = (page: Page) =>
  page.getByRole("textbox", { name: "Task title", exact: true });
const belowInput = (page: Page) =>
  page.getByRole("textbox", { name: /^New task below / });

async function activeTasks(page: Page, list: TaskList) {
  return (await snapshot(page)).tasks.filter(
    (task) => task.list_id === list.id && !task.deleted_at,
  );
}
async function createList(page: Page, label: string) {
  return call<TaskList>(page.request, "lists.create", {
    name: unique(label),
    sort_by: "manual",
  });
}
async function createTask(page: Page, list: TaskList, label: string) {
  return call<Task>(page.request, "tasks.create", {
    title: unique(label),
    list_id: list.id,
  });
}
async function navigateList(page: Page, list: TaskList) {
  await page.getByRole("treeitem", { name: list.name, exact: true }).click();
  await expect(
    page.getByRole("heading", { name: list.name, exact: true }),
  ).toBeVisible();
}
async function startEditing(page: Page, task: Task) {
  await row(page, task)
    .getByRole("button", { name: /^Open task / })
    .click();
  await expect(inlineTitle(page, task)).toBeFocused();
  await expect(detailTitle(page)).toBeVisible();
  return inlineTitle(page, task);
}
async function cleanup(page: Page, list: TaskList) {
  const tasks = await activeTasks(page, list);
  const ids = new Set(tasks.map((task) => task.id));
  for (const task of tasks.filter(
    (task) => !task.parent_id || !ids.has(task.parent_id),
  ))
    await call(page.request, "tasks.delete", { id: task.id });
  await call(page.request, "lists.delete", { id: list.id });
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await resetSettings(page);
});

test("a task title edits inline, stays synchronized with detail, and Enter opens a cancellable sibling", async ({
  page,
}, info) => {
  const list = await createList(page, "Inline title persistence");
  try {
    const task = await createTask(page, list, "Original inline title");
    await page.goto("/");
    await navigateList(page, list);
    const input = await startEditing(page, task);
    await expect(input).toHaveValue(task.title);
    await expect(detailTitle(page)).toHaveValue(task.title);

    const firstRename = unique("Renamed from row");
    await input.fill(firstRename);
    await expect(detailTitle(page)).toHaveValue(firstRename);
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(firstRename);
    await info.attach("inline-title-desktop", {
      body: await page.screenshot(),
      contentType: "image/png",
    });

    const detailRename = unique("Renamed from detail");
    await detailTitle(page).fill(detailRename);
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(detailRename);
    await expect(
      row(page, task).getByRole("button", {
        name: `Open task ${detailRename}`,
        exact: true,
      }),
    ).toBeVisible();
    await startEditing(page, task);
    await expect(input).toHaveValue(detailRename);

    const committedRename = unique("Committed inline title");
    await input.fill(committedRename);
    await input.press("Enter");
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(committedRename);
    await expect(detailTitle(page)).toHaveValue(committedRename);
    await expect(belowInput(page)).toBeFocused();
    await belowInput(page).fill(unique("Cancelled inline sibling"));
    await belowInput(page).press("Escape");
    await expect(belowInput(page)).toHaveCount(0);
    expect((await activeTasks(page, list)).map((item) => item.id)).toEqual([
      task.id,
    ]);

    const escapedRename = unique("Escape keeps the edited title");
    await startEditing(page, task);
    await input.fill(escapedRename);
    await input.press("Escape");
    await expect(input).toHaveCount(0);
    await expect(row(page, task)).toHaveClass(/selected/);
    await expect(detailTitle(page)).toHaveValue(escapedRename);
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(escapedRename);
    const beforeSecondEscape = await snapshot(page);
    await page.keyboard.press("Escape");
    await expect(row(page, task)).toHaveClass(/selected/);
    await expect(detailTitle(page)).toHaveValue(escapedRename);
    expect((await snapshot(page)).revision).toBe(beforeSecondEscape.revision);

    await page.reload();
    await navigateList(page, list);
    await startEditing(page, task);
    await expect(input).toHaveValue(escapedRename);
    await expect(detailTitle(page)).toHaveValue(escapedRename);
    expect((await activeTasks(page, list)).map((item) => item.id)).toEqual([
      task.id,
    ]);

    await input.press("Escape");
    await page.setViewportSize({ width: 390, height: 844 });
    const mobileRename = unique("Edited in the mobile detail");
    await detailTitle(page).fill(mobileRename);
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(mobileRename);
    await page
      .getByRole("button", { name: "Back to tasks", exact: true })
      .click();
    const mobileButton = row(page, task).getByRole("button", {
      name: `Open task ${mobileRename}`,
      exact: true,
    });
    await expect(mobileButton).toBeVisible();
    await mobileButton.click();
    await expect(detailTitle(page)).toHaveValue(mobileRename);
    await expect(input).toHaveCount(0);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await info.attach("inline-title-mobile-detail", {
      body: await page.screenshot(),
      contentType: "image/png",
    });

    await page.setViewportSize({ width: 1470, height: 923 });
    await page
      .getByRole("button", { name: "Search tasks", exact: true })
      .click();
    const search = page.getByRole("textbox", {
      name: "Search tasks and notes",
      exact: true,
    });
    await search.fill(mobileRename);
    await page.getByRole("dialog", { name: "Search", exact: true })
      .getByRole("button", { name: `Open task ${mobileRename}`, exact: true }).click();
    await expect(row(page, task)).toBeVisible();
    await startEditing(page, task);
    const unmatchedTitle = unique("The search no longer matches");
    await input.fill(unmatchedTitle);
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(unmatchedTitle);
    await expect(input).toBeVisible();
    await expect(input).toBeFocused();
    await expect(input).toHaveValue(unmatchedTitle);
    await page.getByRole("button", { name: "Search tasks", exact: true }).click();
    await search.fill(mobileRename);
    await expect(page.getByRole("dialog", { name: "Search", exact: true })).toContainText("No results found");
    await page.keyboard.press("Escape");
    expect((await activeTasks(page, list)).map((item) => item.id)).toEqual([
      task.id,
    ]);

    const sortingAnchor = await createTask(page, list, "AAA title before blur");
    const checkboxTarget = await createTask(page, list, "MMM checkbox target");
    await page.reload();
    await navigateList(page, list);
    await page.getByRole("button", { name: "Sort tasks", exact: true }).click();
    const sortMenu = page.getByRole("menu", {
      name: "Sort and group tasks",
      exact: true,
    });
    await sortMenu.getByRole("button", { name: "Title", exact: true }).click();
    await expect
      .poll(
        async () =>
          (await snapshot(page)).lists.find((item) => item.id === list.id)
            ?.sort_by,
      )
      .toBe("title");
    await page.keyboard.press("Escape");
    await expect(sortMenu).toBeHidden();
    const sortingInput = await startEditing(page, sortingAnchor);
    const lastTitle = unique("ZZZ title after blur");
    await sortingInput.fill(lastTitle);
    await expect
      .poll(async () => (await getTask(page, sortingAnchor.id)).title)
      .toBe(lastTitle);
    await expect(
      page.locator(".task-list-scroll .task-row").first(),
    ).toHaveAttribute("aria-label", lastTitle);
    const checkbox = row(page, checkboxTarget).getByRole("button", {
      name: `Complete ${checkboxTarget.title}`,
      exact: true,
    });
    const bounds = (await checkbox.boundingBox())!;
    // Use the actual target coordinates: reordering on blur must not move a
    // different task under this pointer between pointerdown and click.
    await page.mouse.click(
      bounds.x + bounds.width / 2,
      bounds.y + bounds.height / 2,
    );
    await expect
      .poll(async () => (await getTask(page, checkboxTarget.id)).completed_at)
      .not.toBeNull();
    expect((await getTask(page, sortingAnchor.id)).completed_at).toBeNull();
    expect((await getTask(page, task.id)).completed_at).toBeNull();
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, list);
  }
});

test("modifier title clicks keep multi-selection and IME Enter does not save or add a sibling prematurely", async ({
  page,
}) => {
  const list = await createList(page, "Inline selection and IME");
  try {
    const first = await createTask(page, list, "Selection first");
    const middle = await createTask(page, list, "Selection middle");
    const last = await createTask(page, list, "Selection last");
    await page.clock.install({ time: new Date("2031-01-15T12:00:00+01:00") });
    await page.goto("/");
    await navigateList(page, list);
    await startEditing(page, first);
    await row(page, middle)
      .getByRole("button", { name: /^Open task / })
      .click({ modifiers: ["Meta"] });
    await expect(page.locator(".multi-actions")).toContainText("2 selected");
    await expect(row(page, first)).toHaveClass(/selected/);
    await expect(row(page, middle)).toHaveClass(/selected/);
    await expect(
      page.getByRole("textbox", { name: "Edit task title", exact: true }),
    ).toHaveCount(0);
    await expect(belowInput(page)).toHaveCount(0);

    await startEditing(page, first);
    await row(page, last)
      .getByRole("button", { name: /^Open task / })
      .click({ modifiers: ["Shift"] });
    await expect(page.locator(".multi-actions")).toContainText("3 selected");
    for (const task of [first, middle, last])
      await expect(row(page, task)).toHaveClass(/selected/);
    await expect(
      page.getByRole("textbox", { name: "Edit task title", exact: true }),
    ).toHaveCount(0);

    const input = await startEditing(page, first);
    await page.clock.pauseAt(new Date("2031-01-15T12:01:00+01:00"));
    const beforeComposition = await getTask(page, first.id);
    const updates: string[] = [];
    page.on("request", (request) => {
      if (request.method() !== "POST" || !request.url().endsWith("/__tasks"))
        return;
      const body = request.postDataJSON();
      if (["tasks.update", "tasks.create"].includes(body?.action))
        updates.push(body.action);
    });
    const composedTitle = unique("日本語のタスク");
    // These are real DOM composition events, not a system IME simulation or
    // a stub of the service. Enter must leave the active composition alone.
    await input.dispatchEvent("compositionstart", { data: "日本語" });
    await input.fill(composedTitle);
    await input.dispatchEvent("keydown", {
      key: "Enter",
      code: "Enter",
      isComposing: true,
      keyCode: 229,
      bubbles: true,
      cancelable: true,
    });
    await page.clock.runFor(600);
    await expect(input).toBeFocused();
    await expect(input).toHaveValue(composedTitle);
    await expect(belowInput(page)).toHaveCount(0);
    expect(await getTask(page, first.id)).toEqual(beforeComposition);
    expect(updates).toEqual([]);

    await input.dispatchEvent("compositionend", { data: composedTitle });
    await input.press("Enter");
    await expect
      .poll(async () => (await getTask(page, first.id)).title)
      .toBe(composedTitle);
    await expect(detailTitle(page)).toHaveValue(composedTitle);
    await expect(belowInput(page)).toBeFocused();
    await belowInput(page).press("Escape");
    expect(
      (await activeTasks(page, list)).map((item) => item.id).sort(),
    ).toEqual([first.id, middle.id, last.id].sort());
    expect(updates.filter((action) => action === "tasks.create")).toEqual([]);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, list);
  }
});

test("inline concurrent editor conflicts during IME preserve the shared title draft until an explicit Retry", async ({
  page,
}) => {
  const list = await createList(page, "Inline conflicting writer");
  try {
    const task = await createTask(page, list, "Original concurrent title");
    await page.clock.install({ time: new Date("2031-01-15T12:00:00+01:00") });
    await page.goto("/");
    await navigateList(page, list);
    const input = await startEditing(page, task);
    // Hold the clock while composing, then let the normal revision poll load
    // a competing CLI write. Committing must retain the version from the start
    // of composition instead of silently overwriting the newer title.
    await page.clock.pauseAt(new Date("2031-01-15T12:01:00+01:00"));
    const localTitle = unique("Local title draft survives");
    await input.dispatchEvent("compositionstart", { data: "日本語" });
    await input.fill(localTitle);
    const externalTitle = unique("CLI competing title");
    const externalNote = "Independent note written by the real CLI.";
    const external = cliCall<Task>("tasks.update", {
      id: task.id,
      title: externalTitle,
      notes: externalNote,
    });
    expect(external.title).toBe(externalTitle);
    const refreshed = page.waitForResponse((response) => {
      const request = response.request();
      return (
        request.method() === "POST" &&
        request.url().endsWith("/__tasks") &&
        request.postDataJSON()?.action === "snapshot" &&
        response.ok()
      );
    });
    await page.clock.runFor(2_600);
    await refreshed;
    await expect(
      page.getByRole("textbox", { name: "Task notes", exact: true }),
    ).toHaveText(externalNote);
    await expect(input).toHaveValue(localTitle);
    await expect(input).toBeFocused();
    await input.dispatchEvent("compositionend", { data: localTitle });
    await input.press("Enter");
    await expect(page.getByRole("alert")).toContainText("Your draft is kept");
    await expect(belowInput(page)).toHaveCount(0);
    await expect(input).toHaveValue(localTitle);
    await expect(detailTitle(page)).toHaveValue(localTitle);
    expect(await getTask(page, task.id)).toMatchObject({
      title: externalTitle,
      notes: externalNote,
    });
    await page.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(page.getByRole("alert")).toHaveCount(0);
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(localTitle);
    expect(await getTask(page, task.id)).toMatchObject({
      notes: externalNote,
      version: external.version + 1,
    });
    await expect(detailTitle(page)).toHaveValue(localTitle);
    await page.reload();
    await navigateList(page, list);
    await startEditing(page, task);
    await expect(input).toHaveValue(localTitle);
    await expect(detailTitle(page)).toHaveValue(localTitle);
    expect((await activeTasks(page, list)).map((item) => item.id)).toEqual([
      task.id,
    ]);

    // If the CLI deletes during an empty IME edit, unmount must retain that
    // empty draft locally. Restore is a separate explicit action, never an
    // implicit attempt to save an invalid title back into the deleted task.
    await input.dispatchEvent("compositionstart", { data: "" });
    await input.fill("");
    cliCall("tasks.delete", { id: task.id });
    const deleted = await getTask(page, task.id);
    expect(deleted.deleted_at).not.toBeNull();
    const deletionRefresh = page.waitForResponse((response) => {
      const request = response.request();
      return (
        request.method() === "POST" &&
        request.url().endsWith("/__tasks") &&
        request.postDataJSON()?.action === "snapshot" &&
        response.ok()
      );
    });
    await page.clock.runFor(2_600);
    await deletionRefresh;
    await expect(input).toHaveCount(0);
    await expect(detailTitle(page)).toHaveValue("");
    await expect(detailTitle(page)).toHaveJSProperty("readOnly", true);
    await expect(page.getByRole("alert")).toContainText(
      "Your title draft is kept",
    );
    const attemptedUpdates: string[] = [];
    page.on("request", (request) => {
      if (request.method() !== "POST" || !request.url().endsWith("/__tasks"))
        return;
      const body = request.postDataJSON();
      if (body?.action === "tasks.update" && body.input?.id === task.id)
        attemptedUpdates.push(body.action);
    });
    await page
      .locator(".task-editor .trash-notice")
      .getByRole("button", { name: "Restore", exact: true })
      .click();
    await expect
      .poll(async () => (await getTask(page, task.id)).deleted_at)
      .toBeNull();
    const restored = await getTask(page, task.id);
    expect(restored).toMatchObject({ title: localTitle, notes: externalNote });
    expect(attemptedUpdates).toEqual([]);
    await expect(detailTitle(page)).toHaveValue("");
    await expect(detailTitle(page)).toBeEditable();
    const correctedTitle = unique("Corrected after explicit restore");
    await detailTitle(page).fill(correctedTitle);
    await page.clock.runFor(500);
    await expect(page.getByRole("alert")).toContainText("Your draft is kept");
    await page.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(page.getByRole("alert")).toHaveCount(0);
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(correctedTitle);
    expect(await getTask(page, task.id)).toMatchObject({
      deleted_at: null,
      notes: externalNote,
      version: restored.version + 1,
    });
    await expect(detailTitle(page)).toHaveValue(correctedTitle);
    expect((await activeTasks(page, list)).map((item) => item.id)).toEqual([
      task.id,
    ]);
  } finally {
    await cleanup(page, list);
  }
});
