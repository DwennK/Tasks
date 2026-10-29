import { expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import {
  test,
  call,
  getTask,
  localDay,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";
import type { TaskList } from "../../src/types";

test.use({
  viewport: { width: 390, height: 844 },
  isMobile: true,
  hasTouch: true,
});

test("compact mobile Quick Add keeps touch metadata and IME input as a draft until submission", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Mobile Quick Add"),
  });
  const title = unique("タスク mobile composition");
  let taskId: string | undefined;
  const creations: string[] = [];
  page.on("request", (request) => {
    if (request.method() !== "POST" || !request.url().endsWith("/__tasks"))
      return;
    const input = request.postDataJSON();
    if (input?.action === "tasks.create") creations.push(input.input?.title);
  });

  try {
    await page.goto("/");
    await expect(
      page.getByRole("heading", { name: "Inbox", exact: true }),
    ).toBeVisible();
    await page.keyboard.press("Meta+n");
    const dialog = page.getByRole("dialog", { name: "Add Task", exact: true });
    await expect(dialog).toBeVisible();
    const bounds = (await dialog.boundingBox())!;
    expect(bounds.width).toBeCloseTo(366, 0);
    expect(bounds.height).toBeCloseTo(114, 0);
    expect(bounds.x).toBeCloseTo(12, 0);
    const input = dialog.getByRole("textbox", {
      name: "Quick task title",
      exact: true,
    });
    await expect(input).toBeFocused();
    await input.fill(title);

    // Browser automation cannot drive a system IME, but these are the actual
    // DOM composition events handled by the input; no database calls are mocked.
    await input.dispatchEvent("compositionstart", { data: "タスク" });
    await input.press("Enter");
    await expect(dialog).toBeVisible();
    await expect(input).toHaveValue(title);
    expect(creations).toEqual([]);
    expect(
      (await snapshot(page)).tasks.some((task) => task.title === title),
    ).toBe(false);
    await input.dispatchEvent("compositionend", { data: "タスク" });

    await dialog.getByRole("button", { name: "Quick list", exact: true }).tap();
    const lists = page.getByRole("menu", {
      name: "Quick task list",
      exact: true,
    });
    await lists
      .getByRole("menuitemradio", { name: list.name, exact: true })
      .tap();
    await expect(
      dialog.getByRole("button", { name: "Quick list", exact: true }),
    ).toContainText(list.name);
    await dialog
      .getByRole("button", { name: "Quick priority", exact: true })
      .tap();
    await page
      .getByRole("menuitemradio", { name: "High Priority", exact: true })
      .tap();
    await expect(
      dialog.getByRole("button", { name: "Quick priority", exact: true }),
    ).toHaveAttribute("title", "High Priority");
    await dialog
      .getByRole("button", { name: "Quick due date", exact: true })
      .tap();
    const datePicker = page.getByRole("dialog", {
      name: "Task date and reminders",
      exact: true,
    });
    await expect(datePicker).toBeVisible();
    const dateBounds = (await datePicker.boundingBox())!;
    expect(dateBounds.x).toBeGreaterThanOrEqual(11);
    expect(dateBounds.x + dateBounds.width).toBeLessThanOrEqual(379);
    await datePicker
      .getByRole("button", { name: "Tomorrow", exact: true })
      .tap();
    await datePicker.getByRole("button", { name: "OK", exact: true }).tap();
    await expect(
      dialog.getByRole("button", { name: "Quick due date", exact: true }),
    ).toContainText("Tomorrow");
    expect(creations).toEqual([]);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await mkdir("output/playwright", { recursive: true });
    await page.screenshot({ path: "output/playwright/quick-add-mobile.png" });

    await dialog.getByRole("button", { name: "Add Task", exact: true }).tap();
    await expect(dialog).toBeHidden();
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(title);
    const matches = (await snapshot(page)).tasks.filter(
      (task) => task.title === title,
    );
    expect(matches).toHaveLength(1);
    expect(creations).toEqual([title]);
    const created = matches[0]!;
    taskId = created.id;
    expect(created.list_id).toBe(list.id);
    expect(created.priority).toBe(3);
    expect(created.due_date).toBe(localDay(1));
    await page.reload();
    await expect(
      page.getByRole("heading", { name: "Inbox", exact: true }),
    ).toBeVisible();
    const persisted = await getTask(page, created.id);
    expect(persisted.title).toBe(title);
    expect(persisted.list_id).toBe(list.id);
    expect(persisted.priority).toBe(3);
    expect(persisted.due_date).toBe(localDay(1));
  } finally {
    if (taskId) await call(page.request, "tasks.delete", { id: taskId });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
