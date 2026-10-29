import TurndownService from "turndown";
import { marked } from "marked";
import type { Task } from "../types";

export type ExportFormat = "markdown" | "text";
type NotesFormat = Task["notes_format"];

function checkboxForItem(node: HTMLElement): Element | undefined {
  return Array.from(node.children)
    .map((child) =>
      child.matches('input[type="checkbox"]')
        ? child
        : child.nodeName === "LABEL"
          ? child.querySelector('input[type="checkbox"]')
          : null,
    )
    .find((child): child is Element => child !== null);
}

function converter(format: ExportFormat): TurndownService {
  const plain = format === "text";
  const service = new TurndownService({
    headingStyle: "atx",
    bulletListMarker: "-",
    codeBlockStyle: "fenced",
    emDelimiter: "*",
    hr: "---",
    preformattedCode: true,
  });
  const escape = service.escape.bind(service);
  service.escape = (text) =>
    escape(text).replace(/</g, "&lt;").replace(/>/g, "&gt;");
  service.addRule("unsupportedContent", {
    filter: (node) =>
      ["SCRIPT", "STYLE", "IFRAME", "OBJECT", "EMBED", "SVG", "IMG"].includes(
        node.nodeName,
      ),
    replacement: () => "",
  });
  service.addRule("taskCheckboxLabel", {
    filter: (node) =>
      node.nodeName === "LABEL" &&
      !!node.querySelector('input[type="checkbox"]'),
    replacement: () => "",
  });
  service.addRule("taskCheckbox", {
    filter: (node) => node.nodeName === "INPUT",
    replacement: () => "",
  });
  service.addRule("taskItem", {
    filter: (node) =>
      node.nodeName === "LI" &&
      (node.getAttribute("data-type") === "taskItem" ||
        node.hasAttribute("data-checked") ||
        !!checkboxForItem(node)),
    replacement: (content, node) => {
      const checked =
        node.getAttribute("data-checked") === "true" ||
        checkboxForItem(node)?.hasAttribute("checked");
      const body = content
        .replace(/^\s+/, "")
        .replace(/\n+$/, "")
        .replace(/\n/g, "\n    ");
      return `-   [${checked ? "x" : " "}] ${body}\n`;
    },
  });
  service.addRule("strike", {
    filter: (node) => ["DEL", "S", "STRIKE"].includes(node.nodeName),
    replacement: (content) => (plain ? content : `~~${content}~~`),
  });
  service.addRule("underline", {
    filter: "u",
    replacement: (content) => (plain ? content : `<u>${content}</u>`),
  });
  service.addRule("highlight", {
    filter: "mark",
    replacement: (content) => (plain ? content : `<mark>${content}</mark>`),
  });
  service.addRule("safeLink", {
    filter: "a",
    replacement: (content, node) => {
      const href = node.getAttribute("href") || "";
      if (!/^(?:https?:|mailto:|tasks:|#)/i.test(href)) return content;
      if (plain) return content === href ? content : `${content} (${href})`;
      return `[${content}](${href.replace(/\(/g, "%28").replace(/\)/g, "%29")})`;
    },
  });
  if (plain) {
    service.escape = (text) => text;
    service.addRule("plainInline", {
      filter: ["strong", "b", "em", "i", "code"],
      replacement: (content, node) =>
        node.nodeName === "CODE" ? node.textContent || "" : content,
    });
    service.addRule("plainHeading", {
      filter: ["h1", "h2", "h3", "h4", "h5", "h6", "blockquote"],
      replacement: (content) => `\n\n${content}\n\n`,
    });
    service.addRule("plainCodeBlock", {
      filter: "pre",
      replacement: (_content, node) => `\n\n${node.textContent || ""}\n\n`,
    });
  }
  return service;
}

export function serializeNotes(
  notes: string,
  notesFormat: NotesFormat,
  format: ExportFormat,
): string {
  if (!notes) return "";
  if (notesFormat === "plain")
    return format === "text" ? notes : converter(format).escape(notes);
  const html =
    notesFormat === "markdown"
      ? marked.parse(notes, { async: false, gfm: true, breaks: true })
      : notes;
  return converter(format).turndown(html);
}

/** Export the selected task's content, including its existing child hierarchy. */
export function serializeTask(
  task: Task,
  tasks: Task[] = [],
  format: ExportFormat = "markdown",
): string {
  const service = converter(format);
  const title = (value: string) =>
    format === "markdown" ? service.escape(value) : value;
  const visited = new Set<string>();
  const children = new Map<string, Task[]>();
  for (const child of tasks) {
    if (!child.parent_id || child.deleted_at) continue;
    const siblings = children.get(child.parent_id) || [];
    siblings.push(child);
    children.set(child.parent_id, siblings);
  }
  for (const siblings of children.values())
    siblings.sort((a, b) => a.position - b.position);

  const body = (current: Task): string => {
    visited.add(current.id);
    const parts: string[] = [];
    const notes = serializeNotes(current.notes, current.notes_format, format);
    if (notes) parts.push(notes);
    if (current.checklist?.length)
      parts.push(
        current.checklist
          .map(
            (item) => `- [${item.completed ? "x" : " "}] ${title(item.text)}`,
          )
          .join("\n"),
      );
    const childLines: string[] = [];
    for (const child of children.get(current.id) || []) {
      if (visited.has(child.id)) continue;
      const content = body(child);
      childLines.push(
        `${child.is_note ? "- " : `- [${child.completed_at ? "x" : " "}] `}${title(child.title)}` +
          (content ? `\n\n${content.replace(/^/gm, "    ")}` : ""),
      );
    }
    if (childLines.length) parts.push(childLines.join("\n"));
    return parts.join("\n\n");
  };
  const content = body(task);
  const heading = `${format === "markdown" ? "# " : ""}${title(task.title)}`;
  return `${heading}${content ? `\n\n${content}` : ""}\n`;
}
