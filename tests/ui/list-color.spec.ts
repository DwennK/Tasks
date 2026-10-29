import { expect } from "@playwright/test";
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

test("List Color is hidden by default, preserves selection and only adds accents after explicit Show", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Colored rows"),
    color: "#e76a64",
  });
  const task = await call<Task>(page.request, "tasks.create", {
    title: unique("Colored Today task"),
    list_id: list.id,
    due_date: localDay(),
  });
  const sidebar = page.getByRole("complementary", {
    name: "Task lists",
    exact: true,
  });
  const row = page.getByRole("group", { name: task.title, exact: true });
  const icon = page
    .getByRole("treeitem", { name: list.name, exact: true })
    .locator(".tree-leading > svg");
  const nav = (name: string) =>
    sidebar.getByRole("button", { name: new RegExp(`^${name}(?:\\s|$)`) });
  async function verifyViews(show: boolean) {
    for (const view of ["Today", "All", "list"]) {
      if (view === "list")
        await page
          .getByRole("treeitem", { name: list.name, exact: true })
          .click();
      else await nav(view).click();
      // View navigation clears selection; desktop Escape preserves it.
      await expect(row).toBeVisible();
      await expect(row).not.toHaveClass(/selected/);
      await expect(row).toHaveCSS("border-left-width", show ? "2px" : "0px");
      if (show)
        await expect(row).toHaveCSS("border-left-color", "rgb(231, 106, 100)");
      const unselected = await row.evaluate(
        (element) => getComputedStyle(element).backgroundColor,
      );
      await row
        .getByRole("button", { name: `Open task ${task.title}`, exact: true })
        .click();
      await page.mouse.move(10, 10);
      await expect(row).toHaveClass(/selected/);
      await expect(
        page.getByRole("textbox", { name: "Task title", exact: true }),
      ).toHaveValue(task.title);
      await expect(row).toHaveCSS("border-left-width", show ? "2px" : "0px");
      const selected = await row.evaluate(
        (element) => getComputedStyle(element).backgroundColor,
      );
      expect(selected).not.toBe(unselected);
      expect(selected).not.toBe("rgba(0, 0, 0, 0)");
      await expect(icon).toHaveCSS("color", "rgb(231, 106, 100)");
    }
  }
  async function chooseListColor(show: boolean) {
    const settings = await settingsCategory(page, "Appearance");
    await settings.getByRole("tab", { name: "Display", exact: true }).click();
    const section = settings.locator("section").filter({
      has: page.getByRole("heading", { name: /^List Color/ }),
    });
    const choice = section.getByRole("button", {
      name: show ? /Show$/ : /Hide$/,
    });
    await choice.click();
    await expect(choice).toHaveAttribute("aria-pressed", "true");
    await expect
      .poll(async () => (await snapshot(page)).settings.show_list_color)
      .toBe(show);
    await closeSettings(page);
  }
  try {
    await page.goto("/");
    expect((await snapshot(page)).settings.show_list_color).toBe(false);
    await verifyViews(false);
    await chooseListColor(true);
    await verifyViews(true);
    await page.reload();
    await verifyViews(true);
    await chooseListColor(false);
    await verifyViews(false);
    await page.reload();
    await verifyViews(false);
  } finally {
    await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
