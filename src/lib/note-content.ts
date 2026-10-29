import DOMPurify from "dompurify";
import { Marked, type Token } from "marked";

export function escapeNoteText(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

const markdown = new Marked({
  gfm: true,
  breaks: true,
  renderer: {
    // These nodes are not supported by the editor schema. Keep their content
    // instead of silently dropping images or collapsing table cells together.
    image: ({ href, text }) =>
      `<a href="${escapeNoteText(href)}">${escapeNoteText(text || href)}</a>`,
    table: ({ raw }) => `<pre><code>${escapeNoteText(raw)}</code></pre>`,
    heading({ tokens, depth }) {
      const level = Math.min(depth, 3);
      return `<h${level}>${this.parser.parseInline(tokens)}</h${level}>`;
    },
  },
});

const blockTags = new Set([
  "P",
  "H1",
  "H2",
  "H3",
  "BLOCKQUOTE",
  "PRE",
  "UL",
  "OL",
  "HR",
]);

/** Import external checkboxes into the editor's own interactive task nodes. */
export function normalizeNoteHTML(html: string): string {
  const holder = document.createElement("div");
  holder.innerHTML = DOMPurify.sanitize(html, {
    ALLOWED_TAGS: [
      "p",
      "br",
      "strong",
      "b",
      "em",
      "i",
      "u",
      "s",
      "del",
      "h1",
      "h2",
      "h3",
      "h4",
      "h5",
      "h6",
      "table",
      "thead",
      "tbody",
      "tfoot",
      "tr",
      "th",
      "td",
      "caption",
      "ul",
      "ol",
      "li",
      "blockquote",
      "hr",
      "code",
      "pre",
      "a",
      "mark",
      "span",
      "input",
      "label",
      "div",
    ],
    ALLOWED_ATTR: [
      "href",
      "title",
      "target",
      "rel",
      "data-type",
      "data-checked",
      "data-pm-slice",
      "start",
      "checked",
      "type",
      "class",
    ],
    ALLOW_DATA_ATTR: false,
  });

  for (const heading of holder.querySelectorAll("h4, h5, h6")) {
    const replacement = document.createElement("h3");
    replacement.append(...heading.childNodes);
    heading.replaceWith(replacement);
  }
  // The editor has no table node. Preserve readable row/column boundaries.
  for (const table of Array.from(holder.querySelectorAll("table")).reverse()) {
    const rows = Array.from(table.rows).map((row) =>
      Array.from(row.cells)
        .map((cell) => cell.textContent?.trim() || "")
        .join("\t"),
    );
    if (table.caption?.textContent) rows.unshift(table.caption.textContent);
    const pre = document.createElement("pre");
    const code = pre.appendChild(document.createElement("code"));
    code.textContent = rows.join("\n");
    table.replaceWith(pre);
  }
  for (const element of holder.querySelectorAll("[data-pm-slice]")) {
    const match = /^(\d+) (\d+)(?: -(\d+))? (.*)$/.exec(
      element.getAttribute("data-pm-slice") || "",
    );
    try {
      const context: unknown = match && JSON.parse(match[4]);
      if (!Array.isArray(context) || context.length % 2)
        throw new Error("Invalid clipboard context");
    } catch {
      element.removeAttribute("data-pm-slice");
    }
  }

  // Children first: each parent must own its content div, otherwise Tiptap's
  // taskItem parser can mistake a nested item's div for the parent's content.
  for (const item of Array.from(holder.querySelectorAll("li")).reverse()) {
    if (item.closest("pre, code")) continue;
    const checkbox = Array.from(
      item.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'),
    ).find(
      (input) => input.closest("li") === item && !input.closest("pre, code"),
    );
    if (!checkbox && item.getAttribute("data-type") !== "taskItem") continue;
    const checked = item.hasAttribute("data-checked")
      ? ["", "true"].includes(item.getAttribute("data-checked")!)
      : !!checkbox?.checked;
    item.setAttribute("data-type", "taskItem");
    item.setAttribute("data-checked", String(checked));
    if (checkbox) {
      const label = checkbox.closest("label");
      const next = checkbox.nextSibling;
      if (next?.nodeType === Node.TEXT_NODE)
        next.textContent = next.textContent?.replace(/^\s+/, "") || "";
      checkbox.remove();
      if (label && !label.textContent?.trim()) label.remove();
    }

    const content = document.createElement("div");
    let paragraph: HTMLParagraphElement | undefined;
    const append = (node: Node) => {
      if (node instanceof HTMLElement && node.tagName === "DIV") {
        paragraph = undefined;
        for (const child of Array.from(node.childNodes)) append(child);
        paragraph = undefined;
      } else if (node instanceof HTMLElement && blockTags.has(node.tagName)) {
        paragraph = undefined;
        content.append(node);
      } else if (
        node.nodeType !== Node.TEXT_NODE ||
        node.textContent?.trim() ||
        paragraph
      ) {
        if (!paragraph) {
          paragraph = document.createElement("p");
          content.append(paragraph);
        }
        paragraph.append(node);
      }
    };
    for (const child of Array.from(item.childNodes)) append(child);
    if (content.firstElementChild?.tagName !== "P")
      content.prepend(document.createElement("p"));
    item.replaceChildren(content);
  }

  // Task lists may only contain task items. Split mixed source lists into
  // adjacent runs instead of turning ordinary bullets into extra checkboxes.
  for (const list of Array.from(holder.querySelectorAll("ul, ol")).reverse()) {
    const items = Array.from(list.children).filter(
      (node) => node.tagName === "LI",
    );
    if (!items.some((item) => item.getAttribute("data-type") === "taskItem"))
      continue;
    const runs = document.createDocumentFragment();
    let run: Element | undefined;
    let previousTask = false;
    const start = Number(list.getAttribute("start") || 1);
    for (const [index, item] of items.entries()) {
      const task = item.getAttribute("data-type") === "taskItem";
      if (!run || task !== previousTask) {
        run = document.createElement(task ? "ul" : list.tagName.toLowerCase());
        if (task) run.setAttribute("data-type", "taskList");
        else if (list.tagName === "OL")
          run.setAttribute("start", String(start + index));
        if (!index && list.hasAttribute("data-pm-slice"))
          run.setAttribute(
            "data-pm-slice",
            list.getAttribute("data-pm-slice")!,
          );
        runs.append(run);
        previousTask = task;
      }
      run.append(item);
    }
    list.replaceWith(runs);
  }
  holder.querySelectorAll("input").forEach((input) => input.remove());
  return holder.innerHTML;
}

export function noteContentHTML(
  value: string,
  format: "plain" | "markdown" | "html",
): string {
  if (!value) return "<p></p>";
  if (format === "plain")
    return value
      .split("\n")
      .map((line) => `<p>${escapeNoteText(line) || "<br>"}</p>`)
      .join("");
  return normalizeNoteHTML(
    format === "markdown" ? markdown.parse(value, { async: false }) : value,
  );
}

function clipboardMarkdown(text: string): string {
  let fence: { marker: string; length: number } | undefined;
  return text
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .map((line) => {
      const delimiter = line.match(/^ {0,3}(`{3,}|~{3,})(.*)$/);
      if (delimiter) {
        if (!fence)
          fence = { marker: delimiter[1][0], length: delimiter[1].length };
        else if (
          delimiter[1][0] === fence.marker &&
          delimiter[1].length >= fence.length &&
          !delimiter[2].trim()
        )
          fence = undefined;
        return line;
      }
      if (fence) return line;
      return line
        .replace(/^( {0,3})([-+*])(?=\[[ xX]\](?:\s|$))/, "$1$2 ")
        .replace(
          /^( {0,3})([☐☑☒])(?:\uFE0F)?[ \t]+/,
          (_match, indent: string, glyph: string) =>
            `${indent}- [${glyph === "☐" ? " " : "x"}] `,
        )
        .replace(/^( {0,3})(\[[ xX]\])[ \t]+/, "$1- $2 ");
    })
    .join("\n");
}

const formattedTokens = new Set([
  "heading",
  "list",
  "blockquote",
  "code",
  "hr",
  "table",
  "strong",
  "em",
  "del",
  "codespan",
  "link",
  "image",
]);
function hasFormatting(tokens: Token[]): boolean {
  return tokens.some(
    (token) =>
      formattedTokens.has(token.type) ||
      ("tokens" in token &&
        Array.isArray(token.tokens) &&
        hasFormatting(token.tokens)),
  );
}

/** Return null for ordinary prose so native whitespace/insertion stays intact. */
export function markdownClipboardHTML(text: string): string | null {
  const normalized = clipboardMarkdown(text);
  if (!hasFormatting(markdown.lexer(normalized))) return null;
  return noteContentHTML(normalized, "markdown");
}
