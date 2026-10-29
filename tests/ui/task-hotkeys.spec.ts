import { expect, type Locator, type Page } from "@playwright/test";
import {
  test,
  call,
  closeSettings,
  getTask,
  notes,
  openTask,
  resetSettings,
  settingsCategory,
  snapshot,
  unique,
} from "./helpers";
import type { Section, Tag, Task, TaskList } from "../../src/types";

const row = (page: Page, task: Task) =>
  page.getByRole("group", { name: task.title, exact: true });
const openButton = (page: Page, task: Task) =>
  page.getByRole("button", { name: `Open task ${task.title}`, exact: true });
const belowInput = (page: Page, task: Task) =>
  page.getByRole("textbox", {
    name: `New task below ${task.title}`,
    exact: true,
  });
const taskListRows = (page: Page) =>
  page.locator(".task-list-scroll .task-row");

async function listTasks(page: Page, list: TaskList) {
  return (await snapshot(page)).tasks.filter(
    (task) => task.list_id === list.id && !task.deleted_at,
  );
}
async function createList(page: Page, label: string) {
  return call<TaskList>(page.request, "lists.create", {
    name: unique(label),
    group_by: "section",
    sort_by: "manual",
  });
}
async function createSection(page: Page, list: TaskList, label: string) {
  return call<Section>(page.request, "sections.create", {
    list_id: list.id,
    name: unique(label),
  });
}
async function createTask(
  page: Page,
  list: TaskList,
  section: Section,
  label: string,
  parent?: Task,
) {
  return call<Task>(page.request, "tasks.create", {
    title: unique(label),
    list_id: list.id,
    section_id: section.id,
    ...(parent ? { parent_id: parent.id } : {}),
  });
}
async function showList(page: Page, list: TaskList) {
  await page.goto("/");
  await navigateList(page, list);
}
async function navigateList(page: Page, list: TaskList) {
  await page.getByRole("treeitem", { name: list.name, exact: true }).click();
  await expect(
    page.getByRole("heading", { name: list.name, exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Loading tasks…", { exact: true })).toBeHidden();
}
async function cleanup(page: Page, list: TaskList) {
  // Discover every live fixture, including any unexpected task created by
  // Enter, so a failed assertion cannot leave tasks behind in the Inbox.
  const tasks = await listTasks(page, list);
  const ids = new Set(tasks.map((task) => task.id));
  for (const task of tasks.filter(
    (task) => !task.parent_id || !ids.has(task.parent_id),
  ))
    await call(page.request, "tasks.delete", { id: task.id });
  await call(page.request, "lists.delete", { id: list.id });
}
async function enterFromRow(page: Page, task: Task) {
  const button = openButton(page, task);
  await button.click();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(task.title);
  // Clicking a desktop title starts inline editing. Leave that field first
  // so this remains a regression for Enter from the selected row button.
  const titleEditor = page.getByRole("textbox", {
    name: "Edit task title",
    exact: true,
  });
  await expect(titleEditor).toBeFocused();
  await titleEditor.press("Escape");
  await expect(titleEditor).toBeHidden();
  await button.focus();
  await button.press("Enter");
  const input = belowInput(page, task);
  await expect(input).toBeVisible();
  await expect(input).toBeFocused();
  return input;
}
async function submitBelow(
  page: Page,
  list: TaskList,
  input: Locator,
  title: string,
) {
  await input.fill(title);
  await input.press("Enter");
  await expect
    .poll(
      async () =>
        (await listTasks(page, list)).filter((task) => task.title === title)
          .length,
    )
    .toBe(1);
  const task = (await listTasks(page, list)).find(
    (item) => item.title === title,
  )!;
  await expect(row(page, task)).toBeVisible();
  return task;
}
async function persistedOrder(page: Page, list: TaskList, parent: Task | null) {
  return (await listTasks(page, list))
    .filter((task) => task.parent_id === (parent?.id ?? null))
    .sort((a, b) => a.position - b.position)
    .map((task) => task.id);
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await resetSettings(page);
});

test("Enter adds a task below its selected sibling in the same list and section, and cancellation writes nothing", async ({
  page,
}) => {
  const list = await createList(page, "Add below");
  try {
    const section = await createSection(page, list, "Current section");
    const otherSection = await createSection(page, list, "Other section");
    const parent = await createTask(page, list, section, "Selected parent");
    const tail = await createTask(page, list, section, "Next root");
    const other = await createTask(page, list, otherSection, "Other root");
    const child = await createTask(
      page,
      list,
      section,
      "Selected child",
      parent,
    );
    const lastChild = await createTask(
      page,
      list,
      section,
      "Next child",
      parent,
    );
    // An explicit position below the selection must win over this default.
    await call(page.request, "settings.set", {
      key: "default_add_to",
      value: "top",
    });
    await showList(page, list);

    const beforeCancel = await listTasks(page, list);
    const cancelled = await enterFromRow(page, parent);
    await cancelled.fill(unique("Cancelled sibling"));
    await cancelled.press("Escape");
    await expect(cancelled).toBeHidden();
    expect(await listTasks(page, list)).toEqual(beforeCancel);

    const addedRoot = await submitBelow(
      page,
      list,
      await enterFromRow(page, parent),
      unique("Inserted root"),
    );
    expect(addedRoot).toMatchObject({
      list_id: list.id,
      parent_id: null,
      section_id: section.id,
    });
    await expect
      .poll(() => persistedOrder(page, list, null))
      .toEqual([parent.id, addedRoot.id, tail.id, other.id]);
    await page
      .getByRole("button", {
        name: `Expand subtasks for ${parent.title}`,
        exact: true,
      })
      .click();
    const addedChild = await submitBelow(
      page,
      list,
      await enterFromRow(page, child),
      unique("Inserted child"),
    );
    expect(addedChild).toMatchObject({
      list_id: list.id,
      parent_id: parent.id,
      section_id: section.id,
    });
    await expect
      .poll(() => persistedOrder(page, list, parent))
      .toEqual([child.id, addedChild.id, lastChild.id]);
    await expect(taskListRows(page)).toHaveCount(7);
    await expect
      .poll(() =>
        taskListRows(page).evaluateAll((elements) =>
          elements.map((element) => element.getAttribute("aria-label")),
        ),
      )
      .toEqual([
        parent.title,
        child.title,
        addedChild.title,
        lastChild.title,
        addedRoot.title,
        tail.title,
        other.title,
      ]);
    await page.reload();
    await navigateList(page, list);
    await expect(row(page, addedRoot)).toBeVisible();
    await page
      .getByRole("button", {
        name: `Expand subtasks for ${parent.title}`,
        exact: true,
      })
      .click();
    await expect(row(page, addedChild)).toBeVisible();
    expect(await persistedOrder(page, list, parent)).toEqual([
      child.id,
      addedChild.id,
      lastChild.id,
    ]);
    expect(await persistedOrder(page, list, null)).toEqual([
      parent.id,
      addedRoot.id,
      tail.id,
      other.id,
    ]);

    // Changing the visual sort must not cause insertion to rewrite the
    // canonical order of existing siblings or silently switch back to Custom.
    await page.getByRole("button", { name: "Sort tasks", exact: true }).click();
    await page
      .getByRole("menu", { name: "Sort and group tasks", exact: true })
      .getByRole("button", { name: "Title", exact: true })
      .click();
    await expect
      .poll(
        async () =>
          (await snapshot(page)).lists.find((item) => item.id === list.id)
            ?.sort_by,
      )
      .toBe("title");
    await page.keyboard.press("Escape");
    await expect(
      page.getByRole("menu", { name: "Sort and group tasks", exact: true }),
    ).toBeHidden();
    const beforeSortedInsertion = await persistedOrder(page, list, null);
    const sortedSibling = await submitBelow(
      page,
      list,
      await enterFromRow(page, parent),
      unique("AAA sorted sibling"),
    );
    await expect
      .poll(() => persistedOrder(page, list, null))
      .toEqual([parent.id, sortedSibling.id, addedRoot.id, tail.id, other.id]);
    expect(
      (await persistedOrder(page, list, null)).filter(
        (id) => id !== sortedSibling.id,
      ),
    ).toEqual(beforeSortedInsertion);
    expect(
      (await snapshot(page)).lists.find((item) => item.id === list.id)?.sort_by,
    ).toBe("title");
    await expect(taskListRows(page).first()).toHaveAttribute(
      "aria-label",
      sortedSibling.title,
    );
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, list);
  }
});

test("Enter in task title and rich notes keeps editing without adding a sibling", async ({
  page,
}) => {
  const list = await createList(page, "Editing Enter");
  try {
    const section = await createSection(page, list, "Writing");
    const task = await createTask(page, list, section, "Editing task");
    await showList(page, list);
    await openTask(page, task.title);
    const title = page.getByRole("textbox", {
      name: "Task title",
      exact: true,
    });
    const renamed = unique("Renamed by keyboard");
    await title.fill(renamed);
    await title.press("Enter");
    await expect(notes(page)).toBeFocused();
    await expect(
      page.getByRole("textbox", { name: /^New task below / }),
    ).toHaveCount(0);
    await notes(page).fill("First paragraph");
    await notes(page).press("End");
    await notes(page).press("Enter");
    await page.keyboard.insertText("Second paragraph");
    await expect(notes(page).locator("p")).toHaveText([
      "First paragraph",
      "Second paragraph",
    ]);
    await expect
      .poll(async () => {
        const saved = await getTask(page, task.id);
        return {
          title: saved.title,
          text: saved.notes_text,
        };
      })
      .toEqual({
        title: renamed,
        text: "First paragraph Second paragraph",
      });
    expect((await listTasks(page, list)).map((item) => item.id)).toEqual([
      task.id,
    ]);
    await expect(
      page.getByRole("textbox", { name: /^New task below / }),
    ).toHaveCount(0);
    await page.reload();
    await navigateList(page, list);
    await openTask(page, renamed);
    await expect(notes(page).locator("p")).toHaveText([
      "First paragraph",
      "Second paragraph",
    ]);
    expect((await listTasks(page, list)).map((item) => item.id)).toEqual([
      task.id,
    ]);
  } finally {
    await cleanup(page, list);
  }
});

test("configured group and subtask shortcuts survive reload and operate recursively on desktop and mobile", async ({
  page,
}, info) => {
  const list = await createList(page, "Disclosure hotkeys");
  try {
    const first = await createSection(page, list, "First group");
    const second = await createSection(page, list, "Second group");
    const parent = await createTask(page, list, first, "Parent one");
    const child = await createTask(page, list, first, "Child one", parent);
    const grandchild = await createTask(page, list, first, "Grandchild", child);
    const otherParent = await createTask(page, list, second, "Parent two");
    const otherChild = await createTask(
      page,
      list,
      second,
      "Child two",
      otherParent,
    );
    await showList(page, list);
    const before = await listTasks(page, list);
    const settings = await settingsCategory(page, "Hotkeys");
    await settings
      .getByRole("button", {
        name: "Change Toggle all groups shortcut",
        exact: true,
      })
      .click();
    await page.keyboard.press("Meta+Alt+g");
    await expect
      .poll(async () => (await snapshot(page)).settings.hotkeys)
      .toMatchObject({ toggle_all_groups: "mod+alt+g" });
    await settings
      .getByRole("button", {
        name: "Change Expand/Collapse all subtasks shortcut",
        exact: true,
      })
      .click();
    await page.keyboard.press("Meta+Alt+s");
    await expect
      .poll(async () => (await snapshot(page)).settings.hotkeys)
      .toMatchObject({
        toggle_all_groups: "mod+alt+g",
        toggle_all_subtasks: "mod+alt+s",
      });
    await closeSettings(page);
    await page.reload();
    await navigateList(page, list);
    await expect(row(page, parent)).toBeVisible();
    await expect(row(page, otherParent)).toBeVisible();
    await expect(row(page, child)).toBeHidden();
    await expect(row(page, otherChild)).toBeHidden();
    await expect(page.locator(".task-group-toggle")).toHaveCount(2);

    await page.keyboard.press("Meta+Alt+g");
    await expect(taskListRows(page)).toHaveCount(0);
    await expect(page.locator(".task-group-toggle").first()).toBeVisible();
    await expect(page.locator(".task-group-toggle").last()).toBeVisible();
    await page.keyboard.press("Meta+Alt+g");
    await expect(row(page, parent)).toBeVisible();
    await expect(row(page, otherParent)).toBeVisible();

    await page.keyboard.press("Meta+Alt+s");
    for (const task of [child, grandchild, otherChild])
      await expect(row(page, task)).toBeVisible();
    for (const task of [parent, child, otherParent])
      await expect(
        page.getByRole("button", {
          name: `Collapse subtasks for ${task.title}`,
          exact: true,
        }),
      ).toHaveAttribute("aria-expanded", "true");
    await openButton(page, grandchild).click();
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(grandchild.title);
    await page.keyboard.press("Meta+Alt+s");
    for (const task of [child, grandchild, otherChild])
      await expect(row(page, task)).toBeHidden();
    await expect(taskListRows(page)).toHaveCount(2);

    // Selection survives collapsing its ancestors. Enter from the document
    // must reveal that exact insertion point, including both ancestor levels.
    await page.evaluate(() => (document.activeElement as HTMLElement)?.blur());
    await expect(page.locator("body")).toBeFocused();
    await page.keyboard.press("Enter");
    const revealedInput = belowInput(page, grandchild);
    await expect(revealedInput).toBeVisible();
    await expect(revealedInput).toBeFocused();
    for (const task of [parent, child, grandchild])
      await expect(row(page, task)).toBeVisible();
    await revealedInput.fill(unique("Cancelled hidden-child sibling"));
    await revealedInput.press("Escape");
    await expect(revealedInput).toBeHidden();
    expect(await listTasks(page, list)).toEqual(before);
    // Desktop Escape preserves the selected task. Reopen the list itself to
    // close the detail while leaving the disclosure state under test intact.
    await navigateList(page, list);
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toBeHidden();
    await page.keyboard.press("Meta+Alt+s");
    await expect(taskListRows(page)).toHaveCount(2);

    await page.setViewportSize({ width: 390, height: 844 });
    await page.keyboard.press("Meta+Alt+g");
    await expect(taskListRows(page)).toHaveCount(0);
    await page.keyboard.press("Meta+Alt+g");
    await expect(taskListRows(page)).toHaveCount(2);
    await page.keyboard.press("Meta+Alt+s");
    for (const task of [parent, child, grandchild, otherParent, otherChild])
      await expect(row(page, task)).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await info.attach("task-hotkeys-mobile", {
      body: await page.screenshot(),
      contentType: "image/png",
    });
    await page.keyboard.press("Meta+Alt+s");
    await expect(taskListRows(page)).toHaveCount(2);
    // Disclosure state changes must never alter persisted task data.
    expect(await listTasks(page, list)).toEqual(before);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, list);
  }
});

test("adding below in a tag view preserves both the view tag and an explicit title tag", async ({
  page,
}) => {
  const list = await createList(page, "Tagged siblings");
  const tags: Tag[] = [];
  try {
    for (const label of ["view-tag", "typed-tag", "destination-tag"])
      tags.push(
        await call<Tag>(page.request, "tags.create", {
          name: unique(label).replaceAll(" ", "-"),
        }),
      );
    const [viewTag, typedTag, destinationTag] = tags as [Tag, Tag, Tag];
    const section = await createSection(page, list, "Tagged section");
    const original = await createTask(page, list, section, "Tagged anchor");
    const anchor = await call<Task>(page.request, "tasks.update", {
      id: original.id,
      tag_ids: [viewTag.id],
    });
    const showTag = async () => {
      await page
        .getByRole("complementary", { name: "Task lists", exact: true })
        .getByRole("button", { name: viewTag.name, exact: true })
        .click();
      await expect(
        page.getByRole("heading", { name: viewTag.name, exact: true }),
      ).toBeVisible();
      await expect(row(page, anchor)).toBeVisible();
    };
    await page.goto("/");
    await showTag();
    const title = unique("Sibling keeps both tags");
    const input = await enterFromRow(page, anchor);
    await input.fill(`${title} #${typedTag.name}`);
    await input.press("Enter");
    await expect.poll(async () => (await listTasks(page, list)).length).toBe(2);
    const sibling = (await listTasks(page, list)).find(
      (task) => task.id !== anchor.id,
    )!;
    expect(sibling).toMatchObject({
      title,
      list_id: list.id,
      parent_id: null,
      section_id: section.id,
    });
    expect([...sibling.tag_ids].sort()).toEqual(
      [viewTag.id, typedTag.id].sort(),
    );
    await expect(row(page, sibling)).toBeVisible();
    await expect
      .poll(() => persistedOrder(page, list, null))
      .toEqual([anchor.id, sibling.id]);
    await page.reload();
    await showTag();
    await expect(row(page, sibling)).toBeVisible();
    expect([...(await getTask(page, sibling.id)).tag_ids].sort()).toEqual(
      [viewTag.id, typedTag.id].sort(),
    );
    expect((await getTask(page, anchor.id)).tag_ids).toEqual([viewTag.id]);

    // The same pointer click blurs the inline field and changes the active
    // tag. Creation must retain the context in which the user wrote the task.
    const blurredTitle = unique("Sibling keeps original view on blur");
    const blurredInput = await enterFromRow(page, anchor);
    await blurredInput.fill(`${blurredTitle} #${typedTag.name}`);
    await page
      .getByRole("complementary", { name: "Task lists", exact: true })
      .getByRole("button", { name: destinationTag.name, exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: destinationTag.name, exact: true }),
    ).toBeVisible();
    await expect.poll(async () => (await listTasks(page, list)).length).toBe(3);
    const blurredSibling = (await listTasks(page, list)).find(
      (task) => task.title === blurredTitle,
    );
    expect(blurredSibling).toBeTruthy();
    expect(blurredSibling).toMatchObject({
      list_id: list.id,
      parent_id: null,
      section_id: section.id,
    });
    expect([...blurredSibling!.tag_ids].sort()).toEqual(
      [viewTag.id, typedTag.id].sort(),
    );
    expect(blurredSibling!.tag_ids).not.toContain(destinationTag.id);
    await expect(row(page, blurredSibling!)).toBeHidden();
    await expect
      .poll(() => persistedOrder(page, list, null))
      .toEqual([anchor.id, blurredSibling!.id, sibling.id]);
    await showTag();
    await expect(row(page, blurredSibling!)).toBeVisible();
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await cleanup(page, list);
    for (const tag of tags)
      await call(page.request, "tags.delete", { id: tag.id });
  }
});
