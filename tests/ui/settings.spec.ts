import { expect } from "@playwright/test";
import { test } from "./helpers";
import {
  addTask,
  closeSettings,
  getTask,
  localDay,
  resetSettings,
  settingsCategory,
  snapshot,
  unique,
} from "./helpers";

test.beforeEach(async ({ page }) => {
  await resetSettings(page);
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
});

test("task defaults and recognition affect created tasks, not just saved settings", async ({
  page,
}) => {
  const settings = await settingsCategory(page, "More");
  await settings
    .getByRole("combobox", { name: "Default priority", exact: true })
    .selectOption("3");
  await expect
    .poll(async () => (await snapshot(page)).settings.default_priority)
    .toBe(3);
  await settings
    .getByRole("combobox", { name: "Default date", exact: true })
    .selectOption("tomorrow");
  await expect
    .poll(async () => (await snapshot(page)).settings.default_date)
    .toBe("tomorrow");
  await settings
    .getByRole("switch", { name: "Date recognition", exact: true })
    .click();
  await expect
    .poll(async () => (await snapshot(page)).settings.date_recognition)
    .toBe("remove");
  await settings
    .getByRole("combobox", { name: "Default timed reminder", exact: true })
    .selectOption("15m");
  await expect
    .poll(async () => (await snapshot(page)).settings.default_reminder_timed)
    .toBe("15m");
  await closeSettings(page);
  const defaultTask = await addTask(page, unique("Default behavior"));
  expect(defaultTask.priority).toBe(3);
  expect(defaultTask.due_date).toBe(localDay(1));
  const title = unique("Recognized date");
  const add = page.getByRole("textbox", { name: "Add task", exact: true });
  await add.fill(`${title} tomorrow at 09:30`);
  await add.press("Enter");
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
  const task = (await snapshot(page)).tasks.find(
    (task) => task.title === title,
  )!;
  expect(task.priority).toBe(3);
  expect(task.due_date).toBe(localDay(1));
  expect(task.due_time).toBe("09:30");
  expect(task.reminders).toHaveLength(1);
  const due = new Date(`${task.due_date}T09:30:00`);
  expect(due.getTime() - new Date(task.reminders[0]!).getTime()).toBe(
    15 * 60_000,
  );
  await page.reload();
  const settingsAfter = await settingsCategory(page, "More");
  await expect(
    settingsAfter.getByRole("combobox", {
      name: "Default priority",
      exact: true,
    }),
  ).toHaveValue("3");
  await expect(
    settingsAfter.getByRole("combobox", {
      name: "Default timed reminder",
      exact: true,
    }),
  ).toHaveValue("15m");
});

test("calendar settings, smart-list visibility and font choice change the actual interface", async ({
  page,
}) => {
  const task = await addTask(page, unique("Display preference"));
  const row = page.getByRole("group", { name: task.title, exact: true });
  const initialSize = await row
    .locator(".task-row-title")
    .evaluate((element) =>
      Number.parseFloat(getComputedStyle(element).fontSize),
    );
  const settings = await settingsCategory(page, "Date & Time");
  await settings
    .getByRole("combobox", { name: "Start week on", exact: true })
    .selectOption("0");
  await settings
    .getByRole("switch", { name: "Show week numbers", exact: true })
    .check();
  await settings
    .getByRole("combobox", { name: "Time format", exact: true })
    .selectOption("12");
  await expect
    .poll(async () => {
      const settings = (await snapshot(page)).settings;
      return [
        settings.week_start,
        settings.show_week_numbers,
        settings.time_format,
      ];
    })
    .toEqual([0, true, 12]);
  await settingsCategory(page, "Smart Lists");
  await settings
    .getByRole("combobox", { name: "Tomorrow visibility", exact: true })
    .selectOption("hide");
  await expect
    .poll(
      async () =>
        ((await snapshot(page)).settings.smart_lists as Record<string, string>)
          .tomorrow,
    )
    .toBe("hide");
  await settingsCategory(page, "Appearance");
  await settings.getByRole("tab", { name: "Display", exact: true }).click();
  await settings.getByRole("button", { name: /^Font Size\s*Large$/ }).click();
  await expect
    .poll(async () => (await snapshot(page)).settings.font_size)
    .toBe("large");
  await closeSettings(page);
  await expect(
    page.getByRole("button", { name: /^Tomorrow(?: \d+)?$/ }),
  ).toBeHidden();
  const largerSize = await row
    .locator(".task-row-title")
    .evaluate((element) =>
      Number.parseFloat(getComputedStyle(element).fontSize),
    );
  expect(largerSize).toBeGreaterThan(initialSize);
  await page.getByRole("button", { name: "Set due date", exact: true }).click();
  const picker = page.getByRole("dialog", {
    name: "Task date and reminders",
    exact: true,
  });
  await expect(picker.getByRole("columnheader").nth(0)).toHaveText("W");
  await expect(picker.getByRole("columnheader").nth(1)).toHaveAttribute(
    "aria-label",
    "Sunday",
  );
  await picker.getByRole("button", { name: "Tomorrow", exact: true }).click();
  await picker.getByRole("button", { name: /^Time/ }).click();
  await picker.getByLabel("Due time", { exact: true }).fill("09:30");
  await picker.getByLabel("Due time", { exact: true }).press("Enter");
  await picker.getByRole("button", { name: "OK", exact: true }).click();
  await expect
    .poll(async () => (await getTask(page, task.id)).due_time)
    .toBe("09:30");
  await expect(
    page.getByRole("button", { name: "Set due date", exact: true }),
  ).toContainText(/9:30\s*AM/i);
  await page.reload();
  await expect(
    page.getByRole("button", { name: /^Tomorrow(?: \d+)?$/ }),
  ).toBeHidden();
});

test("a remapped keyboard shortcut opens quick add and survives reload", async ({
  page,
}) => {
  const settings = await settingsCategory(page, "Hotkeys");
  await settings
    .getByRole("button", { name: "Change Add task shortcut", exact: true })
    .click();
  await page.keyboard.press("Meta+Shift+Enter");
  await expect
    .poll(
      async () =>
        ((await snapshot(page)).settings.hotkeys as Record<string, string>)
          .quick_add,
    )
    .toBe("mod+shift+enter");
  await closeSettings(page);
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Loading tasks…", { exact: true })).toBeHidden();
  await page.keyboard.press("Meta+Shift+Enter");
  const quick = page.getByRole("dialog", { name: "Add Task", exact: true });
  await expect(quick).toBeVisible();
  const title = unique("Remapped shortcut");
  await quick
    .getByRole("textbox", { name: "Quick task title", exact: true })
    .fill(title);
  await quick.getByRole("button", { name: "Add Task", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Task title", exact: true }),
  ).toHaveValue(title);
  expect(
    (await snapshot(page)).tasks.some((task) => task.title === title),
  ).toBe(true);
});

test("Flat and Card change real frame geometry and persist across reloads", async ({
  page,
}) => {
  await expect(page.getByText("Loading tasks…", { exact: true })).toBeHidden();
  const geometry = () =>
    page.evaluate(() => {
      const frame = document.querySelector<HTMLElement>(".app-frame")!;
      const workspace = document.querySelector<HTMLElement>(".workspace")!;
      const rail = document.querySelector<HTMLElement>(".app-rail")!;
      const style = getComputedStyle(frame),
        paneStyle = getComputedStyle(workspace);
      const bounds = workspace.getBoundingClientRect();
      return {
        padding: [
          style.paddingTop,
          style.paddingRight,
          style.paddingBottom,
          style.paddingLeft,
        ],
        radius: paneStyle.borderTopLeftRadius,
        border: paneStyle.borderTopWidth,
        top: bounds.top,
        right: bounds.right,
        height: bounds.height,
        railWidth: rail.getBoundingClientRect().width,
      };
    });
  const card = await geometry();
  expect(card.padding).toEqual(["6px", "6px", "6px", "0px"]);
  expect(card.radius).toBe("15px");
  expect(card.railWidth).toBe(62);
  const settings = await settingsCategory(page, "Appearance");
  await settings
    .getByRole("button", { name: "Interface Style", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Interface Style",
    exact: true,
  });
  await expect(
    dialog.getByRole("radio", { name: "Card", exact: true }),
  ).toBeChecked();
  await dialog.getByRole("radio", { name: "Flat", exact: true }).click();
  await expect
    .poll(async () => (await snapshot(page)).settings.interface_style)
    .toBe("flat");
  await expect(
    dialog.getByRole("radio", { name: "Flat", exact: true }),
  ).toBeChecked();
  await page.screenshot({
    path: "output/playwright/interface-style-picker.png",
  });
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(settings).toBeVisible();
  await closeSettings(page);
  const flat = await geometry();
  expect(flat.padding).toEqual(["0px", "0px", "0px", "0px"]);
  expect(flat.radius).toBe("0px");
  expect(flat.border).toBe("0px");
  expect(flat.top).toBe(0);
  expect(flat.right).toBe(1470);
  expect(flat.height).toBe(923);
  expect(flat.railWidth).toBe(card.railWidth);
  await page.screenshot({ path: "output/playwright/interface-flat.png" });
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Loading tasks…", { exact: true })).toBeHidden();
  expect(await geometry()).toEqual(flat);
  await settingsCategory(page, "Appearance");
  await settings
    .getByRole("button", { name: "Interface Style", exact: true })
    .click();
  await dialog
    .getByRole("radio", { name: "Flat", exact: true })
    .press("ArrowRight");
  await expect
    .poll(async () => (await snapshot(page)).settings.interface_style)
    .toBe("card");
  await expect(
    dialog.getByRole("radio", { name: "Card", exact: true }),
  ).toBeChecked();
  await page.keyboard.press("Escape");
  await closeSettings(page);
  expect(await geometry()).toEqual(card);
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Loading tasks…", { exact: true })).toBeHidden();
  expect(await geometry()).toEqual(card);
  await page.screenshot({ path: "output/playwright/interface-card.png" });
});

test("app icon images load and Dock controls write persistent settings through the real service", async ({
  page,
}) => {
  const writes: Array<{ key: string; value: unknown }> = [];
  page.on("request", (request) => {
    if (request.method() !== "POST" || !request.url().endsWith("/__tasks"))
      return;
    const body = request.postDataJSON();
    if (
      body?.action === "settings.set" &&
      ["app_icon", "show_dock_icon", "dock_count"].includes(body.input?.key)
    )
      writes.push(body.input);
  });
  const settings = await settingsCategory(page, "Appearance");
  await settings.getByRole("tab", { name: "App icons", exact: true }).click();
  const defaultIcon = settings.getByRole("button", {
    name: "Default app icon",
    exact: true,
  });
  const roundedIcon = settings.getByRole("button", {
    name: "Rounded app icon",
    exact: true,
  });
  const paths: string[] = [];
  for (const button of [defaultIcon, roundedIcon]) {
    const image = button.locator("img");
    await expect(image).toBeVisible();
    await expect
      .poll(async () =>
        image.evaluate(
          (element: HTMLImageElement) =>
            element.complete &&
            element.naturalWidth >= 64 &&
            element.naturalHeight >= 64,
        ),
      )
      .toBe(true);
    paths.push((await image.getAttribute("src"))!);
  }
  expect(paths[0]).not.toBe(paths[1]);
  const [defaultResponse, roundedResponse] = await Promise.all(
    paths.map((path) => page.request.get(path)),
  );
  expect(defaultResponse!.ok()).toBe(true);
  expect(roundedResponse!.ok()).toBe(true);
  expect(
    (await defaultResponse!.body()).equals(await roundedResponse!.body()),
  ).toBe(false);
  await roundedIcon.click();
  await settings
    .getByRole("switch", { name: "Show app icon in Dock", exact: true })
    .uncheck();
  await settings
    .getByRole("combobox", {
      name: "Choose task to be counted in Dock",
      exact: true,
    })
    .selectOption("overdue");
  await expect
    .poll(async () => {
      const settings = (await snapshot(page)).settings;
      return [settings.app_icon, settings.show_dock_icon, settings.dock_count];
    })
    .toEqual(["rounded", false, "overdue"]);
  expect(writes).toEqual(
    expect.arrayContaining([
      { key: "app_icon", value: "rounded" },
      { key: "show_dock_icon", value: false },
      { key: "dock_count", value: "overdue" },
    ]),
  );
  await closeSettings(page);
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Loading tasks…", { exact: true })).toBeHidden();
  await settingsCategory(page, "Appearance");
  await settings.getByRole("tab", { name: "App icons", exact: true }).click();
  await expect(roundedIcon).toHaveAttribute("aria-pressed", "true");
  await expect(
    settings.getByRole("switch", {
      name: "Show app icon in Dock",
      exact: true,
    }),
  ).not.toBeChecked();
  await expect(
    settings.getByRole("combobox", {
      name: "Choose task to be counted in Dock",
      exact: true,
    }),
  ).toHaveValue("overdue");
  await page.screenshot({ path: "output/playwright/settings-app-icons.png" });
  await defaultIcon.click();
  await settings
    .getByRole("switch", { name: "Show app icon in Dock", exact: true })
    .check();
  await settings
    .getByRole("combobox", {
      name: "Choose task to be counted in Dock",
      exact: true,
    })
    .selectOption("none");
  await expect
    .poll(async () => {
      const settings = (await snapshot(page)).settings;
      return [settings.app_icon, settings.show_dock_icon, settings.dock_count];
    })
    .toEqual(["default", true, "none"]);
});
