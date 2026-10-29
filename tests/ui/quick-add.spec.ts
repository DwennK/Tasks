import { expect, type Page, type Route } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  test,
  call,
  getTask,
  localDay,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";

const quick = (page: Page) =>
  page.getByRole("dialog", { name: "Add Task", exact: true });
const titleInput = (page: Page) =>
  quick(page).getByRole("textbox", { name: "Quick task title", exact: true });
async function openQuick(page: Page) {
  await page.keyboard.press("Meta+n");
  await expect(quick(page)).toBeVisible();
  await expect(titleInput(page)).toBeFocused();
}
async function saveQuick(page: Page, title: string) {
  await titleInput(page).fill(title);
  await titleInput(page).press("Enter");
  await expect(quick(page)).toBeHidden();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
  const task = (await snapshot(page)).tasks.find(
    (item) => item.title === title,
  );
  expect(task).toBeTruthy();
  return task!;
}

test.beforeEach(async ({ page }) => {
  await resetSettings(page);
});

test("quick add uses saved defaults and the active date or list context", async ({
  page,
}) => {
  const defaultList = await call<TaskList>(page.request, "lists.create", {
    name: unique("Quick default list"),
  });
  const contextList = await call<TaskList>(page.request, "lists.create", {
    name: unique("Quick context list"),
  });
  for (const [key, value] of Object.entries({
    default_list_id: defaultList.id,
    default_priority: 2,
    default_date: "tomorrow",
  }))
    await call(page.request, "settings.set", { key, value });
  // Delay only delivery of the genuine initial Rust snapshot. A user can
  // invoke Quick Add as soon as the shell appears, before settings arrive.
  let releaseSnapshot!: () => void;
  let snapshotFetched!: () => void;
  let heldInitialSnapshot = false;
  const snapshotGate = new Promise<void>((resolve) => {
    releaseSnapshot = resolve;
  });
  const realSnapshotReady = new Promise<void>((resolve) => {
    snapshotFetched = resolve;
  });
  const holdInitialSnapshot = async (route: Route) => {
    if (
      !heldInitialSnapshot &&
      route.request().postDataJSON()?.action === "snapshot"
    ) {
      heldInitialSnapshot = true;
      const response = await route.fetch();
      snapshotFetched();
      await snapshotGate;
      await route.fulfill({ response });
      return;
    }
    await route.continue();
  };
  await page.route("**/__tasks", holdInitialSnapshot);
  try {
    await page.goto("/");
    await realSnapshotReady;
    await expect(
      page.getByRole("heading", { name: "Inbox", exact: true }),
    ).toBeVisible();
    await page.keyboard.press("Meta+n");
    await page.keyboard.press("Meta+n");
    await expect(quick(page)).toBeHidden();
  } finally {
    releaseSnapshot();
  }
  await expect(quick(page)).toBeVisible();
  await expect(titleInput(page)).toBeFocused();
  await page.unroute("**/__tasks", holdInitialSnapshot);
  const box = await quick(page).boundingBox();
  expect(box?.width).toBe(600);
  expect(box?.height).toBe(114);
  await expect(
    quick(page).getByRole("button", { name: "Quick due date", exact: true }),
  ).toHaveText("Tomorrow");
  await expect(
    quick(page).getByRole("button", { name: "Quick priority", exact: true }),
  ).toHaveAttribute("title", "Medium Priority");
  await expect(
    quick(page).getByRole("button", { name: "Quick list", exact: true }),
  ).toHaveText(defaultList.name);
  await titleInput(page).fill("Keep this Quick Add draft");
  await page.keyboard.press("Meta+n");
  await expect(titleInput(page)).toHaveValue("Keep this Quick Add draft");
  await titleInput(page).clear();
  await page.screenshot({ path: "output/playwright/quick-add-desktop.png" });
  const saved = await saveQuick(page, unique("Quick defaults"));
  expect(saved).toMatchObject({
    list_id: defaultList.id,
    due_date: localDay(1),
    priority: 2,
  });

  // An older editor picker must not consume Escape from a newer Quick Add
  // popup, including native accessibility activation with no DOM focus.
  await page.getByRole("button", { name: "Set due date", exact: true }).click();
  const olderPicker = page.getByRole("dialog", {
    name: "Task date and reminders",
    exact: true,
  });
  await expect(olderPicker).toBeVisible();
  for (const [triggerName, menuName] of [
    ["Quick priority", "Quick task priority"],
    ["Quick list", "Quick task list"],
  ]) {
    await openQuick(page);
    const discardedTitle = unique("Unsaved layered Quick Add");
    await titleInput(page).fill(discardedTitle);
    // A pointer click outside would legitimately dismiss the older picker.
    // Keyboard activation leaves both layers open and exercises their ordering.
    await expect(olderPicker).toBeVisible();
    await quick(page)
      .getByRole("button", { name: triggerName, exact: true })
      .press("Enter");
    const popup = page.getByRole("menu", { name: menuName, exact: true });
    await expect(popup).toBeVisible();
    await expect(olderPicker).toBeVisible();
    await page.evaluate(() =>
      (document.activeElement as HTMLElement | null)?.blur(),
    );
    await expect(page.locator("body")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(popup).toBeHidden();
    await expect(quick(page)).toBeVisible();
    await expect(titleInput(page)).toHaveValue(discardedTitle);
    await expect(olderPicker).toBeVisible();
    await page.evaluate(() =>
      (document.activeElement as HTMLElement | null)?.blur(),
    );
    await expect(page.locator("body")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(quick(page)).toBeHidden();
    await expect(olderPicker).toBeVisible();
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(saved.title);
    expect(
      (await snapshot(page)).tasks.some(
        (task) => task.title === discardedTitle,
      ),
    ).toBe(false);
  }
  await page.keyboard.press("Escape");
  await expect(olderPicker).toBeHidden();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(saved.title);

  const sidebar = page.getByRole("complementary", {
    name: "Task lists",
    exact: true,
  });
  await sidebar
    .getByRole("button", { name: /^Today(?: \d+)?$/, exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Today", exact: true }),
  ).toBeVisible();
  await openQuick(page);
  await expect(
    quick(page).getByRole("button", { name: "Quick due date", exact: true }),
  ).toHaveText("Today");
  const today = await saveQuick(page, unique("Quick Today context"));
  expect(today).toMatchObject({
    list_id: defaultList.id,
    due_date: localDay(),
    priority: 2,
  });

  await page
    .getByRole("treeitem", { name: contextList.name, exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: contextList.name, exact: true }),
  ).toBeVisible();
  await openQuick(page);
  await expect(
    quick(page).getByRole("button", { name: "Quick list", exact: true }),
  ).toHaveText(contextList.name);
  const contextual = await saveQuick(page, unique("Quick list context"));
  expect(contextual).toMatchObject({
    list_id: contextList.id,
    due_date: localDay(1),
    priority: 2,
  });
  await page.reload();
  expect(await getTask(page, contextual.id)).toMatchObject({
    title: contextual.title,
    list_id: contextList.id,
    due_date: localDay(1),
    priority: 2,
  });
});

test("quick add previews recognized metadata and explicit choices override it without losing the title", async ({
  page,
}) => {
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Quick chosen list"),
  });
  await call(page.request, "settings.set", {
    key: "date_recognition",
    value: "remove",
  });
  await call(page.request, "settings.set", {
    key: "default_reminder_timed",
    value: "15m",
  });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("complementary", { name: "Task lists", exact: true })
    .getByRole("button", { name: /^Today(?: \d+)?$/, exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Today", exact: true }),
  ).toBeVisible();
  await openQuick(page);
  const title = unique("Quick metadata");
  await expect(titleInput(page)).toHaveValue("");
  await expect(
    quick(page).getByRole("button", { name: "Quick due date", exact: true }),
  ).toHaveText("Today");
  await expect(
    quick(page).getByRole("button", { name: "Quick priority", exact: true }),
  ).toHaveAttribute("title", "No Priority");
  await expect(
    quick(page).getByRole("button", { name: "Quick list", exact: true }),
  ).toHaveText("Inbox");
  await expect(
    quick(page).getByRole("button", { name: "Add Task", exact: true }),
  ).toBeDisabled();
  await page.screenshot({
    path: "output/playwright/quick-add-reference35.png",
  });
  await quick(page).screenshot({
    path: "output/playwright/quick-add-reference35-dialog.png",
  });
  await titleInput(page).fill(`${title} tomorrow at 09:30 !high ~${list.name}`);
  await expect(
    quick(page).getByRole("button", { name: "Quick due date", exact: true }),
  ).toHaveText("Tomorrow 09:30");
  await expect(
    quick(page).getByRole("button", { name: "Quick priority", exact: true }),
  ).toHaveAttribute("title", "High Priority");
  await expect(
    quick(page).getByRole("button", { name: "Quick list", exact: true }),
  ).toHaveText(list.name);

  const priority = quick(page).getByRole("button", {
    name: "Quick priority",
    exact: true,
  });
  await priority.click();
  await expect(
    page.getByRole("menuitemradio", { name: "High Priority", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(
    page.getByRole("menuitemradio", { name: "Medium Priority", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(priority).toBeFocused();
  await expect(priority).toHaveAttribute("title", "Medium Priority");
  await quick(page)
    .getByRole("button", { name: "Quick list", exact: true })
    .click();
  await page.getByRole("menuitemradio", { name: "Inbox", exact: true }).click();
  await quick(page)
    .getByRole("button", { name: "Quick due date", exact: true })
    .click();
  const picker = page.getByRole("dialog", {
    name: "Task date and reminders",
    exact: true,
  });
  // AX activation can leave the Quick Add title focused behind the picker.
  // Escape must close Repeat, then Date, while retaining the unsaved task.
  await picker.getByRole("button", { name: "Repeat", exact: true }).click();
  const repeat = picker.getByRole("menu", {
    name: "Repeat frequency",
    exact: true,
  });
  await expect(repeat).toBeVisible();
  await titleInput(page).focus();
  await expect(titleInput(page)).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(repeat).toBeHidden();
  await expect(picker).toBeVisible();
  await expect(quick(page)).toBeVisible();
  await titleInput(page).focus();
  await page.keyboard.press("Escape");
  await expect(picker).toBeHidden();
  await expect(quick(page)).toBeVisible();
  await expect(titleInput(page)).toHaveValue(
    `${title} tomorrow at 09:30 !high ~${list.name}`,
  );
  await quick(page)
    .getByRole("button", { name: "Quick due date", exact: true })
    .click();
  await picker.getByRole("button", { name: "Today", exact: true }).click();
  await picker.getByRole("button", { name: /^Time/ }).click();
  await picker.getByLabel("Due time", { exact: true }).fill("14:45");
  await picker.getByLabel("Due time", { exact: true }).press("Enter");
  await picker.getByRole("button", { name: "OK", exact: true }).click();
  await expect(picker).toBeHidden();
  await expect(quick(page)).toBeVisible();
  await quick(page)
    .getByRole("button", { name: "Add Task", exact: true })
    .click();
  await expect(quick(page)).toBeHidden();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
  const created = (await snapshot(page)).tasks.find(
    (task) => task.title === title,
  ) as Task;
  expect(created).toMatchObject({
    list_id: "inbox",
    due_date: localDay(),
    due_time: "14:45",
    priority: 2,
  });
  expect(created.reminders).toHaveLength(1);
  expect(
    new Date(`${created.due_date}T14:45:00`).getTime() -
      new Date(created.reminders[0]!).getTime(),
  ).toBe(15 * 60_000);
  await page.reload();
  expect(await getTask(page, created.id)).toMatchObject({
    title,
    list_id: "inbox",
    due_date: localDay(),
    due_time: "14:45",
    priority: 2,
  });
});

test("quick add Clear removes recognized recurrence and date despite saved reminder defaults", async ({
  page,
}) => {
  for (const [key, value] of Object.entries({
    date_recognition: "remove",
    default_date: "tomorrow",
    default_reminder_timed: "15m",
    default_reminder_allday: "09:00",
  }))
    await call(page.request, "settings.set", { key, value });
  const title = unique("Quick clear date");
  let createdId = "";
  try {
    await page.goto("/");
    await openQuick(page);
    await titleInput(page).fill(`${title} every Monday at 09:30`);
    await quick(page)
      .getByRole("button", { name: "Quick due date", exact: true })
      .click();
    await page
      .getByRole("dialog", { name: "Task date and reminders", exact: true })
      .getByRole("button", { name: "Clear", exact: true })
      .click();
    await expect(
      quick(page).getByRole("button", { name: "Quick due date", exact: true }),
    ).toHaveText("No Date");
    await quick(page)
      .getByRole("button", { name: "Add Task", exact: true })
      .click();
    await expect(quick(page)).toBeHidden();
    const created = (await snapshot(page)).tasks.find(
      (task) => task.title === title,
    );
    expect(created).toBeTruthy();
    createdId = created!.id;
    const cleared = {
      title,
      due_date: null,
      due_time: null,
      recurrence: null,
      reminders: [],
      reminder_at: null,
    };
    expect(created).toMatchObject(cleared);
    await page.reload();
    expect(await getTask(page, createdId)).toMatchObject(cleared);
  } finally {
    if (createdId) await call(page.request, "tasks.delete", { id: createdId });
  }
});
