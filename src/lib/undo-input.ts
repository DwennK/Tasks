// The native menu owns Cmd+Z. Forward it to the focused text editor before
// considering task history; browser key events retain their normal behavior.
export function isTextInput(element: Element | null): element is HTMLElement {
  return (
    element instanceof HTMLElement &&
    (element.matches(
      'textarea,input:not([type="checkbox"]):not([type="radio"]):not([type="range"]):not([type="color"]):not([type="button"]):not([type="submit"]):not([type="reset"]):not([type="file"]):not([type="hidden"])',
    ) ||
      element.isContentEditable)
  );
}

export function undoTextInput(element: Element | null): boolean {
  if (!isTextInput(element)) return false;
  if (element.isContentEditable) {
    // ProseMirror/Tiptap owns its own history, separate from WebKit's history.
    const key = new KeyboardEvent("keydown", {
      key: "z",
      code: "KeyZ",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    if (!element.dispatchEvent(key)) return true;
  }
  document.execCommand("undo");
  return true;
}
