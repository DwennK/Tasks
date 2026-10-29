import { test, resetSettings } from "./helpers";
import { expect, type Page, type APIRequestContext } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

const run = Date.now().toString(36);
const db = resolve("output/ui-test.sqlite3");
const cli = resolve("target/debug/tasks");
type Task = {
  id: string;
  title: string;
  notes: string;
  list_id: string;
  parent_id: string | null;
  due_date: string | null;
  due_time: string | null;
  priority: number;
  completed_at: string | null;
  deleted_at: string | null;
  position: number;
};

async function service(
  request: APIRequestContext,
  action: string,
  input: Record<string, unknown> = {},
): Promise<any> {
  const response = await request.post("/__tasks", { data: { action, input } });
  expect(response.ok(), await response.text()).toBeTruthy();
  return response.json();
}
async function snapshot(page: Page) {
  return service(page.request, "snapshot");
}
async function taskByTitle(
  page: Page,
  title: string,
): Promise<Task | undefined> {
  return (await snapshot(page)).tasks.find(
    (task: Task) => task.title === title,
  );
}
async function add(page: Page, title: string) {
  const input = page.getByRole("textbox", { name: "Add task", exact: true });
  await input.fill(title);
  await input.press("Enter");
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
}
async function newContainer(
  page: Page,
  kind: "Folder" | "List",
  name: string,
  parent?: string,
) {
  if (parent) {
    const folder = (await snapshot(page)).folders.find(
      (item: any) => item.id === parent,
    );
    await page
      .getByRole("treeitem", { name: folder.name, exact: true })
      .hover();
    await page
      .getByRole("button", { name: `Add to ${folder.name}`, exact: true })
      .click();
  } else {
    await page.locator(".tree-heading").hover();
    await page
      .getByRole("button", { name: "Create list or folder", exact: true })
      .click();
  }
  await page
    .getByRole("menuitem", { name: `New ${kind}`, exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: `Add ${kind}`, exact: true });
  await dialog
    .getByRole("textbox", { name: `${kind} name`, exact: true })
    .fill(name);
  await dialog.getByRole("button", { name: "Save", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("treeitem", { name, exact: true })).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await resetSettings(page);
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
});

test("task lifecycle persists notes, date, priority, subtasks, completion and recoverable trash", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const title = `UI lifecycle ${run}`,
    child = `UI child ${run}`,
    notes = "Release checklist\n- Verify desktop\n- Verify keyboard";
  const today = new Date();
  const date = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
  await page.keyboard.press("Meta+n");
  const quick = page.getByRole("dialog", { name: "Add Task", exact: true });
  await expect(quick).toBeVisible();
  await quick
    .getByRole("textbox", { name: "Quick task title", exact: true })
    .fill(title);
  await quick
    .getByRole("button", { name: "Quick due date", exact: true })
    .click();
  const quickDate = page.getByRole("dialog", {
    name: "Task date and reminders",
    exact: true,
  });
  await quickDate.getByRole("button", { name: "Today", exact: true }).click();
  await quickDate.getByRole("button", { name: "OK", exact: true }).click();
  await expect(quickDate).toBeHidden();
  await quick
    .getByRole("button", { name: "Quick priority", exact: true })
    .click();
  await page
    .getByRole("menuitemradio", { name: "High Priority", exact: true })
    .click();
  await quick.getByRole("button", { name: "Add Task", exact: true }).click();
  await expect(quick).toBeHidden();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
  await page
    .getByRole("textbox", { name: "Task notes", exact: true })
    .fill(notes);
  await expect
    .poll(async () => (await taskByTitle(page, title))?.notes)
    .toContain("Verify keyboard");
  await page.getByRole("button", { name: "Add subtask", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Add subtask", exact: true })
    .fill(child);
  await page
    .getByRole("textbox", { name: "Add subtask", exact: true })
    .press("Enter");
  await expect(
    page.getByRole("button", { name: child, exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: `Expand subtasks for ${title}`, exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: `Open task ${child}`, exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", {
      name: `Collapse subtasks for ${title}`,
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("button", { name: `Open task ${child}`, exact: true }),
  ).toBeHidden();
  await page.getByRole("button", { name: "Set due date", exact: true }).click();
  const dateDialog = page.getByRole("dialog", {
    name: "Task date and reminders",
    exact: true,
  });
  await dateDialog.getByRole("button", { name: /^Time/ }).click();
  await dateDialog.getByLabel("Due time", { exact: true }).fill("14:30");
  await dateDialog.getByLabel("Due time", { exact: true }).press("Enter");
  await dateDialog.getByRole("button", { name: "OK", exact: true }).click();
  await expect
    .poll(async () => (await taskByTitle(page, title))?.due_time)
    .toBe("14:30");
  expect((await taskByTitle(page, title))?.priority).toBe(3);
  await page.reload();
  await page
    .getByRole("button", { name: `Open task ${title}`, exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Task notes", exact: true }),
  ).toContainText("Verify keyboard");
  await page
    .getByRole("button", { name: "Complete task", exact: true })
    .click();
  await expect
    .poll(async () => Boolean((await taskByTitle(page, child))?.completed_at))
    .toBe(true);
  await page.getByRole("button", { name: /^Completed(?: \d+)?$/ }).click();
  await page
    .getByRole("button", { name: `Open task ${title}`, exact: true })
    .click();
  await page
    .getByRole("button", { name: "1 completed task", exact: true })
    .click();
  await page
    .getByRole("button", { name: `Reopen ${child}`, exact: true })
    .click();
  await expect
    .poll(async () => (await taskByTitle(page, title))?.completed_at)
    .toBeNull();
  await page.keyboard.press("Meta+3");
  await page
    .getByRole("button", { name: `Open task ${title}`, exact: true })
    .click();
  await page.getByRole("button", { name: "Task actions", exact: true }).click();
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await page.getByRole("button", { name: /^Trash(?: \d+)?$/ }).click();
  await expect(
    page.getByRole("button", { name: `Open task ${title}`, exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: `Restore ${title}`, exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: `Open task ${title}`, exact: true }),
  ).toBeHidden();
  await page.keyboard.press("Meta+f");
  const search = page.getByRole("textbox", {
    name: "Search tasks and notes",
    exact: true,
  });
  await expect(search).toBeFocused();
  await search.fill("Verify keyboard");
  await expect(
    page.getByRole("button", { name: `Open task ${title}`, exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: `Open task ${title}`, exact: true })
    .click();
  await page.screenshot({ path: "output/playwright/desktop-lifecycle.png" });
  expect(errors).toEqual([]);
});

test("nested folders and lists preserve tasks; sibling drag ordering persists", async ({
  page,
}) => {
  const folder = `UI work ${run}`,
    subfolder = `UI clients ${run}`,
    list = `UI release ${run}`;
  await newContainer(page, "Folder", folder);
  const folderId = (await snapshot(page)).folders.find(
    (item: any) => item.name === folder,
  ).id;
  await newContainer(page, "Folder", subfolder, folderId);
  const subfolderId = (await snapshot(page)).folders.find(
    (item: any) => item.name === subfolder,
  ).id;
  await newContainer(page, "List", list, subfolderId);
  const first = `UI first ${run}`,
    second = `UI second ${run}`;
  await add(page, first);
  await add(page, second);
  const firstRow = page.getByRole("group", {
    name: first,
    exact: true,
  });
  const secondRow = page.getByRole("group", {
    name: second,
    exact: true,
  });
  await secondRow.dragTo(firstRow, { targetPosition: { x: 50, y: 2 } });
  await expect
    .poll(async () => {
      const a = await taskByTitle(page, first),
        b = await taskByTitle(page, second);
      return b!.position < a!.position;
    })
    .toBe(true);
  await page.reload();
  await page.getByRole("treeitem", { name: list, exact: true }).click();
  await expect(
    page
      .locator(".task-row")
      .first()
      .getByRole("button", { name: `Open task ${second}`, exact: true }),
  ).toBeVisible();
  await page.getByRole("treeitem", { name: list, exact: true }).hover();
  await page
    .getByRole("button", { name: `Actions for ${list}`, exact: true })
    .click();
  await page
    .getByRole("menu", { name: `${list} actions`, exact: true })
    .getByRole("menuitem", { name: "Edit", exact: true })
    .click();
  const edit = page.getByRole("dialog", { name: "Edit List", exact: true });
  const renamed = `${list} renamed`;
  await edit
    .getByRole("textbox", { name: "List name", exact: true })
    .fill(renamed);
  await edit.getByRole("button", { name: "Save", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: renamed, exact: true }),
  ).toBeVisible();
  await page.getByRole("treeitem", { name: renamed, exact: true }).hover();
  await page
    .getByRole("button", { name: `Actions for ${renamed}`, exact: true })
    .click();
  await page
    .getByRole("menu", { name: `${renamed} actions`, exact: true })
    .getByRole("menuitem", { name: "Delete", exact: true })
    .click();
  const deletion = page.getByRole("dialog", {
    name: "Delete List",
    exact: true,
  });
  await deletion.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(deletion).toBeHidden();
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: `Open task ${first}`, exact: true }),
  ).toBeVisible();
  expect((await taskByTitle(page, first))?.list_id).toBe("inbox");
  await page.getByRole("treeitem", { name: folder, exact: true }).hover();
  await page
    .getByRole("button", { name: `Actions for ${folder}`, exact: true })
    .click();
  await page
    .getByRole("menu", { name: `${folder} actions`, exact: true })
    .getByRole("menuitem", { name: "Pin", exact: true })
    .click();
  await expect
    .poll(
      async () =>
        (await snapshot(page)).folders.find((item: any) => item.id === folderId)
          .pinned,
    )
    .toBe(true);
  await page.getByRole("treeitem", { name: folder, exact: true }).hover();
  await page
    .getByRole("button", { name: `Actions for ${folder}`, exact: true })
    .click();
  await page
    .getByRole("menu", { name: `${folder} actions`, exact: true })
    .getByRole("menuitem", { name: "Ungroup", exact: true })
    .click();
  await expect
    .poll(
      async () =>
        (await snapshot(page)).folders.find(
          (item: any) => item.id === subfolderId,
        ).parent_id,
    )
    .toBeNull();
});

test("CLI edits synchronize and concurrent editor conflicts preserve the local draft", async ({
  page,
}) => {
  const title = `UI sync ${run}`,
    external = `UI external ${run}`;
  await add(page, title);
  const task = await taskByTitle(page, title);
  const command = (action: string, input: Record<string, unknown>) =>
    JSON.parse(
      execFileSync(
        cli,
        ["--db", db, "--json", "call", action, JSON.stringify(input)],
        { encoding: "utf8" },
      ),
    );
  command("tasks.update", { id: task!.id, title: external });
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(external);
  const draft = "Keep this local draft despite a concurrent writer";
  await page
    .getByRole("textbox", { name: "Task notes", exact: true })
    .fill(draft);
  command("tasks.update", { id: task!.id, title: `${external} changed` });
  await expect(page.getByRole("alert")).toContainText("Your draft is kept");
  await expect(
    page.getByRole("textbox", { name: "Task notes", exact: true }),
  ).toHaveText(draft);
  // A failed note write must also block Add below. This is a real competing
  // CLI write, not a mocked service error or an artificial failed-draft flag.
  const rowButton = page.locator(
    `[data-task-id="${task!.id}"] .task-row-title`,
  );
  await rowButton.focus();
  await rowButton.press("Enter");
  const inline = page.getByRole("textbox", { name: /^New task below / });
  await expect(inline).toBeFocused();
  const pendingTitle = `UI blocked sibling ${run}`;
  await inline.fill(pendingTitle);
  const beforeBlocked = await snapshot(page);
  await inline.press("Enter");
  await expect(page.getByRole("alert")).toContainText(
    "Could not save pending changes. Your new task has not been added.",
  );
  await expect(inline).toHaveValue(pendingTitle);
  await expect(
    page.getByRole("textbox", { name: "Task notes", exact: true }),
  ).toHaveText(draft);
  const afterBlocked = await snapshot(page);
  expect(afterBlocked.revision).toBe(beforeBlocked.revision);
  expect(
    afterBlocked.tasks.some((item: Task) => item.title === pendingTitle),
  ).toBe(false);
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.getByRole("alert")).toBeHidden();
  await expect
    .poll(async () => command("tasks.get", { id: task!.id }).notes)
    .toContain(draft);
  expect(command("tasks.get", { id: task!.id }).title).toBe(
    `${external} changed`,
  );
  await expect(inline).toHaveValue(pendingTitle);
  await inline.press("Escape");
  await expect(inline).toBeHidden();
  expect(
    (await snapshot(page)).tasks.some(
      (item: Task) => item.title === pendingTitle,
    ),
  ).toBe(false);
});
