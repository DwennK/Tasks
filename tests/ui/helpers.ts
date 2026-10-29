import {
  test as base,
  expect,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import type { Snapshot, Task } from "../../src/types";

// Keep real browser failures alongside the database assertions. The conflict
// test intentionally causes one 400 response and verifies its recovery UI.
export const test = base.extend<{ browserDiagnostics: void }>({
  browserDiagnostics: [
    async ({ page }, use, info) => {
      const failures: string[] = [];
      const consoleErrors: string[] = [];
      const sourceChanges: string[] = [];
      page.on("pageerror", (error) => failures.push(error.message));
      page.on("console", (message) => {
        if (message.type() === "error") consoleErrors.push(message.text());
        if (/\[vite\].*(hot updated|page reload)/.test(message.text()))
          sourceChanges.push(message.text());
      });
      // Vite can reload the document without logging "page reload", for
      // example when Tailwind rescans a changed Markdown file. Observe its
      // protocol so a tooling reload cannot masquerade as lost app state.
      page.on("websocket", (socket) => {
        socket.on("framereceived", ({ payload }) => {
          try {
            const message = JSON.parse(payload.toString());
            if (["update", "full-reload", "prune"].includes(message.type))
              sourceChanges.push(
                `Vite ${message.type}: ${JSON.stringify(message)}`,
              );
          } catch {
            // Non-JSON messages are not part of Vite's update protocol.
          }
        });
      });
      await use();
      await info.attach("browser-diagnostics", {
        body: JSON.stringify(
          { pageErrors: failures, consoleErrors, sourceChanges },
          null,
          2,
        ),
        contentType: "application/json",
      });
      expect(failures, "Uncaught browser errors").toEqual([]);
      const unexpected = consoleErrors.filter(
        (message) =>
          !(
            info.title.includes("concurrent editor conflicts") &&
            /Failed to load resource:.*status of 400/.test(message)
          ),
      );
      expect(unexpected, "Unexpected browser console errors").toEqual([]);
      expect(
        sourceChanges,
        "Source edits invalidated this browser run",
      ).toEqual([]);
    },
    { auto: true },
  ],
});

export const dbPath = resolve("output/ui-test.sqlite3");
export const unique = (label: string) =>
  `${label} ${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
export const localDay = (offset = 0) => {
  const value = new Date();
  value.setDate(value.getDate() + offset);
  return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}`;
};
export async function call<T = any>(
  request: APIRequestContext,
  action: string,
  input: Record<string, unknown> = {},
): Promise<T> {
  const response = await request.post("/__tasks", { data: { action, input } });
  expect(response.ok(), `${action}: ${await response.text()}`).toBeTruthy();
  return response.json();
}
export const snapshot = (page: Page) =>
  call<Snapshot>(page.request, "snapshot");
export const getTask = (page: Page, id: string) =>
  call<Task>(page.request, "tasks.get", { id });
export function cliCall<T = any>(
  action: string,
  input: Record<string, unknown> = {},
): T {
  return JSON.parse(
    execFileSync(
      resolve("target/debug/tasks"),
      ["--db", dbPath, "--json", "call", action, JSON.stringify(input)],
      { encoding: "utf8" },
    ),
  );
}
export async function resetSettings(page: Page) {
  const baseline = {
    theme: "light",
    theme_color: "default",
    interface_style: "card",
    app_icon: "default",
    show_dock_icon: true,
    dock_count: "today",
    font_size: "normal",
    show_list_color: false,
    sidebar_count: "all",
    sidebar_width: 212,
    list_width: 394,
    default_priority: 0,
    default_list_id: "inbox",
    default_tag_id: null,
    default_date: "none",
    default_reminder_timed: "none",
    default_reminder_allday: "none",
    default_add_to: "bottom",
    date_recognition: "off",
    tag_recognition: "remove",
    completion_sound: false,
    week_start: 1,
    show_week_numbers: false,
    time_format: 24,
    show_timezone: false,
    hotkeys: {},
    view_preferences: {},
    smart_lists: {
      all: "show",
      today: "show",
      tomorrow: "show",
      upcoming: "show",
      inbox: "show",
      completed: "show",
      trash: "show",
      wont_do: "show",
      tags: "show",
      filters: "show",
    },
  };
  for (const [key, value] of Object.entries(baseline))
    await call(page.request, "settings.set", { key, value });
}
export async function addTask(page: Page, title: string): Promise<Task> {
  const input = page.getByRole("textbox", { name: "Add task", exact: true });
  await input.fill(title);
  await input.press("Enter");
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
  const task = (await snapshot(page)).tasks.find(
    (task) => task.title === title,
  );
  expect(task, `Created task ${title}`).toBeTruthy();
  return task!;
}
export const notes = (page: Page) =>
  page.getByRole("textbox", { name: "Task notes", exact: true });
export async function openTask(page: Page, title: string) {
  await page
    .getByRole("button", { name: `Open task ${title}`, exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
}
export async function settingsCategory(page: Page, name: string) {
  let dialog = page.getByRole("dialog", { name: "Settings", exact: true });
  if (!(await dialog.isVisible()))
    await page.getByRole("button", { name: "Settings", exact: true }).click();
  await dialog
    .getByRole("navigation", { name: "Settings categories" })
    .getByRole("button", { name, exact: true })
    .click();
  return dialog;
}
export async function closeSettings(page: Page) {
  await page
    .getByRole("button", { name: "Close settings", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Settings", exact: true }),
  ).toBeHidden();
}
