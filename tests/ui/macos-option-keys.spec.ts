import { expect, type Locator } from "@playwright/test";
import type { Section, Task, TaskList } from "../../src/types";
import {
  test,
  call,
  closeSettings,
  resetSettings,
  settingsCategory,
  snapshot,
  unique,
} from "./helpers";

const optionCases = [
  {
    name: "Option character",
    key: "©",
    code: "KeyG",
    ctrlKey: false,
    chord: "mod+alt+g",
    display: "⌘⌥G",
  },
  {
    name: "Option dead key",
    key: "Dead",
    code: "KeyE",
    ctrlKey: false,
    chord: "mod+alt+e",
    display: "⌘⌥E",
  },
  {
    name: "Command Control Option character",
    key: "∆",
    code: "KeyJ",
    ctrlKey: true,
    chord: "mod+ctrl+alt+j",
    display: "⌘⌃⌥J",
  },
];

async function dispatchOptionKey(
  target: Locator,
  combination: (typeof optionCases)[number],
) {
  // Exercise the actual DOM listeners with the character/code pair produced
  // by a macOS layout. This does not emulate OS-level global key delivery.
  return target.evaluate((element, value) => {
    const event = new KeyboardEvent("keydown", {
      key: value.key,
      code: value.code,
      metaKey: true,
      ctrlKey: value.ctrlKey,
      altKey: true,
      shiftKey: false,
      isComposing: false,
      bubbles: true,
      cancelable: true,
    });
    element.dispatchEvent(event);
    return event.defaultPrevented;
  }, combination);
}

test("macOS Option characters record stable local shortcuts and retain every modifier after reload", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Option keyboard shortcuts"),
    group_by: "section",
    sort_by: "manual",
  });
  try {
    for (const label of ["First", "Second"]) {
      const section = await call<Section>(page.request, "sections.create", {
        list_id: list.id,
        name: unique(`${label} shortcut group`),
      });
      await call<Task>(page.request, "tasks.create", {
        title: unique(`${label} shortcut task`),
        list_id: list.id,
        section_id: section.id,
      });
    }
    const initialTasks = (await snapshot(page)).tasks.filter(
      (task) => task.list_id === list.id && !task.deleted_at,
    );
    const rows = page.locator(".task-list-scroll .task-row");
    const groupButtons = page.locator(".task-group-toggle");
    const openList = async () => {
      await page
        .getByRole("treeitem", { name: list.name, exact: true })
        .click();
      await expect(
        page.getByRole("heading", { name: list.name, exact: true }),
      ).toBeVisible();
      await expect(rows).toHaveCount(2);
      await expect(groupButtons).toHaveCount(2);
    };
    await page.goto("/");
    expect(await page.evaluate(() => navigator.platform)).toMatch(/Mac/);
    await openList();

    for (const combination of optionCases) {
      await test.step(combination.name, async () => {
        const settings = await settingsCategory(page, "Hotkeys");
        const recorder = settings.getByRole("button", {
          name: "Change Toggle all groups shortcut",
          exact: true,
        });
        await recorder.click();
        await expect(recorder).toHaveText("Type shortcut");
        expect(await dispatchOptionKey(recorder, combination)).toBe(true);
        await expect
          .poll(async () => {
            const hotkeys = (await snapshot(page)).settings.hotkeys as Record<
              string,
              string
            >;
            return hotkeys.toggle_all_groups;
          })
          .toBe(combination.chord);
        await expect(recorder).toHaveText(combination.display);
        await expect(settings.getByRole("alert")).toHaveCount(0);
        await closeSettings(page);

        expect(await dispatchOptionKey(page.locator("body"), combination)).toBe(
          true,
        );
        await expect(rows).toHaveCount(0);
        await expect(groupButtons.first()).toBeVisible();
        await expect(groupButtons.last()).toBeVisible();
        expect(await dispatchOptionKey(page.locator("body"), combination)).toBe(
          true,
        );
        await expect(rows).toHaveCount(2);

        await page.reload();
        await openList();
        const hotkeys = (await snapshot(page)).settings.hotkeys as Record<
          string,
          string
        >;
        expect(hotkeys.toggle_all_groups).toBe(combination.chord);
        expect(await dispatchOptionKey(page.locator("body"), combination)).toBe(
          true,
        );
        await expect(rows).toHaveCount(0);
        expect(await dispatchOptionKey(page.locator("body"), combination)).toBe(
          true,
        );
        await expect(rows).toHaveCount(2);
      });
    }
    expect(
      (await snapshot(page)).tasks.filter(
        (task) => task.list_id === list.id && !task.deleted_at,
      ),
    ).toEqual(initialTasks);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    for (const task of (await snapshot(page)).tasks.filter(
      (task) => task.list_id === list.id && !task.deleted_at,
    ))
      await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});
