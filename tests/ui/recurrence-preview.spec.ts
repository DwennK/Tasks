import { expect } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  test,
  call,
  getTask,
  openTask,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";

test("calendar previews use the shared recurrence engine without saving draft rules", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Recurrence preview"),
  });
  const task = await call<Task>(page.request, "tasks.create", {
    title: unique("Three monthly occurrences"),
    list_id: list.id,
    due_date: "2030-01-01",
    recurrence: {
      frequency: "monthly",
      interval: 1,
      mode: "due_date",
      month_days: [1, 15, -1],
      count: 3,
    },
  });
  try {
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await openTask(page, task.title);
    const original = await getTask(page, task.id);
    const revision = (await snapshot(page)).revision;
    const requests: { action: string; input: Record<string, unknown> }[] = [];
    page.on("request", (request) => {
      if (request.method() === "POST" && request.url().endsWith("/__tasks"))
        requests.push(request.postDataJSON());
    });
    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Task date and reminders",
      exact: true,
    });
    const calendar = picker.locator(".date-calendar");
    const day = (value: string) => calendar.locator(`[data-date="${value}"]`);
    await expect(day("2030-01-15")).toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    await expect(day("2030-01-31")).toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    await expect(day("2030-01-15")).toHaveAccessibleDescription(
      /Upcoming occurrence/i,
    );
    await expect(day("2030-01-01")).not.toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    await expect(day("2030-02-01")).not.toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    expect(
      requests.some(
        ({ action, input }) =>
          action === "recurrence.preview" && input.due_date === "2030-01-01",
      ),
    ).toBe(true);
    await page.screenshot({ path: "output/playwright/recurrence-preview.png" });
    expect((await snapshot(page)).revision).toBe(revision);

    await picker.getByRole("button", { name: "Repeat", exact: true }).click();
    await picker
      .getByRole("menu", { name: "Repeat frequency", exact: true })
      .getByRole("menuitem", { name: "Custom", exact: true })
      .click();
    const custom = picker.getByRole("group", {
      name: "Custom repeat",
      exact: true,
    });
    await custom
      .getByRole("button", { name: "Repeat on day 15", exact: true })
      .click();
    await custom.getByRole("button", { name: "OK", exact: true }).click();
    // Removing the middle date changes the third occurrence to February 1.
    // These markers are computed from the unsaved dialog draft.
    await expect(day("2030-01-15")).not.toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    await expect(day("2030-01-31")).toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    await expect(day("2030-02-01")).toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    await page.screenshot({
      path: "output/playwright/recurrence-preview-draft.png",
    });
    expect((await snapshot(page)).revision).toBe(revision);
    expect(await getTask(page, task.id)).toEqual(original);

    await page.keyboard.press("Escape");
    await expect(picker).toBeHidden();
    await page
      .getByRole("button", { name: "Set due date", exact: true })
      .click();
    await expect(day("2030-01-15")).toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    await expect(day("2030-01-31")).toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    await expect(day("2030-02-01")).not.toHaveAttribute(
      "data-recurrence-preview",
      "true",
    );
    expect(await getTask(page, task.id)).toEqual(original);
    expect((await snapshot(page)).revision).toBe(revision);
    expect(
      requests.filter(
        ({ action }) =>
          !["revision", "snapshot", "recurrence.preview"].includes(action),
      ),
    ).toEqual([]);
  } finally {
    await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
