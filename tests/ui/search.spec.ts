import { expect } from "@playwright/test";
import { test, call, resetSettings, unique } from "./helpers";

test("GUI and Rust search rendered notes consistently without destroying literal plain text", async ({
  page,
}) => {
  await resetSettings(page);
  const token = unique("search").replaceAll(" ", "-");
  const cases = [
    {
      label: "HTML entities",
      notes_format: "html",
      notes: `<p>${token} AT&amp;T &lt;team&gt;</p>`,
      query: `${token} AT&T <team>`,
    },
    {
      label: "Inline word",
      notes_format: "html",
      notes: `<p>${token} hel<strong>lo</strong> team</p>`,
      query: `${token} hello team`,
    },
    {
      label: "Inline phrase",
      notes_format: "html",
      notes: `<p>${token} ready <em>for</em> release</p>`,
      query: `${token} ready for release`,
    },
    {
      label: "Literal plain text",
      notes_format: "plain",
      notes: `${token} literal <example> text`,
      query: `${token} literal <example> text`,
    },
    {
      label: "Markdown phrase",
      notes_format: "markdown",
      notes: `## ${token} **offline** launch\n\nKeep this local.`,
      query: `${token} offline launch`,
    },
  ];
  const tasks = [];
  for (const item of cases)
    tasks.push(
      await call(page.request, "tasks.create", {
        title: unique(item.label),
        notes: item.notes,
        notes_format: item.notes_format,
      }),
    );
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Inbox", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Loading tasks…", { exact: true })).toBeHidden();
  await page.keyboard.press("Meta+f");
  const input = page.getByRole("textbox", {
    name: "Search tasks and notes",
    exact: true,
  });
  await expect(input).toBeFocused();
  for (let index = 0; index < cases.length; index++) {
    const current = cases[index]!,
      task = tasks[index]!;
    const matches = await call(page.request, "tasks.list", {
      search: current.query,
    });
    expect(
      matches.map((item: { id: string }) => item.id),
      current.label,
    ).toEqual([task.id]);
    await input.fill(current.query);
    await expect(
      page.getByRole("button", {
        name: `Open task ${task.title}`,
        exact: true,
      }),
    ).toBeVisible();
    await expect(page.getByRole("dialog", { name: "Search", exact: true }).locator(".search-result")).toHaveCount(1);
    const stored = await call(page.request, "tasks.get", { id: task.id });
    expect(stored.notes).toBe(current.notes);
    expect(stored.notes_format).toBe(current.notes_format);
  }
  await page.screenshot({ path: "output/playwright/rendered-note-search.png" });
});

test("search dialog finds every supported type and supports keyboard, clear, cancel and mobile", async ({ page }) => {
  await resetSettings(page);
  const token = unique("Global search");
  const original = await call(page.request, "snapshot");
  const list = await call(page.request, "lists.create", { name: `${token} list` });
  const tag = await call(page.request, "tags.create", { name: `${token} tag` });
  const task = await call(page.request, "tasks.create", { title: `${token} task`, list_id: list.id });
  const filter = { id: `${token}-filter`, name: `${token} filter`, list_ids: [list.id], tag_ids: [], priorities: [], date: "all", match: "all" };
  await call(page.request, "settings.set", { key: "saved_filters", value: [...(original.settings.saved_filters || []), filter] });
  const dialog = page.getByRole("dialog", { name: "Search", exact: true });
  const input = dialog.getByRole("textbox", { name: "Search tasks and notes" });
  const open = async () => {
    await page.getByRole("button", { name: "Search tasks", exact: true }).click();
    await expect(input).toBeFocused();
  };
  try {
    await page.goto("/");
    await page.getByRole("heading", { name: "Inbox", exact: true }).waitFor();
    await open();
    await expect(dialog).toContainText("Search tasks, tags, lists and filters");
    await expect(dialog.locator(".search-result")).toHaveCount(0);
    await page.screenshot({ path: "output/playwright/search-empty-desktop.png" });
    await input.press("Shift+Tab");
    await expect(dialog.getByRole("button", { name: "Close search" })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(input).toBeFocused();
    await input.fill(token);
    await expect(dialog.locator(".search-result")).toHaveCount(4);
    await page.screenshot({ path: "output/playwright/search-results-desktop.png" });
    await input.press("ArrowDown");
    await expect(dialog.getByRole("button", { name: `Open task ${task.title}` })).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(dialog).toBeHidden();
    await expect(page.getByRole("textbox", { name: "Task title", exact: true })).toHaveValue(task.title);
    for (const [kind, item] of [["tag", tag], ["list", list], ["filter", filter]] as const) {
      await open();
      await input.fill(token);
      await dialog.getByRole("button", { name: `Open ${kind} ${item.name}`, exact: true }).click();
      await expect(dialog).toBeHidden();
      await expect(page.getByRole("heading", { name: item.name, exact: true })).toBeVisible();
    }
    await open();
    await input.fill(`${token} absent`);
    await expect(dialog).toContainText("No results found");
    await dialog.getByRole("button", { name: "Clear search" }).click();
    await expect(input).toHaveValue("");
    await expect(input).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(page.getByRole("heading", { name: filter.name, exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Search tasks", exact: true })).toBeFocused();
    await page.setViewportSize({ width: 390, height: 844 });
    await open();
    await page.screenshot({ path: "output/playwright/search-empty-mobile.png" });
    await input.fill(token);
    await expect(dialog.locator(".search-result")).toHaveCount(4);
    await page.screenshot({ path: "output/playwright/search-results-mobile.png" });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await dialog.getByRole("button", { name: `Open task ${task.title}`, exact: true }).click();
    await expect(page.getByRole("textbox", { name: "Task title", exact: true })).toHaveValue(task.title);
  } finally {
    await call(page.request, "settings.set", { key: "saved_filters", value: original.settings.saved_filters || [] });
    await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "lists.delete", { id: list.id });
    await call(page.request, "tags.delete", { id: tag.id });
  }
});
