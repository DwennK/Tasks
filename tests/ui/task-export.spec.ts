import { expect, test as unitTest } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { serializeNotes, serializeTask } from "../../src/components/taskExport";
import type { Task } from "../../src/types";
import { test, call, openTask, resetSettings, unique } from "./helpers";

const richContent = `<h1>Résumé 🚀</h1>
<p><strong>Bold</strong> <em>italique</em> <u>souligné</u> <s>ancien</s> <mark>surligné</mark></p>
<ul data-type="taskList">
  <li data-type="taskItem" data-checked="true"><label><input type="checkbox" checked><span></span></label><div><p>Café terminé</p></div></li>
  <li data-type="taskItem" data-checked="false"><label><input type="checkbox"><span></span></label><div><p>Encore à faire</p></div></li>
</ul>
<ol start="3"><li><p>Étape ordonnée</p></li></ol>
<blockquote><p>Une citation</p></blockquote>
<pre><code class="language-ts">const café = &#96;ok&#96;;
</code></pre>
<p><a href="https://example.test/guide?x=1">Référence</a></p><hr>`;

unitTest(
  "task export preserves rich Unicode content and nested completion state",
  () => {
    const markdown = serializeNotes(richContent, "html", "markdown");
    expect(markdown).toContain("# Résumé 🚀");
    expect(markdown).toContain("**Bold** *italique*");
    expect(markdown).toContain("<u>souligné</u>");
    expect(markdown).toContain("~~ancien~~");
    expect(markdown).toContain("<mark>surligné</mark>");
    expect(markdown).toContain("[x] Café terminé");
    expect(markdown).toContain("[ ] Encore à faire");
    expect(markdown).toContain("3.  Étape ordonnée");
    expect(markdown).toContain("> Une citation");
    expect(markdown).toContain("```ts\nconst café");
    expect(markdown).toContain("[Référence](https://example.test/guide?x=1)");
    expect(markdown).toContain("---");

    const plain = serializeNotes(richContent, "html", "text");
    expect(plain).toContain("Résumé 🚀");
    expect(plain).toContain("Bold italique souligné ancien surligné");
    expect(plain).toContain("[x] Café terminé");
    expect(plain).toContain("[ ] Encore à faire");
    expect(plain).toContain("Référence (https://example.test/guide?x=1)");
    expect(plain).toContain("const café");
    expect(plain).not.toMatch(/<(?:h1|p|u|mark)>|\*\*Bold\*\*|```/);

    const markdownChecklist = serializeNotes(
      "- [x] Markdown fait\n- [ ] À faire",
      "markdown",
      "markdown",
    );
    expect(markdownChecklist).toContain("[x] Markdown fait");
    expect(markdownChecklist).toContain("[ ] À faire");

    const parent = {
      id: "parent",
      title: "Parent",
      notes: "",
      notes_format: "plain",
      checklist: [],
    } as unknown as Task;
    const child = {
      ...parent,
      id: "child",
      title: "Enfant",
      parent_id: "parent",
      completed_at: "2026-10-27",
      position: 0,
    } as Task;
    const grandchild = {
      ...child,
      id: "grandchild",
      title: "Petit enfant",
      parent_id: "child",
      completed_at: null,
    } as Task;
    expect(serializeTask(parent, [child, grandchild], "markdown")).toContain(
      "- [x] Enfant\n\n    - [ ] Petit enfant",
    );
    expect(
      serializeNotes(
        '<script>malicious()</script><img src="file:///private/file"><p>Safe</p>',
        "html",
        "markdown",
      ),
    ).toBe("Safe");
    expect(serializeNotes("<button>Text</button>", "plain", "markdown")).toBe(
      "&lt;button&gt;Text&lt;/button&gt;",
    );
  },
);

test("export preview, copy and downloaded UTF-8 files retain the selected format", async ({
  page,
  context,
}) => {
  await resetSettings(page);
  const task = await call<Task>(page.request, "tasks.create", {
    title: unique("Export résumé 🚀"),
    notes: richContent,
    notes_format: "html",
  });
  await page.goto("/");
  await openTask(page, task.title);
  await page.getByRole("button", { name: "Task actions", exact: true }).click();
  await page.getByRole("menuitem", { name: "Export", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Export", exact: true });
  // Inactive native WebKit windows may pause an opacity animation before
  // its first frame. The export panel must not depend on animation progress.
  expect(
    await dialog.evaluate((element) => {
      const panel = getComputedStyle(element);
      const backdrop = getComputedStyle(element.parentElement!);
      return {
        opacity: panel.opacity,
        animation: panel.animationName,
        backdrop: backdrop.backgroundColor,
        blur: backdrop.backdropFilter,
      };
    }),
  ).toEqual({
    opacity: "1",
    animation: "none",
    backdrop: "rgba(0, 0, 0, 0)",
    blur: "none",
  });
  const preview = dialog.getByRole("textbox", { name: "Export preview" });
  const markdown = await preview.inputValue();
  expect(markdown).toContain(`# ${task.title}`);
  expect(markdown).toContain("# Résumé 🚀");
  expect(markdown).toContain("[x] Café terminé");
  expect(markdown).toContain("```ts");
  const markdownPromise = page.waitForEvent("download");
  await dialog.getByRole("button", { name: "Download", exact: true }).click();
  const markdownFile = await markdownPromise;
  expect(markdownFile.suggestedFilename()).toMatch(/\.md$/);
  expect(await readFile((await markdownFile.path())!, "utf8")).toBe(markdown);

  await dialog.getByRole("tab", { name: "Plain Text", exact: true }).click();
  const plain = await preview.inputValue();
  expect(plain).toContain("Référence (https://example.test/guide?x=1)");
  expect(plain).not.toContain("# Résumé");
  const plainPromise = page.waitForEvent("download");
  await dialog.getByRole("button", { name: "Download", exact: true }).click();
  const plainFile = await plainPromise;
  expect(plainFile.suggestedFilename()).toMatch(/\.txt$/);
  expect(await readFile((await plainFile.path())!, "utf8")).toBe(plain);

  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await dialog.getByRole("button", { name: "Copy", exact: true }).click();
  await expect(dialog.getByRole("status")).toHaveText("Copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(plain);
  await page.screenshot({ path: "output/playwright/task-export.png" });
  await dialog.getByRole("button", { name: "Close dialog" }).click();
  await expect(dialog).toBeHidden();
});
