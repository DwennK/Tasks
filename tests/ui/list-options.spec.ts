import { expect, type Page } from "@playwright/test";
import {
  test,
  addTask,
  call,
  getTask,
  localDay,
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
  await expect(page.getByText("Loading tasks…", { exact: true })).toBeHidden();
});

async function listDialog(page: Page) {
  await page.locator(".tree-heading").hover();
  await page
    .getByRole("button", { name: "Create list or folder", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "New List", exact: true }).click();
  return page.getByRole("dialog", { name: "Add List", exact: true });
}
async function listMenu(page: Page, name: string) {
  await page.getByRole("treeitem", { name, exact: true }).hover();
  await page
    .getByRole("button", { name: `Actions for ${name}`, exact: true })
    .click();
  return page.getByRole("menu", { name: `${name} actions`, exact: true });
}

test("list emoji, no color and Note List create real notes and persist in the editor", async ({
  page,
}) => {
  const name = unique("Science notebook");
  const dialog = await listDialog(page);
  await dialog
    .getByRole("textbox", { name: "List name", exact: true })
    .fill(name);
  await dialog
    .getByRole("button", { name: "Choose list icon", exact: true })
    .click();
  const icons = page.getByRole("dialog", {
    name: "Choose list icon",
    exact: true,
  });
  await icons
    .getByRole("textbox", { name: "Search icons", exact: true })
    .fill("test tube");
  await icons
    .getByRole("button", { name: "test tube science", exact: true })
    .click();
  await expect(
    dialog.getByRole("button", { name: "Choose list icon", exact: true }),
  ).toHaveText("🧪");
  await dialog.getByRole("button", { name: "No color", exact: true }).click();
  await dialog.getByRole("button", { name: "List type", exact: true }).click();
  await dialog
    .getByRole("listbox", { name: "List type", exact: true })
    .getByRole("option", { name: "Note List", exact: true })
    .click();
  await page.screenshot({ path: "output/playwright/list-note-dialog.png" });
  await dialog.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByRole("heading", { name, exact: true })).toBeVisible();
  const list = (await snapshot(page)).lists.find((item) => item.name === name)!;
  expect(list.icon).toBe("🧪");
  expect(list.color).toBe("");
  expect(list.list_type).toBe("note");
  const task = await addTask(page, unique("Experiment note"));
  expect(task.is_note).toBe(true);
  await expect(
    page.getByRole("button", { name: "Complete task", exact: true }),
  ).toBeHidden();
  await notes(page).fill("Notes created in this list remain editable offline.");
  await expect
    .poll(async () => (await getTask(page, task.id)).notes)
    .toContain("remain editable offline");
  await page.reload();
  await page.getByRole("treeitem", { name, exact: true }).click();
  await expect(page.getByRole("treeitem", { name, exact: true })).toContainText(
    "🧪",
  );
  await openTask(page, task.title);
  await expect(notes(page)).toHaveText(
    "Notes created in this list remain editable offline.",
  );
  await expect(
    page.getByRole("button", { name: "Complete task", exact: true }),
  ).toBeHidden();
  const menu = await listMenu(page, name);
  await menu.getByRole("menuitem", { name: "Edit", exact: true }).click();
  const edit = page.getByRole("dialog", { name: "Edit List", exact: true });
  await expect(
    edit.getByRole("button", { name: "No color", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(
    edit.getByRole("button", { name: "List type", exact: true }),
  ).toHaveText("Note List");
});

test("hidden smart-list tasks stay available directly; pin and list duplication preserve independent data", async ({
  page,
}) => {
  const name = unique("Private planning");
  const dialog = await listDialog(page);
  await dialog
    .getByRole("textbox", { name: "List name", exact: true })
    .fill(name);
  await dialog.getByRole("button", { name: "Blue", exact: true }).click();
  await dialog
    .getByRole("button", { name: "Show in smart lists", exact: true })
    .click();
  await dialog
    .getByRole("listbox", { name: "Show in smart lists", exact: true })
    .getByRole("option", { name: "Do not show", exact: true })
    .click();
  await dialog.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByRole("heading", { name, exact: true })).toBeVisible();
  const list = (await snapshot(page)).lists.find((item) => item.name === name)!;
  const task = await addTask(page, unique("Private due task"));
  await notes(page).fill("Copied list data must be independent.");
  await expect
    .poll(async () => (await getTask(page, task.id)).notes)
    .toContain("must be independent");
  await page.getByRole("button", { name: "Set due date", exact: true }).click();
  const date = page.getByRole("dialog", {
    name: "Task date and reminders",
    exact: true,
  });
  await date.getByRole("button", { name: "Today", exact: true }).click();
  await date.getByRole("button", { name: "OK", exact: true }).click();
  await expect(date).toBeHidden();
  await expect
    .poll(async () => (await getTask(page, task.id)).due_date)
    .toBe(localDay());
  await page
    .getByRole("complementary", { name: "Task lists", exact: true })
    .getByRole("button", { name: /^Today(?: \d+)?$/ })
    .click();
  await expect(
    page.getByRole("button", { name: `Open task ${task.title}`, exact: true }),
  ).toBeHidden();
  expect(
    (
      await call(page.request, "tasks.list", {
        view: "today",
        date: localDay(),
      })
    ).map((item: { id: string }) => item.id),
  ).not.toContain(task.id);
  expect(
    (await call(page.request, "tasks.list", { list_id: list.id })).map(
      (item: { id: string }) => item.id,
    ),
  ).toContain(task.id);
  await page.getByRole("treeitem", { name, exact: true }).click();
  await expect(
    page.getByRole("button", { name: `Open task ${task.title}`, exact: true }),
  ).toBeVisible();
  await (
    await listMenu(page, name)
  )
    .getByRole("menuitem", { name: "Pin", exact: true })
    .click();
  await expect
    .poll(
      async () =>
        (await snapshot(page)).lists.find((item) => item.id === list.id)
          ?.pinned,
    )
    .toBe(true);
  await page.reload();
  const pinnedMenu = await listMenu(page, name);
  await expect(
    pinnedMenu.getByRole("menuitem", { name: "Unpin", exact: true }),
  ).toBeVisible();
  await pinnedMenu
    .getByRole("menuitem", { name: "Duplicate", exact: true })
    .click();
  const copiedName = `${name} (copy)`;
  await expect(
    page.getByRole("treeitem", { name: copiedName, exact: true }),
  ).toBeVisible();
  const copied = (await snapshot(page)).lists.find(
    (item) => item.name === copiedName,
  )!;
  expect(copied.id).not.toBe(list.id);
  expect(copied.show_in_smart_lists).toBe(false);
  expect(copied.color).toBe("#509bef");
  const clone = (await snapshot(page)).tasks.find(
    (item) => item.list_id === copied.id && item.title === task.title,
  )!;
  expect(clone.id).not.toBe(task.id);
  expect(clone.notes).toContain("must be independent");
  expect(clone.due_date).toBe(localDay());
  await page.getByRole("treeitem", { name: copiedName, exact: true }).click();
  await openTask(page, clone.title);
  await page
    .getByRole("button", { name: "Complete task", exact: true })
    .click();
  await expect
    .poll(async () => (await getTask(page, clone.id)).completed_at)
    .not.toBeNull();
  expect((await getTask(page, task.id)).completed_at).toBeNull();
});
