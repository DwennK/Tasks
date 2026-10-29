import { marked } from "marked";

const blockTags = new Set([
  "ADDRESS",
  "ARTICLE",
  "ASIDE",
  "BLOCKQUOTE",
  "BR",
  "DD",
  "DIV",
  "DL",
  "DT",
  "FIGCAPTION",
  "FIGURE",
  "FOOTER",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "HEADER",
  "HR",
  "LI",
  "MAIN",
  "NAV",
  "OL",
  "P",
  "PRE",
  "SECTION",
  "TABLE",
  "TD",
  "TH",
  "TR",
  "UL",
]);

/** Search the visible note, preserving inline words and literal plain text. */
export function noteText(notes: string, format: string): string {
  if (format === "plain" || !format) return notes.replace(/\s+/g, " ").trim();
  const html =
    format === "markdown" ? marked.parse(notes, { async: false }) : notes;
  // Template content stays inert: links, images and scripts are never mounted.
  const template = document.createElement("template");
  template.innerHTML = html;
  const read = (node: Node): string => {
    if (node.nodeType === Node.TEXT_NODE) return node.nodeValue || "";
    if (
      node.nodeType !== Node.ELEMENT_NODE &&
      node.nodeType !== Node.DOCUMENT_FRAGMENT_NODE
    )
      return "";
    const element = node as Element;
    if (["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE"].includes(element.tagName))
      return "";
    if (element.tagName === "IMG") return element.getAttribute("alt") || "";
    const text = Array.from(node.childNodes, read).join("");
    return blockTags.has(element.tagName) ? ` ${text} ` : text;
  };
  return read(template.content).replace(/\s+/g, " ").trim();
}
