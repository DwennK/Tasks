import { expect } from "@playwright/test";
import { test, resetSettings, snapshot, closeSettings } from "./helpers";

test("rail shortcuts open working views and settings without changing tasks, including a short window", async ({ page }) => {
  await resetSettings(page);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Inbox", exact: true })).toBeVisible();
  const before = (await snapshot(page)).tasks;
  const rail = page.getByRole("complementary", { name: "App navigation" });
  await rail.getByRole("button", { name: "Upcoming tasks", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Next 7 Days", exact: true })).toBeVisible();
  await rail.getByRole("button", { name: "Today's tasks", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Today", exact: true })).toBeVisible();
  await rail.getByRole("button", { name: "Board view", exact: true }).click();
  await expect(page.locator(".task-list-scroll")).toHaveClass(/task-board/);
  await rail.getByRole("button", { name: "Tasks", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Inbox", exact: true })).toBeVisible();
  await expect(page.locator(".task-list-scroll")).not.toHaveClass(/task-board/);
  await rail.getByRole("button", { name: "Create task filter", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Add Filter", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await rail.getByRole("button", { name: "Refresh tasks", exact: true }).click();
  await expect(rail.getByRole("button", { name: "Refresh tasks", exact: true })).toBeEnabled();
  await rail.getByRole("button", { name: "Notifications", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Notifications", exact: true })).toBeVisible();
  await closeSettings(page);
  await rail.getByRole("button", { name: "Keyboard shortcuts", exact: true }).click();
  await expect(page.getByRole("tab", { name: "Hotkeys", exact: true })).toBeVisible();
  await closeSettings(page);
  await page.setViewportSize({ width: 760, height: 500 });
  for (const button of await rail.getByRole("button").all()) {
    const box = await button.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.y).toBeGreaterThanOrEqual(0);
    expect(box!.y + box!.height).toBeLessThanOrEqual(500);
  }
  await rail.getByRole("button", { name: "Search tasks", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Search", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  expect((await snapshot(page)).tasks).toEqual(before);
});
