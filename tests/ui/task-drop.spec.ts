import { expect, type Locator } from "@playwright/test";
import { test, call, resetSettings, snapshot, unique } from "./helpers";
import type { Task, TaskList } from "../../src/types";

test("task insertion preview follows the pointer, matches persisted order and clears on cancellation", async ({ page }) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", { name: unique("Drop preview") });
  const created: Task[] = [];
  for (const name of ["First task", "Middle task", "Last task"])
    created.push(await call<Task>(page.request, "tasks.create", { title: unique(name), list_id: list.id }));
  const [a, b, c] = created;
  const child = await call<Task>(page.request, "tasks.create", { title: unique("Nested task"), parent_id: b.id });
  const row = (task: Task) => page.getByRole("group", { name: task.title, exact: true });
  const indicator = page.locator(".task-drop-indicator");
  const move = async (target: Locator, edge: "before" | "after") => {
    const box = (await target.boundingBox())!;
    await page.mouse.move(box.x + box.width - 45, box.y + (edge === "before" ? 3 : box.height - 3), { steps: 6 });
    // Two dragover events are required by Chromium when entering a new target.
    await page.mouse.move(box.x + box.width - 44, box.y + (edge === "before" ? 3 : box.height - 3));
  };
  const start = async (task: Task) => {
    const box = (await row(task).boundingBox())!;
    await page.mouse.move(box.x + box.width - 45, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width - 65, box.y + box.height / 2, { steps: 5 });
  };
  const order = async () => (await snapshot(page)).tasks.filter(t => t.list_id === list.id && !t.parent_id && !t.deleted_at).sort((a,b) => a.position-b.position).map(t => t.id);
  try {
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await page.getByRole("button", { name: `Expand subtasks for ${b.title}`, exact: true }).click();
    await start(c);
    await move(row(b), "before");
    await expect(indicator).toHaveCount(1);
    await expect(indicator).toHaveAttribute("data-position", "before");
    const line = (await indicator.boundingBox())!;
    const target = (await row(b).boundingBox())!;
    expect(Math.abs(line.y + line.height / 2 - target.y)).toBeLessThan(1);
    await expect(row(c)).toHaveCSS("opacity", "0.4");
    await page.screenshot({ path: "output/playwright/task-drop-before.png" });
    await move(row(b), "after");
    await expect(indicator).toHaveAttribute("data-position", "after");
    const subtree = (await page.locator(`[data-task-id="${b.id}"]`).boundingBox())!;
    const after = (await indicator.boundingBox())!;
    expect(Math.abs(after.y + after.height / 2 - subtree.y - subtree.height)).toBeLessThan(1);
    await page.screenshot({ path: "output/playwright/task-drop-after-subtree.png" });
    await move(row(b), "before");
    await page.mouse.up();
    await expect(indicator).toHaveCount(0);
    await expect.poll(order).toEqual([a.id, c.id, b.id]);
    await start(a);
    await move(row(b), "after");
    await page.mouse.up();
    await expect.poll(order).toEqual([c.id, b.id, a.id]);
    await page.reload();
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await expect(page.locator(".task-row").first()).toHaveAttribute("aria-label", c.title);
    await page.getByRole("button", { name: `Expand subtasks for ${b.title}`, exact: true }).click();
    const beforeCancel = (await snapshot(page)).revision;
    await start(a);
    await move(row(child), "before");
    await expect(indicator).toHaveCount(0);
    await move(row(c), "before");
    await expect(indicator).toHaveCount(1);
    await page.keyboard.press("Escape");
    await page.mouse.up();
    await expect(indicator).toHaveCount(0);
    await expect(page.locator(".task-row.is-dragging")).toHaveCount(0);
    expect((await snapshot(page)).revision).toBe(beforeCancel);
    expect(await order()).toEqual([c.id, b.id, a.id]);
    await page.setViewportSize({ width: 390, height: 844 });
    await start(a);
    await move(row(c), "before");
    await expect(indicator).toHaveCount(1);
    await page.screenshot({ path: "output/playwright/task-drop-mobile.png" });
    await page.keyboard.press("Escape");
    await page.mouse.up();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  } finally {
    await page.mouse.up();
    for (const task of created) await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
