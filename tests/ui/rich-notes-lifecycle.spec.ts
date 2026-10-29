import { expect, test } from "@playwright/test";

test("a queued slash-menu focus update is safe after editor unmount", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/__rich_notes_lifecycle__", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: `<!doctype html><html><head><meta charset="utf-8"></head>
        <body><div id="fixture"></div><script type="module">
          import { createApp, h } from '/node_modules/.vite/deps/vue.js';
          import RichNotes from '/src/components/RichNotes.vue';
          window.lifecycleApp = createApp({ render: () => h(RichNotes, {
            taskId: 'lifecycle-fixture', modelValue: '', format: 'plain'
          }) });
          window.lifecycleApp.mount('#fixture');
        </script></body></html>`,
    }),
  );
  await page.goto("/__rich_notes_lifecycle__");
  const notes = page.getByRole("textbox", { name: "Task notes" });
  await notes.fill("/");
  await expect(
    page.getByRole("menu", { name: "Insert content" }),
  ).toBeVisible();
  await notes.blur();
  await page.evaluate(() => {
    document.querySelector<HTMLElement>('[aria-label="Task notes"]')!.focus();
    // Tiptap's onFocus queues a Vue nextTick. Destroy in the same event turn,
    // before that callback can inspect coordinates on the removed view.
    (
      window as unknown as { lifecycleApp: { unmount(): void } }
    ).lifecycleApp.unmount();
  });
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => requestAnimationFrame(() => resolve())),
  );
  await expect(notes).toHaveCount(0);
  expect(errors).toEqual([]);
});
