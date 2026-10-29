import { expect } from "@playwright/test";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import type { Tag, TaskList } from "../../src/types";
import {
  call,
  closeSettings,
  localDay,
  resetSettings,
  settingsCategory,
  snapshot,
  test,
  unique,
} from "./helpers";

test("date and tag recognition keep or remove text without changing the selected metadata", async ({
  page,
}) => {
  await resetSettings(page);
  const tag = await call<Tag>(page.request, "tags.create", {
    name: unique("recognition").replaceAll(" ", "-"),
  });
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Recognition fixtures"),
  });
  const create = async (text: string, title: string) => {
    const input = page.getByRole("textbox", { name: "Add task", exact: true });
    await input.fill(text);
    await input.press("Enter");
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(title);
    const task = (await snapshot(page)).tasks.find(
      (item) => item.title === title,
    )!;
    expect(task).toBeTruthy();
    return task;
  };
  try {
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    const beforeOpening = (await snapshot(page)).revision;
    let settings = await settingsCategory(page, "More");
    const enabled = () =>
      settings.getByRole("switch", { name: "Date recognition", exact: true });
    const removeDate = () =>
      settings.getByRole("checkbox", {
        name: "Remove text in tasks",
        exact: true,
      });
    const removeTag = () =>
      settings.getByRole("checkbox", {
        name: "Remove tags from task name",
        exact: true,
      });
    await expect(enabled()).not.toBeChecked();
    await expect(removeDate()).toBeDisabled();
    expect((await snapshot(page)).revision).toBe(beforeOpening);
    await closeSettings(page);

    const literal = unique("Leave literal date") + " tomorrow at 09:30";
    const off = await create(literal, literal);
    expect(off.due_date).toBeNull();
    expect(off.due_time).toBeNull();

    settings = await settingsCategory(page, "More");
    await enabled().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.date_recognition)
      .toBe("remove");
    await expect(removeDate()).toBeChecked();
    await removeDate().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.date_recognition)
      .toBe("keep");
    await removeTag().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.tag_recognition)
      .toBe("keep");

    // Turning recognition off and on must remember the last text policy in
    // this panel rather than silently replacing Keep with Remove.
    await enabled().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.date_recognition)
      .toBe("off");
    await expect(removeDate()).toBeDisabled();
    await enabled().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.date_recognition)
      .toBe("keep");
    await closeSettings(page);
    await page.reload();
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    settings = await settingsCategory(page, "More");
    await expect(enabled()).toBeChecked();
    await expect(removeDate()).not.toBeChecked();
    await expect(removeTag()).not.toBeChecked();
    await page.screenshot({ path: "output/playwright/recognition-keep.png" });
    await closeSettings(page);
    const keptTitle = `${unique("Keep recognized text")} tomorrow at 09:30 #${tag.name}`;
    const kept = await create(keptTitle, keptTitle);
    expect(kept).toMatchObject({
      due_date: localDay(1),
      due_time: "09:30",
      tag_ids: [tag.id],
    });

    settings = await settingsCategory(page, "More");
    await removeDate().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.date_recognition)
      .toBe("remove");
    await removeTag().click();
    await expect
      .poll(async () => (await snapshot(page)).settings.tag_recognition)
      .toBe("remove");
    await closeSettings(page);
    const removedTitle = unique("Remove recognized text");
    const removed = await create(
      `${removedTitle} tomorrow at 09:30 #${tag.name}`,
      removedTitle,
    );
    expect(removed).toMatchObject({
      due_date: kept.due_date,
      due_time: kept.due_time,
      tag_ids: kept.tag_ids,
    });
    await page.reload();
    settings = await settingsCategory(page, "More");
    await expect(removeDate()).toBeChecked();
    await expect(removeTag()).toBeChecked();
  } finally {
    // A wrong title is one of the regressions under test; discover fixtures
    // by their dedicated list so that failure still cleans up every write.
    for (const task of (await snapshot(page)).tasks.filter(
      (item) => item.list_id === list.id && !item.deleted_at,
    ))
      await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
    await call(page.request, "tags.delete", { id: tag.id });
  }
});

test("a rejected notification setting keeps its last acknowledged checkbox state", async ({
  page,
}) => {
  await resetSettings(page);
  await call(page.request, "settings.set", {
    key: "notifications",
    value: false,
  });
  const before = await snapshot(page);
  // This component boundary models a parent waiting for/refusing a native
  // permission request. It does not claim to test macOS permission delivery.
  // Serve the fixture through the real Vite server. A synthetic fulfilled
  // navigation has no network address and Chrome blocks its localhost socket.
  await writeFile(
    resolve("output/playwright/notification-ack.html"),
    `<!doctype html><html><head><meta charset="utf-8"></head><body><div id="fixture"></div>
      <script type="module">
      import { createApp, h, ref } from '/node_modules/.vite/deps/vue.js';
      import SettingsPanel from '/src/components/SettingsPanel.vue';
      import '/src/style.css';
      const settings = ref(${JSON.stringify(before.settings)});
      window.settingAttempts = [];
      window.acknowledgeSettings = value => { settings.value = value; };
      createApp({ render: () => h(SettingsPanel, {
        settings: settings.value, lists: [], initialSection: 'notifications',
        'onUpdate-setting': (key, value) => window.settingAttempts.push({ key, value })
      }) }).mount('#fixture');
      </script></body></html>`,
  );
  try {
    await page.goto("/output/playwright/notification-ack.html");
    const input = page.getByRole("switch", {
      name: "Task reminders",
      exact: true,
    });
    await expect(input).not.toBeChecked();
    await input.click();
    await expect(input).not.toBeChecked();
    expect(await page.evaluate(() => (window as any).settingAttempts)).toEqual([
      { key: "notifications", value: true },
    ]);
    expect((await snapshot(page)).revision).toBe(before.revision);

    // Simulate a later acknowledged write using the real shared service, then
    // verify a failed disable request cannot make the visual state lie either.
    await call(page.request, "settings.set", {
      key: "notifications",
      value: true,
    });
    const acknowledged = await snapshot(page);
    await page.evaluate(
      (value) => (window as any).acknowledgeSettings(value),
      acknowledged.settings,
    );
    await expect(input).toBeChecked();
    await input.click();
    await expect(input).toBeChecked();
    expect(await page.evaluate(() => (window as any).settingAttempts)).toEqual([
      { key: "notifications", value: true },
      { key: "notifications", value: false },
    ]);
    expect((await snapshot(page)).revision).toBe(acknowledged.revision);
  } finally {
    await call(page.request, "settings.set", {
      key: "notifications",
      value: false,
    });
  }
});
