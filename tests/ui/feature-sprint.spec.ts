import { expect, type Page } from "@playwright/test";
import type { Task, TaskList } from "../../src/types";
import {
  test,
  call,
  getTask,
  localDay,
  openTask,
  resetSettings,
  snapshot,
  unique,
} from "./helpers";

const noPageOverflow = async (page: Page) =>
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);

test("task progress persists and Task Activities restores earlier content without undoing progress", async ({
  page,
}) => {
  await resetSettings(page);
  const task = await call<Task>(page.request, "tasks.create", {
    title: unique("History original"),
    notes: "Original description kept by content restoration.",
    notes_format: "plain",
  });
  const checklist = await call<Task>(page.request, "tasks.create", {
    title: unique("Automatic progress"),
    checklist: [
      { id: "checked", text: "Already finished", completed: true },
      { id: "remaining", text: "Remaining item", completed: false },
    ],
  });
  const changedTitle = unique("History edited");
  try {
    await page.goto("/");
    await openTask(page, task.title);
    const slider = page.getByRole("slider", {
      name: "Task progress",
      exact: true,
    });
    await slider.focus();
    await slider.press("ArrowRight");
    await expect
      .poll(async () => (await getTask(page, task.id)).progress)
      .toBe(1);
    await page.reload();
    await openTask(page, task.title);
    await expect(slider).toHaveValue("1");
    await page
      .getByRole("textbox", { name: "Task title", exact: true })
      .fill(changedTitle);
    await slider.focus();
    await expect
      .poll(async () => (await getTask(page, task.id)).title)
      .toBe(changedTitle);
    await page
      .getByRole("button", { name: "Task actions", exact: true })
      .click();
    await page
      .getByRole("menuitem", { name: "Task Activities", exact: true })
      .click();
    const history = page.getByRole("dialog", {
      name: "Task Activities",
      exact: true,
    });
    await expect(history).toBeVisible();
    await expect(
      history.getByRole("heading", { name: changedTitle, exact: true }),
    ).toBeVisible();
    await page.screenshot({
      path: "output/playwright/sprint-task-history.png",
    });
    await history
      .getByRole("button", { name: "Restore previous content", exact: true })
      .click();
    await expect(history.getByRole("status")).toHaveText(
      "Title and description restored.",
    );
    const restored = await getTask(page, task.id);
    expect(restored.title).toBe(task.title);
    expect(restored.notes).toBe(task.notes);
    expect(restored.progress).toBe(1);
    await history
      .getByRole("button", { name: "Close dialog", exact: true })
      .click();
    await openTask(page, checklist.title);
    const automatic = page.getByRole("progressbar", {
      name: "Checklist progress",
      exact: true,
    });
    await expect(automatic).toHaveAttribute("aria-valuenow", "50");
    await expect(
      page.getByRole("slider", { name: "Task progress", exact: true }),
    ).toHaveCount(0);
    await page
      .getByRole("button", { name: "Check Remaining item", exact: true })
      .click();
    await expect
      .poll(async () => (await getTask(page, checklist.id)).progress)
      .toBe(100);
    await expect(automatic).toHaveAttribute("aria-valuenow", "100");
    await page.reload();
    await openTask(page, checklist.title);
    await expect(automatic).toHaveAttribute("aria-valuenow", "100");
  } finally {
    await call(page.request, "tasks.delete", { id: task.id });
    await call(page.request, "tasks.delete", { id: checklist.id });
  }
});

test("Summary filters, saved templates and note creation retain the selected tasks and Markdown", async ({
  page,
}) => {
  await resetSettings(page);
  const previous = (await snapshot(page)).settings;
  await call(page.request, "settings.set", {
    key: "smart_lists",
    value: {
      ...(previous.smart_lists as Record<string, unknown>),
      summary: "show",
    },
  });
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Summary project"),
  });
  const created: Task[] = [];
  const templateName = unique("Priority report");
  let noteId: string | undefined;
  try {
    for (const [label, priority, offset] of [
      ["Included high priority", 3, 0],
      ["Excluded low priority", 1, 0],
      ["Excluded later task", 3, 8],
    ] as const)
      created.push(
        await call<Task>(page.request, "tasks.create", {
          title: unique(label),
          list_id: list.id,
          priority,
          due_date: localDay(offset),
        }),
      );
    await page.goto("/");
    await page.getByRole("button", { name: "Summary", exact: true }).click();
    const summary = page.getByRole("region", {
      name: "Task summary",
      exact: true,
    });
    await summary
      .getByLabel("Summary date basis", { exact: true })
      .selectOption("due");
    await summary
      .getByLabel("Summary start date", { exact: true })
      .fill(localDay());
    await summary
      .getByLabel("Summary end date", { exact: true })
      .fill(localDay(1));
    await summary
      .getByText("Filters & display options", { exact: true })
      .click();
    await summary
      .getByLabel("Summary lists", { exact: true })
      .selectOption(list.id);
    await summary
      .getByRole("checkbox", { name: "Completed", exact: true })
      .uncheck();
    await summary
      .getByRole("checkbox", { name: "Active", exact: true })
      .check();
    await summary.getByRole("checkbox", { name: "High", exact: true }).check();
    const preview = summary.getByRole("textbox", {
      name: "Summary text preview",
      exact: true,
    });
    await expect.poll(() => preview.inputValue()).toContain(created[0].title);
    expect(await preview.inputValue()).not.toContain(created[1].title);
    expect(await preview.inputValue()).not.toContain(created[2].title);
    await summary
      .getByRole("button", { name: "Save summary template", exact: true })
      .click();
    await summary
      .getByRole("textbox", { name: "Summary template name", exact: true })
      .fill(templateName);
    await summary
      .getByRole("button", { name: "Save Template", exact: true })
      .click();
    await expect
      .poll(async () =>
        (
          (await snapshot(page)).settings.summary_templates as {
            name: string;
          }[]
        ).some((item) => item.name === templateName),
      )
      .toBe(true);
    await summary
      .getByRole("checkbox", { name: "High", exact: true })
      .uncheck();
    await expect.poll(() => preview.inputValue()).toContain(created[1].title);
    await summary
      .getByLabel("Summary template", { exact: true })
      .selectOption({ label: templateName });
    await expect
      .poll(() => preview.inputValue())
      .not.toContain(created[1].title);
    await page.screenshot({ path: "output/playwright/sprint-summary.png" });
    const expectedMarkdown = await preview.inputValue();
    await summary
      .getByRole("button", { name: "Create Note", exact: true })
      .click();
    await expect
      .poll(async () => {
        const note = (await snapshot(page)).tasks.find(
          (item) =>
            item.is_note && item.notes === expectedMarkdown && !item.deleted_at,
        );
        noteId = note?.id;
        return note?.notes_format;
      })
      .toBe("markdown");
    await expect(
      page.getByRole("textbox", { name: "Task title", exact: true }),
    ).toHaveValue(`Summary · ${localDay()} – ${localDay(1)}`);
    await noPageOverflow(page);
  } finally {
    for (const task of created)
      await call(page.request, "tasks.delete", { id: task.id });
    if (noteId) await call(page.request, "tasks.delete", { id: noteId });
    await call(page.request, "lists.delete", { id: list.id });
    await call(page.request, "settings.set", {
      key: "summary_templates",
      value: previous.summary_templates || [],
    });
  }
});

test("Timeline moves and resizes task dates, schedules undated tasks and retains the selected view", async ({
  page,
}) => {
  await resetSettings(page);
  const list = await call<TaskList>(page.request, "lists.create", {
    name: unique("Timeline project"),
  });
  const scheduled = await call<Task>(page.request, "tasks.create", {
    title: unique("Timeline duration"),
    list_id: list.id,
    start_date: localDay(),
    due_date: localDay(2),
  });
  const undated = await call<Task>(page.request, "tasks.create", {
    title: unique("Timeline unplanned"),
    list_id: list.id,
  });
  try {
    await page.goto("/");
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await page
      .getByRole("button", { name: "List actions", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Timeline View", exact: true })
      .click();
    const timeline = page.getByRole("region", {
      name: "Task timeline",
      exact: true,
    });
    const bar = timeline.getByRole("button", {
      name: `Open timeline task ${scheduled.title}`,
      exact: true,
    });
    await bar.focus();
    await bar.press("Alt+ArrowRight");
    await expect
      .poll(async () => {
        const task = await getTask(page, scheduled.id);
        return [task.start_date, task.due_date];
      })
      .toEqual([localDay(1), localDay(3)]);
    await timeline
      .getByRole("button", {
        name: `Resize end of ${scheduled.title}`,
        exact: true,
      })
      .press("ArrowRight");
    await expect
      .poll(async () => (await getTask(page, scheduled.id)).due_date)
      .toBe(localDay(4));
    expect((await getTask(page, scheduled.id)).start_date).toBe(localDay(1));
    await timeline
      .getByRole("button", {
        name: `Schedule ${undated.title} today`,
        exact: true,
      })
      .click();
    await expect
      .poll(async () => (await getTask(page, undated.id)).due_date)
      .toBe(localDay());
    await expect(
      timeline.getByRole("button", {
        name: `Schedule ${undated.title} today`,
        exact: true,
      }),
    ).toHaveCount(0);
    await timeline
      .getByRole("group", { name: "Timeline scale", exact: true })
      .getByRole("button", { name: "Day", exact: true })
      .click();
    await noPageOverflow(page);
    await page.screenshot({ path: "output/playwright/sprint-timeline.png" });
    const dragTarget = await bar.boundingBox();
    expect(dragTarget).toBeTruthy();
    await page.mouse.move(
      dragTarget!.x + 35,
      dragTarget!.y + dragTarget!.height / 2,
    );
    await page.mouse.down();
    await page.mouse.move(
      dragTarget!.x + 99,
      dragTarget!.y + dragTarget!.height / 2,
      { steps: 8 },
    );
    await page.mouse.up();
    await expect
      .poll(async () => {
        const moved = await getTask(page, scheduled.id);
        return [moved.start_date, moved.due_date];
      })
      .toEqual([localDay(2), localDay(5)]);
    await page.reload();
    await page.getByRole("treeitem", { name: list.name, exact: true }).click();
    await expect(timeline).toBeVisible();
    await expect(
      timeline.getByRole("button", {
        name: `Open timeline task ${undated.title}`,
        exact: true,
      }),
    ).toBeVisible();
  } finally {
    await call(page.request, "tasks.delete", { id: scheduled.id });
    await call(page.request, "tasks.delete", { id: undated.id });
    await call(page.request, "lists.delete", { id: list.id });
  }
});

test("mobile multiline paste previews French and English dates before creating only the selected tasks", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await resetSettings(page);
  await call(page.request, "settings.set", {
    key: "date_recognition",
    value: "remove",
  });
  const prefix = unique("Batch sprint");
  const first = `${prefix} review`;
  const second = `${prefix} appel`;
  const omitted = `${prefix} omitted`;
  try {
    await page.goto("/");
    const input = page.getByRole("textbox", { name: "Add task", exact: true });
    await input.evaluate((element, text) => {
      const clipboardData = new DataTransfer();
      clipboardData.setData("text/plain", text);
      element.dispatchEvent(
        new ClipboardEvent("paste", {
          bubbles: true,
          cancelable: true,
          clipboardData,
        }),
      );
    }, `${first} tomorrow at 9am\n${second} dans 3 jours à 14h\n${omitted}`);
    const dialog = page.getByRole("dialog", {
      name: "Add Multiple Tasks",
      exact: true,
    });
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByLabel("Preview task 1", { exact: true }),
    ).toContainText(`${localDay(1)} 09:00`);
    await expect(
      dialog.getByLabel("Preview task 2", { exact: true }),
    ).toContainText(`${localDay(3)} 14:00`);
    await dialog
      .getByRole("checkbox", { name: "Include task 3", exact: true })
      .uncheck();
    expect(
      (await snapshot(page)).tasks.some((task) =>
        task.title.startsWith(prefix),
      ),
    ).toBe(false);
    await noPageOverflow(page);
    await page.screenshot({
      path: "output/playwright/sprint-batch-mobile.png",
    });
    await dialog
      .getByRole("button", { name: "Add 2 Tasks", exact: true })
      .click();
    await expect(dialog).toBeHidden();
    const created = (await snapshot(page)).tasks
      .filter((task) => task.title.startsWith(prefix) && !task.deleted_at)
      .sort((a, b) => a.position - b.position);
    expect(
      created.map((task) => [task.title, task.due_date, task.due_time]),
    ).toEqual([
      [first, localDay(1), "09:00"],
      [second, localDay(3), "14:00"],
    ]);
    await noPageOverflow(page);
    await page.reload();
    expect(
      (await snapshot(page)).tasks.filter(
        (task) => task.title.startsWith(prefix) && !task.deleted_at,
      ),
    ).toHaveLength(2);
  } finally {
    for (const task of (await snapshot(page)).tasks.filter(
      (task) => task.title.startsWith(prefix) && !task.deleted_at,
    ))
      await call(page.request, "tasks.delete", { id: task.id });
  }
});

test("reminder actions persist Done and Dismiss while Snooze changes only the reminder", async ({
  page,
}) => {
  await resetSettings(page);
  const reminderAt = new Date(Date.now() - 60_000).toISOString();
  const created: Task[] = [];
  for (const label of ["Snooze task", "Dismiss task", "Done task"])
    created.push(
      await call<Task>(page.request, "tasks.create", {
        title: unique(label),
        due_date: localDay(),
        due_time: "09:00",
        reminders: [reminderAt],
      }),
    );
  const [snoozed, dismissed, done] = created;
  try {
    await call(page.request, "reminders.capture", {
      now: new Date().toISOString(),
    });
    await page.goto("/?reminders");
    const panel = page.getByRole("main", {
      name: "Task reminders",
      exact: true,
    });
    const snoozeCard = panel.getByRole("article", {
      name: snoozed.title,
      exact: true,
    });
    await expect(snoozeCard).toBeVisible();
    await page.screenshot({ path: "output/playwright/sprint-reminders.png" });
    await snoozeCard
      .getByLabel(`Snooze duration for ${snoozed.title}`, { exact: true })
      .selectOption("30");
    const beforeSnooze = Date.now();
    await snoozeCard
      .getByRole("button", { name: "Snooze", exact: true })
      .click();
    await expect(snoozeCard).toHaveCount(0);
    const saved = await getTask(page, snoozed.id);
    expect(saved.due_date).toBe(snoozed.due_date);
    expect(saved.due_time).toBe(snoozed.due_time);
    expect(saved.completed_at).toBeNull();
    expect(saved.reminders).toHaveLength(1);
    expect(Date.parse(saved.reminders[0])).toBeGreaterThanOrEqual(
      beforeSnooze + 30 * 60_000 - 1000,
    );
    expect(Date.parse(saved.reminders[0])).toBeLessThanOrEqual(
      Date.now() + 30 * 60_000 + 1000,
    );
    const dismissCard = panel.getByRole("article", {
      name: dismissed.title,
      exact: true,
    });
    await dismissCard
      .getByRole("button", { name: "Dismiss", exact: true })
      .click();
    await expect(dismissCard).toHaveCount(0);
    expect((await getTask(page, dismissed.id)).completed_at).toBeNull();
    const doneCard = panel.getByRole("article", {
      name: done.title,
      exact: true,
    });
    await doneCard.getByRole("button", { name: "Done", exact: true }).click();
    await expect(doneCard).toHaveCount(0);
    expect((await getTask(page, done.id)).completed_at).not.toBeNull();
    await page.reload();
    for (const task of created)
      await expect(
        panel.getByRole("article", { name: task.title, exact: true }),
      ).toHaveCount(0);
    const pending = await call<Task[]>(page.request, "reminders.pending");
    expect(
      pending.filter((task) => created.some((item) => item.id === task.id)),
    ).toHaveLength(0);
    await noPageOverflow(page);
  } finally {
    for (const task of created)
      await call(page.request, "tasks.delete", { id: task.id });
  }
});

test("Quick Add batch paste keeps explicit priority and No Date above recognized metadata", async ({
  page,
}) => {
  await resetSettings(page);
  await call(page.request, "settings.set", {
    key: "date_recognition",
    value: "remove",
  });
  const prefix = unique("Explicit batch");
  try {
    await page.goto("/");
    await page.keyboard.press("Meta+n");
    const quick = page.getByRole("dialog", { name: "Add Task", exact: true });
    await quick
      .getByRole("button", { name: "Quick priority", exact: true })
      .click();
    await page
      .getByRole("menuitemradio", { name: "Low Priority", exact: true })
      .click();
    await quick
      .getByRole("button", { name: "Quick due date", exact: true })
      .click();
    await page
      .getByRole("dialog", { name: "Task date and reminders", exact: true })
      .getByRole("button", { name: "Clear", exact: true })
      .click();
    await quick
      .getByRole("textbox", { name: "Quick task title", exact: true })
      .evaluate((element, text) => {
        const clipboardData = new DataTransfer();
        clipboardData.setData("text/plain", text);
        element.dispatchEvent(
          new ClipboardEvent("paste", {
            bubbles: true,
            cancelable: true,
            clipboardData,
          }),
        );
      }, `${prefix} review tomorrow at 9am !1\n${prefix} recurring every monday at 8am !1`);
    const batch = page.getByRole("dialog", {
      name: "Add Multiple Tasks",
      exact: true,
    });
    await expect(batch).toBeVisible();
    await expect(
      batch.getByLabel("Preview task 1", { exact: true }),
    ).toContainText("Low priority");
    await batch
      .getByRole("button", { name: "Add 2 Tasks", exact: true })
      .click();
    await expect(batch).toBeHidden();
    const created = (await snapshot(page)).tasks.filter(
      (task) => task.title.startsWith(prefix) && !task.deleted_at,
    );
    expect(created).toHaveLength(2);
    for (const task of created)
      expect(task).toMatchObject({
        priority: 1,
        due_date: null,
        due_time: null,
        recurrence: null,
        reminders: [],
      });
  } finally {
    for (const task of (await snapshot(page)).tasks.filter(
      (task) => task.title.startsWith(prefix) && !task.deleted_at,
    ))
      await call(page.request, "tasks.delete", { id: task.id });
  }
});
