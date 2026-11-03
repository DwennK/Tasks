import { expect, type Page } from "@playwright/test";
import {
  test,
  call,
  getTask,
  notes,
  openTask,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";
import type { Task, TaskList } from "../../src/types";

async function fixture(page: Page, count = 1) {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Undo"),
  });
  const tasks: Task[] = [];
  for (let i = 0; i < count; i++)
    tasks.push(
      await call<Task>(page.request, "tasks.create", {
        title: unique(`Undo ${i}`),
        list_id: list.id,
      }),
    );
  await page.goto("/");
  await page.getByRole("treeitem", { name: list.name, exact: true }).click();
  return { list, tasks };
}
const row = (page: Page, task: Task) =>
  page.getByRole("group", { name: task.title, exact: true });
async function context(page: Page, task: Task) {
  await row(page, task).click({ button: "right" });
  return page.getByRole("menu", { name: "Task actions", exact: true });
}
async function undo(page: Page) {
  // Metadata actions may leave focus on the task title. Explicitly test the
  // global shortcut from navigation; focused text has its own undo history.
  await page
    .getByRole("button", { name: "Toggle sidebar", exact: true })
    .focus();
  const response = page.waitForResponse(
    (response) =>
      response.url().endsWith("/__tasks") &&
      response.request().postDataJSON()?.action === "undo.apply",
  );
  await page.keyboard.press("Meta+z");
  await (await response).finished();
  await page.waitForLoadState("networkidle");
}

test("Command Z reverses metadata, completion and deletion in order", async ({
  page,
}) => {
  const {
    tasks: [task],
  } = await fixture(page);
  await (
    await context(page, task)
  )
    .getByRole("button", { name: "High priority", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, task.id)).priority)
    .toBe(3);
  await page
    .getByRole("button", { name: `Complete ${task.title}`, exact: true })
    .click();
  await expect(row(page, task)).toHaveCount(0);
  await undo(page);
  await expect(row(page, task)).toBeVisible();
  expect((await getTask(page, task.id)).priority).toBe(3);
  await undo(page);
  expect((await getTask(page, task.id)).priority).toBe(0);
  await (
    await context(page, task)
  )
    .getByRole("menuitem", { name: /^Delete/ })
    .click();
  await expect(row(page, task)).toHaveCount(0);
  await undo(page);
  await expect(row(page, task)).toBeVisible();
  expect((await getTask(page, task.id)).deleted_at).toBeNull();
  await page.screenshot({ path: "output/playwright/undo-desktop.png" });
});

test("one Command Z reverses an entire multi-selection", async ({ page }) => {
  const {
    tasks: [a, b],
  } = await fixture(page, 2);
  await page
    .getByRole("button", { name: `Open task ${a.title}`, exact: true })
    .click();
  await page
    .getByRole("button", { name: `Open task ${b.title}`, exact: true })
    .click({ modifiers: ["Meta"] });
  await expect(page.locator(".multi-actions")).toContainText("2 selected");
  await page
    .getByRole("button", { name: "Complete selected tasks", exact: true })
    .click();
  await expect(row(page, a)).toHaveCount(0);
  await expect(row(page, b)).toHaveCount(0);
  await undo(page);
  await expect(row(page, a)).toBeVisible();
  await expect(row(page, b)).toBeVisible();
  expect((await getTask(page, a.id)).completed_at).toBeNull();
  expect((await getTask(page, b.id)).completed_at).toBeNull();
});

test("Command Z and native-menu text forwarding preserve input and rich-note history", async ({
  page,
}) => {
  const {
    tasks: [task],
  } = await fixture(page);
  await (
    await context(page, task)
  )
    .getByRole("button", { name: "High priority", exact: true })
    .click();
  await openTask(page, task.title);
  const title = page.getByRole("textbox", { name: "Task title", exact: true });
  await title.focus();
  await title.press("End");
  await title.pressSequentially("x");
  await title.press("Meta+z");
  await expect(title).toHaveValue(task.title);
  await title.pressSequentially("y");
  await page.evaluate(async () => {
    // Exercise the same text routing used by the native Edit > Undo handler.
    const { undoTextInput } = await import("/src/lib/undo-input.ts");
    undoTextInput(document.activeElement);
  });
  await expect(title).toHaveValue(task.title);
  const editor = notes(page);
  await editor.click();
  await editor.pressSequentially("Note text");
  await editor.press("Meta+z");
  await expect(editor).toHaveText("");
  await editor.pressSequentially("Menu note");
  await page.evaluate(async () => {
    const { undoTextInput } = await import("/src/lib/undo-input.ts");
    undoTextInput(document.activeElement);
  });
  await expect(editor).toHaveText("");
  expect((await getTask(page, task.id)).priority).toBe(3);
});

test("creation undo removes only the task created in this window", async ({
  page,
}) => {
  const {
    list,
    tasks: [existing],
  } = await fixture(page);
  const input = page.getByRole("textbox", { name: "Add task", exact: true });
  const title = unique("Created to undo");
  await input.fill(title);
  await input.press("Enter");
  await expect(
    page.getByRole("group", { name: title, exact: true }),
  ).toBeVisible();
  await undo(page);
  await expect(
    page.getByRole("group", { name: title, exact: true }),
  ).toHaveCount(0);
  const saved = (await snapshot(page)).tasks.filter(
    (task) => task.list_id === list.id,
  );
  expect(saved.map((task) => task.id)).toEqual([existing.id]);
});

test("concurrent editor conflicts prevent task undo without losing newer content", async ({
  page,
}) => {
  const {
    tasks: [task],
  } = await fixture(page);
  await (
    await context(page, task)
  )
    .getByRole("button", { name: "High priority", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, task.id)).priority)
    .toBe(3);
  await call(page.request, "tasks.update", {
    id: task.id,
    notes: "Changed by another client",
  });
  await undo(page);
  await expect(page.getByRole("alert")).toContainText("Cannot undo");
  const saved = await getTask(page, task.id);
  expect(saved.priority).toBe(3);
  expect(saved.notes).toBe("Changed by another client");
});

test("undoing debounced metadata preserves text saved in the same draft", async ({
  page,
}) => {
  const {
    tasks: [task],
  } = await fixture(page);
  const tag = await call<{ id: string }>(page.request, "tags.create", {
    name: unique("Undo tag"),
  });
  await openTask(page, task.title);
  // A title/notes edit and tag click can share the 450 ms draft. Exercise that
  // boundary deterministically against the real service, not a timing race.
  await page.evaluate(
    async ({ id, tagId }) => {
      const { useTasks } = await import("/src/composables/useTasks.ts");
      useTasks().editTask(id, {
        notes: "Keep this note",
        notes_format: "plain",
        tag_ids: [tagId],
      });
    },
    { id: task.id, tagId: tag.id },
  );
  await expect
    .poll(async () => (await getTask(page, task.id)).tag_ids)
    .toEqual([tag.id]);
  await undo(page);
  const saved = await getTask(page, task.id);
  expect(saved.tag_ids).toEqual([]);
  expect(saved.notes).toBe("Keep this note");
});
