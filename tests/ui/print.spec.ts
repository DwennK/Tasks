import { expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { test, call, notes, openTask, resetSettings, unique } from "./helpers";
import type { Task } from "../../src/types";

test("Print invokes the browser and prints only the selected task content", async ({
  page,
}) => {
  await resetSettings(page);
  const other = unique("Unrelated private task");
  await call(page.request, "tasks.create", { title: other });
  const finalParagraph =
    "Final paragraph: the entire task must survive pagination.";
  const task = await call<Task>(page.request, "tasks.create", {
    title: unique("Printed résumé"),
    notes:
      "<h2>Release checklist</h2><p>Only this selected task belongs on the printed page.</p>" +
      Array.from(
        { length: 60 },
        (_, index) =>
          `<p>Print continuation paragraph ${index + 1}: verify the complete note.</p>`,
      ).join("") +
      `<p>${finalParagraph}</p>`,
    notes_format: "html",
  });
  const child = unique("Printed subtask");
  await call(page.request, "tasks.create", {
    title: child,
    parent_id: task.id,
  });
  await page.goto("/");
  await openTask(page, task.title);
  await expect(
    notes(page).getByRole("heading", {
      name: "Release checklist",
      exact: true,
    }),
  ).toBeVisible();
  await page.evaluate(() => {
    document.documentElement.dataset.printEvents = "0";
    document.documentElement.dataset.printCalls = "0";
    // Headless Chrome emits beforeprint once until its print lifecycle ends.
    // Observe both invocations while preserving the real browser print path.
    const nativePrint = window.print.bind(window);
    window.print = () => {
      document.documentElement.dataset.printCalls = String(
        Number(document.documentElement.dataset.printCalls) + 1,
      );
      nativePrint();
    };
    window.addEventListener("beforeprint", () => {
      document.documentElement.dataset.printEvents = String(
        Number(document.documentElement.dataset.printEvents) + 1,
      );
    });
  });
  await page.getByRole("button", { name: "Task actions", exact: true }).click();
  await page.getByRole("menuitem", { name: "Print", exact: true }).click();
  await expect
    .poll(() => page.locator("html").getAttribute("data-print-events"))
    .toBe("1");
  await expect(
    page.getByRole("menu", { name: "Task actions", exact: true }),
  ).toBeHidden();
  await expect(page.locator("html")).toHaveAttribute("data-print-calls", "1");
  await page.keyboard.press("Meta+p");
  await expect(page.locator("html")).toHaveAttribute("data-print-calls", "2");
  await page.emulateMedia({ media: "print" });
  for (const selector of [
    ".app-rail",
    ".sidebar",
    ".task-list-panel",
    ".editor-toolbar",
    ".editor-footer",
  ])
    await expect(page.locator(selector)).toBeHidden();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toBeVisible();
  await expect(notes(page)).toContainText(
    "Only this selected task belongs on the printed page.",
  );
  await expect(
    page.getByRole("button", { name: child, exact: true }),
  ).toBeVisible();
  expect(
    await page.locator(".task-editor").evaluate((element) => ({
      display: getComputedStyle(element).display,
      overflow: getComputedStyle(element).overflow,
      background: getComputedStyle(element).backgroundColor,
    })),
  ).toEqual({
    display: "block",
    overflow: "visible",
    background: "rgb(255, 255, 255)",
  });
  await page.screenshot({
    path: "output/playwright/task-print.png",
  });
  const path = resolve("output/playwright/task-print.pdf");
  await page.pdf({ path, format: "A4", printBackground: true });
  const printed = execFileSync("pdftotext", [path, "-"], { encoding: "utf8" });
  expect(printed).toContain(task.title);
  expect(printed).toContain(child);
  expect(printed).toContain("Release checklist");
  expect(printed).toContain(
    "Only this selected task belongs on the printed page.",
  );
  expect(printed).toContain(finalParagraph);
  expect(printed).not.toContain(other);
  expect(printed).not.toContain("Next 7 Days");
  expect(printed).not.toContain("Add subtask");
  await page.emulateMedia({ media: "screen" });
  await expect(page.locator(".sidebar")).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(task.title);
});
