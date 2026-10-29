import { expect } from "@playwright/test";
import {
  call,
  closeSettings,
  resetSettings,
  settingsCategory,
  snapshot,
  test,
} from "./helpers";

test("login preferences and both global shortcuts persist, reject conflicts and reset", async ({
  page,
}) => {
  const defaults = {
    launch_at_login: false,
    launch_minimized: false,
    global_quick_add_shortcut: "",
    global_show_hide_shortcut: "",
  };
  await resetSettings(page);
  for (const [key, value] of Object.entries(defaults))
    await call(page.request, "settings.set", { key, value });

  try {
    await page.goto("/");
    await expect(
      page.getByRole("heading", { name: "Inbox", exact: true }),
    ).toBeVisible();
    let settings = await settingsCategory(page, "More");
    const launch = () =>
      settings.getByRole("switch", { name: "Launch at Login", exact: true });
    const minimize = () =>
      settings.getByRole("switch", {
        name: "Minimize app when auto launching",
        exact: true,
      });
    await expect(launch()).not.toBeChecked();
    await expect(minimize()).not.toBeChecked();
    await launch().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.launch_at_login)
      .toBe(true);
    await expect(launch()).toBeChecked();
    await minimize().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.launch_minimized)
      .toBe(true);
    await expect(minimize()).toBeChecked();

    settings = await settingsCategory(page, "Hotkeys");
    await expect(
      settings.getByRole("heading", { name: "Global Action", exact: true }),
    ).toBeVisible();
    const recorder = () =>
      settings.getByRole("button", {
        name: "Change Quick Add shortcut",
        exact: true,
      });
    const clear = () =>
      settings.getByRole("button", {
        name: "Clear Quick Add shortcut",
        exact: true,
      });
    const showHide = () =>
      settings.getByRole("button", {
        name: "Change Show/Hide Main Window shortcut",
        exact: true,
      });
    const clearShowHide = () =>
      settings.getByRole("button", {
        name: "Clear Show/Hide Main Window shortcut",
        exact: true,
      });
    await expect(recorder()).toHaveText("Not set");
    await expect(clear()).toHaveCount(0);
    await recorder().click();
    await page.keyboard.press("Control+Shift+A");
    await expect
      .poll(
        async () => (await snapshot(page)).settings.global_quick_add_shortcut,
      )
      .toBe("ctrl+shift+a");
    await expect(recorder()).toHaveText("⌃⇧A");
    await expect(showHide()).toHaveText("Not set");
    const beforeConflict = (await snapshot(page)).revision;
    await showHide().click();
    await page.keyboard.press("Control+Shift+A");
    await expect(settings.getByRole("alert")).toHaveText(
      "This shortcut is already in use. Choose another combination.",
    );
    expect((await snapshot(page)).revision).toBe(beforeConflict);
    await page.keyboard.press("Control+Shift+Y");
    await expect
      .poll(
        async () => (await snapshot(page)).settings.global_show_hide_shortcut,
      )
      .toBe("ctrl+shift+y");
    await expect(showHide()).toHaveText("⌃⇧Y");

    await closeSettings(page);
    await page.reload();
    settings = await settingsCategory(page, "More");
    await expect(launch()).toBeChecked();
    await expect(minimize()).toBeChecked();
    settings = await settingsCategory(page, "Hotkeys");
    await expect(recorder()).toHaveText("⌃⇧A");
    await expect(showHide()).toHaveText("⌃⇧Y");
    await page.screenshot({ path: "output/playwright/global-shortcuts.png" });

    // Rejected recordings must leave the last valid value and database intact.
    const revision = (await snapshot(page)).revision;
    await recorder().click();
    await page.keyboard.press("a");
    await expect(settings.getByRole("alert")).toHaveText(
      "Use Command, Control or Option with a key. Global shortcuts cannot use sequences.",
    );
    let saved = await snapshot(page);
    expect(saved.settings.global_quick_add_shortcut).toBe("ctrl+shift+a");
    expect(saved.revision).toBe(revision);
    await page.keyboard.press("Meta+q");
    await expect(settings.getByRole("alert")).toHaveText(
      "This shortcut is reserved by macOS. Choose another combination.",
    );
    saved = await snapshot(page);
    expect(saved.settings.global_quick_add_shortcut).toBe("ctrl+shift+a");
    expect(saved.revision).toBe(revision);
    await page.keyboard.press("Escape");
    await expect(recorder()).toHaveText("⌃⇧A");

    // Start a fresh recording to dismiss its validation message before clearing.
    await recorder().click();
    await page.keyboard.press("Escape");
    await clear().click();
    await expect(recorder()).toHaveText("Not set");
    await expect(clear()).toHaveCount(0);
    await expect
      .poll(
        async () => (await snapshot(page)).settings.global_quick_add_shortcut,
      )
      .toBe("");
    await showHide().focus();
    await clearShowHide().click();
    await expect
      .poll(
        async () => (await snapshot(page)).settings.global_show_hide_shortcut,
      )
      .toBe("");
    await expect(showHide()).toHaveText("Not set");
    await expect(clearShowHide()).toHaveCount(0);

    await recorder().click();
    await page.keyboard.press("Control+Shift+A");
    await expect(recorder()).toHaveText("⌃⇧A");
    await showHide().click();
    await page.keyboard.press("Control+Shift+Y");
    await expect(showHide()).toHaveText("⌃⇧Y");
    await settings
      .getByRole("button", { name: "Reset Default", exact: true })
      .click();
    await expect
      .poll(async () => {
        const { settings: values } = await snapshot(page);
        return {
          global: values.global_quick_add_shortcut,
          showHide: values.global_show_hide_shortcut,
          local: values.hotkeys,
        };
      })
      .toEqual({ global: "", showHide: "", local: {} });
    await expect(recorder()).toHaveText("Not set");
    await expect(clear()).toHaveCount(0);
    await expect(showHide()).toHaveText("Not set");
    await expect(clearShowHide()).toHaveCount(0);
    await expect(settings.getByRole("alert")).toHaveCount(0);
    await closeSettings(page);
    await page.reload();
    settings = await settingsCategory(page, "Hotkeys");
    await expect(recorder()).toHaveText("Not set");
    await expect(showHide()).toHaveText("Not set");
    settings = await settingsCategory(page, "More");
    await expect(launch()).toBeChecked();
    await expect(minimize()).toBeChecked();
  } finally {
    for (const [key, value] of Object.entries(defaults))
      await call(page.request, "settings.set", { key, value });
    await call(page.request, "settings.set", { key: "hotkeys", value: {} });
  }
});
