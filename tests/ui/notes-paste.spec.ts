import { expect, type Locator, type Page } from "@playwright/test";
import type { Task } from "../../src/types";
import {
  call,
  getTask,
  notes,
  openTask,
  resetSettings,
  test,
  unique,
} from "./helpers";

const pasteShortcut = process.platform === "darwin" ? "Meta+V" : "Control+V";
const copyShortcut = process.platform === "darwin" ? "Meta+C" : "Control+C";
const selectAllShortcut =
  process.platform === "darwin" ? "Meta+A" : "Control+A";
const undoShortcut = process.platform === "darwin" ? "Meta+Z" : "Control+Z";

type ClipboardContent = { plain: string; html?: string };

async function writeClipboard(page: Page, content: ClipboardContent) {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
    origin: new URL(page.url()).origin,
  });
  await page.evaluate(async ({ plain, html }) => {
    if (html === undefined) await navigator.clipboard.writeText(plain);
    else
      await navigator.clipboard.write([
        new ClipboardItem({
          "text/plain": new Blob([plain], { type: "text/plain" }),
          "text/html": new Blob([html], { type: "text/html" }),
        }),
      ]);
  }, content);
}

async function pasteClipboard(
  page: Page,
  editor: Locator,
  content: ClipboardContent,
  plainText = false,
) {
  await writeClipboard(page, content);
  await editor.focus();
  // Observe the browser-generated event to distinguish a real keyboard paste
  // from dispatching a synthetic ClipboardEvent directly to ProseMirror.
  await page.evaluate(() => {
    (window as any).__notesPasteEvent = null;
    document.addEventListener(
      "paste",
      (event) => {
        (window as any).__notesPasteEvent = {
          trusted: event.isTrusted,
          types: Array.from(event.clipboardData?.types || []),
        };
      },
      { capture: true, once: true },
    );
  });
  if (plainText && process.platform === "darwin") {
    // Playwright's macOS editing-command map omits Paste and Match Style.
    // Ask Chrome to run that native command; it still emits a trusted paste.
    const session = await page.context().newCDPSession(page);
    await page.keyboard.down("Shift");
    try {
      await session.send("Input.dispatchKeyEvent", {
        type: "rawKeyDown",
        key: "V",
        code: "KeyV",
        windowsVirtualKeyCode: 86,
        modifiers: 12,
        commands: ["pasteAndMatchStyle"],
      });
      await session.send("Input.dispatchKeyEvent", {
        type: "keyUp",
        key: "V",
        code: "KeyV",
        windowsVirtualKeyCode: 86,
      });
    } finally {
      await page.keyboard.up("Shift");
      await session.detach();
    }
  } else
    await page.keyboard.press(plainText ? "Control+Shift+V" : pasteShortcut);
  await expect
    .poll(() => page.evaluate(() => (window as any).__notesPasteEvent?.trusted))
    .toBe(true);
}

function item(editor: Locator, label: string) {
  const exact = new RegExp(
    `^\\s*${label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*$`,
  );
  return editor
    .locator("li[data-checked] > div > p")
    .filter({ hasText: exact })
    .locator("..")
    .locator("..");
}
const checkbox = (editor: Locator, label: string) =>
  item(editor, label).locator(':scope > label > input[type="checkbox"]');

async function expectChecklist(
  editor: Locator,
  expected: Record<string, boolean>,
) {
  await expect(editor.locator('input[type="checkbox"]')).toHaveCount(
    Object.keys(expected).length,
  );
  for (const [label, checked] of Object.entries(expected)) {
    await expect(item(editor, label)).toHaveAttribute(
      "data-checked",
      String(checked),
    );
    if (checked) await expect(checkbox(editor, label)).toBeChecked();
    else await expect(checkbox(editor, label)).not.toBeChecked();
  }
}

async function expectNotesSaved(page: Page, task: Task, marker: string) {
  await expect
    .poll(async () => (await getTask(page, task.id)).notes)
    .toContain(marker);
  expect((await getTask(page, task.id)).notes_format).toBe("html");
}

const markdown = [
  "## Pasted release notes",
  "",
  "An intro with **bold phrase**, [Documentation](https://example.com/guide), and `inline()`.",
  "",
  "- Ordinary bullet",
  "  - Nested ordinary bullet",
  "- [ ] Open Markdown",
  "  - [x] Nested Markdown done",
  "- [x] Completed Markdown",
  "- Another ordinary bullet",
  "",
  "-[ ] Compact open",
  "-[x] Compact done",
  "☐ Symbol open",
  "☑ Symbol done",
  "",
  "```text",
  "- [ ] literal checkbox in code",
  "**literal bold in code**",
  "```",
].join("\n");
const markdownStates = {
  "Open Markdown": false,
  "Nested Markdown done": true,
  "Completed Markdown": true,
  "Compact open": false,
  "Compact done": true,
  "Symbol open": false,
  "Symbol done": true,
};

for (const viewport of [
  { name: "desktop", width: 1470, height: 923 },
  { name: "mobile", width: 390, height: 844 },
]) {
  test(`real Markdown clipboard paste creates interactive mixed and nested checklists on ${viewport.name}`, async ({
    page,
  }) => {
    await page.setViewportSize({
      width: viewport.width,
      height: viewport.height,
    });
    await resetSettings(page);
    const task = await call<Task>(page.request, "tasks.create", {
      title: unique(`Markdown paste ${viewport.name}`),
    });
    try {
      await page.goto("/");
      await openTask(page, task.title);
      const editor = notes(page);
      await pasteClipboard(page, editor, { plain: markdown });
      await expectChecklist(editor, markdownStates);
      await expect(editor.locator("h2")).toHaveText("Pasted release notes");
      await expect(editor.locator("strong")).toHaveText("bold phrase");
      await expect(
        editor.getByRole("link", { name: "Documentation", exact: true }),
      ).toHaveAttribute("href", "https://example.com/guide");
      await expect(editor.locator("p > code")).toHaveText("inline()");
      await expect(editor.locator("pre > code")).toContainText(
        "- [ ] literal checkbox in code\n**literal bold in code**",
      );
      await expect(editor.locator("li:not([data-checked]) > p")).toHaveText([
        "Ordinary bullet",
        "Nested ordinary bullet",
        "Another ordinary bullet",
      ]);
      await expect(
        item(editor, "Open Markdown").locator('ul[data-type="taskList"]'),
      ).toHaveCount(1);
      await expectNotesSaved(page, task, "literal checkbox in code");
      let before = (await getTask(page, task.id)).notes;
      await checkbox(editor, "Open Markdown").check();
      await expect
        .poll(async () => (await getTask(page, task.id)).notes)
        .not.toBe(before);
      before = (await getTask(page, task.id)).notes;
      await checkbox(editor, "Completed Markdown").uncheck();
      await expect
        .poll(async () => (await getTask(page, task.id)).notes)
        .not.toBe(before);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      await page.screenshot({
        path: `output/playwright/notes-paste-${viewport.name}.png`,
      });
      await page.reload();
      await openTask(page, task.title);
      await expectChecklist(notes(page), {
        ...markdownStates,
        "Open Markdown": true,
        "Completed Markdown": false,
      });
      await expect(notes(page).locator("h2")).toHaveText(
        "Pasted release notes",
      );
      await expect(notes(page).locator("pre > code")).toContainText(
        "- [ ] literal checkbox in code",
      );
    } finally {
      await call(page.request, "tasks.delete", { id: task.id });
    }
  });
}

test("real HTML clipboard paste normalizes external checkbox markup while preserving bullets and formatting", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await resetSettings(page);
  const task = await call<Task>(page.request, "tasks.create", {
    title: unique("HTML checkbox paste"),
  });
  const html = `<h2>From a rich clipboard</h2>
    <ul>
      <li><input type="checkbox" disabled checked>HTML completed</li>
      <li><p><input type="checkbox">HTML open</p><ul><li><input type="checkbox" checked>HTML nested completed</li></ul></li>
      <li>HTML ordinary bullet</li>
    </ul>
    <p><strong>HTML bold</strong> and <a href="https://example.com/rich">HTML link</a>.</p>
    <pre><code>- [ ] HTML code stays literal</code></pre>`;
  try {
    await page.goto("/");
    await openTask(page, task.title);
    const editor = notes(page);
    await pasteClipboard(page, editor, {
      plain:
        "From a rich clipboard\nHTML completed\nHTML open\nHTML nested completed\nHTML ordinary bullet\nHTML bold and HTML link.\n- [ ] HTML code stays literal",
      html,
    });
    expect(
      await page.evaluate(() => (window as any).__notesPasteEvent.types),
    ).toContain("text/html");
    await expectChecklist(editor, {
      "HTML completed": true,
      "HTML open": false,
      "HTML nested completed": true,
    });
    await expect(
      item(editor, "HTML open").locator('ul[data-type="taskList"]'),
    ).toHaveCount(1);
    await expect(editor.locator("li:not([data-checked]) > p")).toHaveText(
      "HTML ordinary bullet",
    );
    await expect(editor.locator("strong")).toHaveText("HTML bold");
    await expect(
      editor.getByRole("link", { name: "HTML link", exact: true }),
    ).toHaveAttribute("href", "https://example.com/rich");
    await expect(editor.locator("pre > code")).toHaveText(
      "- [ ] HTML code stays literal",
    );
    await expectNotesSaved(page, task, "HTML code stays literal");
    const before = (await getTask(page, task.id)).notes;
    await checkbox(editor, "HTML completed").uncheck();
    await expect
      .poll(async () => (await getTask(page, task.id)).notes)
      .not.toBe(before);
    await page.reload();
    await openTask(page, task.title);
    await expectChecklist(notes(page), {
      "HTML completed": false,
      "HTML open": false,
      "HTML nested completed": true,
    });
    await expect(notes(page).locator("strong")).toHaveText("HTML bold");
  } finally {
    await call(page.request, "tasks.delete", { id: task.id });
  }
});

test("rich clipboard paste preserves adjacent lower-level headings and table cell boundaries", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await resetSettings(page);
  const task = await call<Task>(page.request, "tasks.create", {
    title: unique("HTML headings and table paste"),
  });
  const headings = [
    "Fourth level title",
    "Fifth level title",
    "Sixth level title",
  ];
  const tableRows = [
    ["First cell", "Second cell"],
    ["Third cell", "Fourth cell"],
  ];
  const html =
    "<h4>Fourth level title</h4><h5>Fifth level title</h5><h6>Sixth level title</h6>" +
    "<table><tbody><tr><td>First cell</td><td>Second cell</td></tr><tr><td>Third cell</td><td>Fourth cell</td></tr></tbody></table>" +
    "<p>Paragraph after the table</p>";
  const expectStructure = async (editor: Locator) => {
    await expect(editor.locator("h3")).toHaveText(headings);
    await expect(editor.locator("h4, h5, h6")).toHaveCount(0);
    const tableText = editor.locator("pre > code");
    await expect(tableText).toHaveCount(1);
    expect(
      await tableText.evaluate((element) =>
        (element.textContent || "")
          .trimEnd()
          .split("\n")
          .map((row) => row.split("\t")),
      ),
    ).toEqual(tableRows);
    await expect(
      editor
        .locator(":scope > p")
        .filter({ hasText: /^Paragraph after the table$/ }),
    ).toHaveCount(1);
  };
  try {
    await page.goto("/");
    await openTask(page, task.title);
    await pasteClipboard(page, notes(page), {
      plain: [
        ...headings,
        ...tableRows.map((row) => row.join("\t")),
        "Paragraph after the table",
      ].join("\n"),
      html,
    });
    await expectStructure(notes(page));
    await expectNotesSaved(page, task, "Paragraph after the table");
    await page.reload();
    await openTask(page, task.title);
    await expectStructure(notes(page));
  } finally {
    await call(page.request, "tasks.delete", { id: task.id });
  }
});

test("undoing a checklist paste restores the selected text and preserves surrounding rich paragraphs", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await resetSettings(page);
  const task = await call<Task>(page.request, "tasks.create", {
    title: unique("Paste undo surroundings"),
    notes_format: "html",
    notes:
      '<p>Before <strong>protected text</strong></p><p>Replace this selection</p><p>After <a href="https://example.com/keep">protected link</a></p>',
  });
  try {
    await page.goto("/");
    await openTask(page, task.title);
    const editor = notes(page);
    const beforeHTML = await editor.innerHTML();
    await writeClipboard(page, {
      plain: "- [ ] Inserted open\n- [x] Inserted done",
    });
    await editor.focus();
    await editor
      .locator("p")
      .filter({ hasText: /^Replace this selection$/ })
      .evaluate((element) => {
        const range = document.createRange();
        range.selectNodeContents(element);
        const selection = window.getSelection();
        selection?.removeAllRanges();
        selection?.addRange(range);
      });
    await page.keyboard.press(pasteShortcut);
    await expectChecklist(editor, {
      "Inserted open": false,
      "Inserted done": true,
    });
    await expect(editor.locator("strong")).toHaveText("protected text");
    await expect(
      editor.getByRole("link", { name: "protected link", exact: true }),
    ).toHaveAttribute("href", "https://example.com/keep");
    await expect(editor).not.toContainText("Replace this selection");
    await expectNotesSaved(page, task, "Inserted done");
    await editor.focus();
    await page.keyboard.press(undoShortcut);
    await expect(editor.locator('input[type="checkbox"]')).toHaveCount(0);
    await expect(editor).toContainText("Replace this selection");
    expect(await editor.innerHTML()).toBe(beforeHTML);
    await expect
      .poll(async () => (await getTask(page, task.id)).notes)
      .toContain("Replace this selection");
    expect((await getTask(page, task.id)).notes).not.toContain("Inserted done");
    await page.reload();
    await openTask(page, task.title);
    await expect(notes(page).locator("strong")).toHaveText("protected text");
    await expect(
      notes(page).getByRole("link", { name: "protected link", exact: true }),
    ).toHaveAttribute("href", "https://example.com/keep");
    await expect(notes(page)).toContainText("Replace this selection");
  } finally {
    await call(page.request, "tasks.delete", { id: task.id });
  }
});

test("paste into code and paste as plain text keep Markdown literal", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await resetSettings(page);
  const code = await call<Task>(page.request, "tasks.create", {
    title: unique("Literal code paste"),
    notes_format: "html",
    notes: "<pre><code>Existing code\n</code></pre>",
  });
  const plain = await call<Task>(page.request, "tasks.create", {
    title: unique("Literal plain paste"),
  });
  const literal = "-[ ] Literal checkbox\n**Literal bold**";
  try {
    await page.goto("/");
    await openTask(page, code.title);
    const editor = notes(page);
    await editor.focus();
    await editor.locator("pre > code").evaluate((element) => {
      const range = document.createRange();
      range.selectNodeContents(element);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    });
    await page.keyboard.press("ArrowRight");
    await pasteClipboard(page, editor, { plain: literal });
    await expect(editor.locator("pre > code")).toContainText(literal);
    await expect(editor.locator('input[type="checkbox"], strong')).toHaveCount(
      0,
    );
    await expectNotesSaved(page, code, "Literal checkbox");
    await openTask(page, plain.title);
    await pasteClipboard(
      page,
      notes(page),
      {
        plain: literal,
        html: '<ul><li><input type="checkbox">Literal checkbox</li></ul><p><strong>Literal bold</strong></p>',
      },
      true,
    );
    await expect(notes(page)).toContainText("-[ ] Literal checkbox");
    await expect(notes(page)).toContainText("**Literal bold**");
    await expect(
      notes(page).locator('input[type="checkbox"], strong'),
    ).toHaveCount(0);
    await expectNotesSaved(page, plain, "**Literal bold**");
    await page.reload();
    await openTask(page, plain.title);
    await expect(notes(page)).toContainText("-[ ] Literal checkbox");
    await expect(
      notes(page).locator('input[type="checkbox"], strong'),
    ).toHaveCount(0);
    await openTask(page, code.title);
    await expect(notes(page).locator("pre > code")).toContainText(literal);
  } finally {
    await call(page.request, "tasks.delete", { id: code.id });
    await call(page.request, "tasks.delete", { id: plain.id });
  }
});

test("copying a Tasks checklist through the real clipboard preserves nested checkbox states in another task", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1470, height: 923 });
  await resetSettings(page);
  const source = await call<Task>(page.request, "tasks.create", {
    title: unique("Checklist clipboard source"),
    notes_format: "html",
    notes:
      '<ul data-type="taskList"><li data-type="taskItem" data-checked="false"><p>Copied open</p><ul data-type="taskList"><li data-type="taskItem" data-checked="true"><p>Copied nested done</p></li></ul></li><li data-type="taskItem" data-checked="true"><p>Copied done</p></li></ul>',
  });
  const destination = await call<Task>(page.request, "tasks.create", {
    title: unique("Checklist clipboard destination"),
  });
  const states = {
    "Copied open": false,
    "Copied nested done": true,
    "Copied done": true,
  };
  try {
    await page.goto("/");
    await page
      .context()
      .grantPermissions(["clipboard-read", "clipboard-write"], {
        origin: new URL(page.url()).origin,
      });
    await openTask(page, source.title);
    await expectChecklist(notes(page), states);
    await notes(page).focus();
    await page.keyboard.press(selectAllShortcut);
    await page.keyboard.press(copyShortcut);
    const copiedHTML = await page.evaluate(async () => {
      for (const entry of await navigator.clipboard.read())
        if (entry.types.includes("text/html"))
          return (await entry.getType("text/html")).text();
      return "";
    });
    expect(copiedHTML).toContain("data-pm-slice");
    expect(copiedHTML).toContain('data-type="taskList"');
    await openTask(page, destination.title);
    // Blurring canonicalizes the imported fixture to Tiptap's serialized HTML.
    // From here the destination edits must not alter the source at all.
    await expectNotesSaved(page, source, "<label>");
    const sourceBeforePaste = (await getTask(page, source.id)).notes;
    await notes(page).focus();
    await page.keyboard.press(pasteShortcut);
    await expectChecklist(notes(page), states);
    await expect(
      item(notes(page), "Copied open").locator('ul[data-type="taskList"]'),
    ).toHaveCount(1);
    await expectNotesSaved(page, destination, "Copied nested done");
    expect((await getTask(page, source.id)).notes).toBe(sourceBeforePaste);
    await page.reload();
    await openTask(page, destination.title);
    await expectChecklist(notes(page), states);
    const beforeToggle = (await getTask(page, destination.id)).notes;
    await checkbox(notes(page), "Copied nested done").uncheck();
    await expect(checkbox(notes(page), "Copied nested done")).not.toBeChecked();
    await expect
      .poll(async () => (await getTask(page, destination.id)).notes)
      .not.toBe(beforeToggle);
    expect((await getTask(page, source.id)).notes).toBe(sourceBeforePaste);
  } finally {
    await call(page.request, "tasks.delete", { id: source.id });
    await call(page.request, "tasks.delete", { id: destination.id });
  }
});
