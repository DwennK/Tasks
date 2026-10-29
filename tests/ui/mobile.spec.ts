import { resetSettings, settingsCategory, closeSettings } from "./helpers";
import { expect, type Page } from "@playwright/test";
import { test } from "./helpers";
import { mkdir } from "node:fs/promises";

async function call(
  page: Page,
  action: string,
  input: Record<string, unknown> = {},
) {
  const response = await page.request.post("/__tasks", {
    data: { action, input },
  });
  expect(response.ok(), await response.text()).toBeTruthy();
  return response.json();
}

test("mobile navigation and task editor remain usable; appearance persists on desktop", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  const title = `Mobile verification ${Date.now()}`;
  let id: string | undefined;
  await page.setViewportSize({ width: 390, height: 844 });
  await resetSettings(page);
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Toggle sidebar", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Close sidebar", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: /^All(?: \d+)?$/, exact: false })
    .click();
  await expect(
    page.getByRole("heading", { name: "All", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Close sidebar", exact: true }),
  ).toBeHidden();
  await page
    .getByRole("textbox", { name: "Add task", exact: true })
    .fill(title);
  await page
    .getByRole("textbox", { name: "Add task", exact: true })
    .press("Enter");
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
  const notes = "Verified on a narrow screen. These notes must survive reload.";
  await page
    .getByRole("textbox", { name: "Task notes", exact: true })
    .fill(notes);
  await expect
    .poll(async () => {
      const snapshot = await call(page, "snapshot");
      const task = snapshot.tasks.find(
        (item: { title: string }) => item.title === title,
      );
      id = task?.id;
      return task?.notes;
    })
    .toContain(notes);
  await expect(
    page.getByRole("button", { name: "Back to tasks", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Task actions", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await mkdir("output/playwright", { recursive: true });
  await page.screenshot({ path: "output/playwright/mobile.png" });
  await page
    .getByRole("button", { name: "Back to tasks", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: `Open task ${title}`, exact: true }),
  ).toBeVisible();
  await page.reload();
  await page
    .getByRole("button", { name: `Open task ${title}`, exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Task notes", exact: true }),
  ).toHaveText(notes);

  const appearance = await settingsCategory(page, "Appearance");
  await appearance.getByRole("button", { name: "Dark theme", exact: true }).click();
  await closeSettings(page);
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.setViewportSize({ width: 1470, height: 923 });
  await expect(
    page.getByRole("button", { name: /^All(?: \d+)?$/, exact: false }),
  ).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({ path: "output/playwright/desktop-dark.png" });
  const sidebarResize = page.getByRole("separator", {
    name: "Resize sidebar",
    exact: true,
  });
  const listResize = page.getByRole("separator", {
    name: "Resize task list",
    exact: true,
  });
  const sidebarBefore = (await page
    .getByRole("complementary", { name: "Task lists", exact: true })
    .boundingBox())!.width;
  const listBefore = (await page
    .getByRole("region", { name: "Tasks", exact: true })
    .boundingBox())!.width;
  const grip = (await sidebarResize.boundingBox())!;
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  await page.mouse.down();
  await page.mouse.move(
    grip.x + grip.width / 2 + 40,
    grip.y + grip.height / 2,
    { steps: 5 },
  );
  await page.mouse.up();
  await expect
    .poll(async () => (await call(page, "snapshot")).settings.sidebar_width)
    .toBe(252);
  await listResize.focus();
  await listResize.press("Shift+ArrowRight");
  await expect
    .poll(async () => (await call(page, "snapshot")).settings.list_width)
    .toBe(424);
  expect(
    (await page
      .getByRole("complementary", { name: "Task lists", exact: true })
      .boundingBox())!.width,
  ).toBeCloseTo(sidebarBefore + 40, 0);
  expect(
    (await page
      .getByRole("region", { name: "Tasks", exact: true })
      .boundingBox())!.width,
  ).toBeCloseTo(listBefore + 30, 0);
  await page.screenshot({ path: "output/playwright/desktop-resized.png" });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(sidebarResize).toHaveAttribute("aria-valuenow", "252");
  await expect(listResize).toHaveAttribute("aria-valuenow", "424");
  await settingsCategory(page, "Appearance");
  await page
    .getByRole("button", { name: "Default theme", exact: true })
    .click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await closeSettings(page);
  if (id) await call(page, "tasks.delete", { id });
  expect(errors).toEqual([]);
});
