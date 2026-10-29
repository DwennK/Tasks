<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import { EditorContent, useEditor } from "@tiptap/vue-3";
import StarterKit from "@tiptap/starter-kit";
import TaskList from "@tiptap/extension-task-list";
import TaskItem from "@tiptap/extension-task-item";
import Highlight from "@tiptap/extension-highlight";
import Placeholder from "@tiptap/extension-placeholder";
import {
  DOMParser as ProseMirrorDOMParser,
  DOMSerializer,
} from "@tiptap/pm/model";
import {
  markdownClipboardHTML,
  normalizeNoteHTML,
  noteContentHTML,
} from "../lib/note-content";
import {
  PhArrowsOutSimple,
  PhArrowCounterClockwise,
  PhArrowClockwise,
  PhCheckSquare,
  PhCode,
  PhHighlighter,
  PhLink,
  PhLinkSimple,
  PhListBullets,
  PhListNumbers,
  PhMinus,
  PhPaperclip,
  PhQuotes,
  PhTag,
  PhTextB,
  PhTextH,
  PhTextHOne,
  PhTextHTwo,
  PhTextHThree,
  PhTextItalic,
  PhTextStrikethrough,
  PhTextUnderline,
  PhTreeStructure,
} from "@phosphor-icons/vue";

const props = withDefaults(
  defineProps<{
    taskId: string;
    modelValue: string;
    format?: "plain" | "markdown" | "html";
    readonly?: boolean;
    toolbar?: boolean;
  }>(),
  { format: "plain", readonly: false, toolbar: false },
);
const emit = defineEmits<{
  "update:modelValue": [value: string];
  "notes-change": [taskId: string, value: string];
  attachment: [];
  files: [files: File[]];
  subtask: [];
  tag: [];
  "link-task": [];
  "open-url": [url: string];
  "focus-mode": [];
}>();
const container = ref<HTMLElement>();
const slash = ref<{
  from: number;
  to: number;
  query: string;
  left: number;
  top: number;
  height: number;
} | null>(null);
const toolbarLeft = ref("50%");
let resizeObserver: ResizeObserver | undefined;
const slashIndex = ref(0);
const linkOpen = ref(false);
const linkValue = ref("");
const linkError = ref("");
const headingOpen = ref(false);
let lastEmitted = "";
let settingContent = false;

function contentHTML(value: string, format = props.format): string {
  return noteContentHTML(value, format);
}
const commands = [
  { id: "h1", name: "Heading 1", icon: PhTextHOne },
  { id: "h2", name: "Heading 2", icon: PhTextHTwo },
  { id: "h3", name: "Heading 3", icon: PhTextHThree },
  { id: "bullet", name: "Bulleted List", icon: PhListBullets },
  { id: "ordered", name: "Numbered List", icon: PhListNumbers },
  { id: "check", name: "Check Item", icon: PhCheckSquare },
  { id: "quote", name: "Quote", icon: PhQuotes },
  { id: "rule", name: "Horizontal Line", icon: PhMinus },
  { id: "attachment", name: "Attachment", icon: PhPaperclip, divider: true },
  { id: "subtask", name: "Subtask", icon: PhTreeStructure },
  { id: "tag", name: "Tag", icon: PhTag },
  { id: "task-link", name: "Linked task/note", icon: PhLink },
];
const filteredCommands = computed(() =>
  commands.filter((command) =>
    command.name.toLowerCase().includes(slash.value?.query.toLowerCase() || ""),
  ),
);

function saveHTML() {
  const current = editor.value;
  if (!current || current.isDestroyed || settingContent || props.readonly)
    return;
  const html = current.isEmpty ? "" : current.getHTML();
  if (html === lastEmitted) return;
  lastEmitted = html;
  emit("update:modelValue", html);
  emit("notes-change", props.taskId, html);
}
function findSlash() {
  const current = editor.value;
  if (
    !current ||
    current.isDestroyed ||
    props.readonly ||
    !current.isFocused ||
    !current.state.selection.empty
  ) {
    slash.value = null;
    return;
  }
  const { $from } = current.state.selection;
  if ($from.parent.type.name === "codeBlock") {
    slash.value = null;
    return;
  }
  const before = $from.parent.textBetween(
    0,
    $from.parentOffset,
    "\n",
    "\ufffc",
  );
  const match = before.match(/(?:^|\s)\/([\w ]*)$/);
  if (!match || match[1].length > 24) {
    slash.value = null;
    return;
  }
  const from = $from.pos - match[1].length - 1;
  const coords = current.view.coordsAtPos(from);
  const left = Math.max(8, Math.min(coords.left, window.innerWidth - 174));
  const availableBelow = window.innerHeight - coords.bottom - 14;
  const above = availableBelow < 230 && coords.top > 260;
  const height = Math.min(430, above ? coords.top - 12 : availableBelow);
  const top = above ? coords.top - height - 3 : coords.bottom + 3;
  if (slash.value?.query !== match[1]) slashIndex.value = 0;
  slash.value = { from, to: $from.pos, query: match[1], left, top, height };
}
// Register reconciliation before useEditor's own destroy hook. Once its view
// is destroyed there is no DOM observer to flush, even though the ref remains.
onBeforeUnmount(() => reconcile());
const editor = useEditor({
  content: contentHTML(props.modelValue),
  editable: !props.readonly,
  // Parse Markdown once, structurally. Regex paste rules would also reinterpret
  // literal content copied as HTML or pasted with Shift+Command+V.
  enablePasteRules: false,
  extensions: [
    StarterKit.configure({
      heading: { levels: [1, 2, 3] },
      link: {
        openOnClick: false,
        autolink: true,
        defaultProtocol: "https",
        HTMLAttributes: { rel: "noopener noreferrer", target: "_blank" },
      },
    }),
    TaskList,
    TaskItem.configure({ nested: true }),
    Highlight,
    Placeholder.configure({
      placeholder: "Write something or type / for commands",
    }),
  ],
  editorProps: {
    attributes: {
      class: "notes-prose",
      role: "textbox",
      "aria-label": "Task notes",
      "aria-multiline": "true",
      spellcheck: "true",
    },
    transformPastedHTML: normalizeNoteHTML,
    clipboardTextParser(text, context, plainText, view) {
      const literal =
        plainText ||
        context.parent.type.spec.code ||
        context.marks().some((mark) => mark.type.name === "code");
      const html = literal ? null : markdownClipboardHTML(text);
      const holder = document.createElement("div");
      if (html) holder.innerHTML = html;
      else {
        // Match ProseMirror's native text parser, including the insertion marks.
        const serializer = DOMSerializer.fromSchema(view.state.schema);
        for (const block of text.split(/(?:\r\n?|\n)+/)) {
          const paragraph = holder.appendChild(document.createElement("p"));
          if (block)
            paragraph.append(
              serializer.serializeNode(
                view.state.schema.text(block, context.marks()),
              ),
            );
        }
      }
      return ProseMirrorDOMParser.fromSchema(view.state.schema).parseSlice(
        holder,
        {
          preserveWhitespace: true,
          context,
        },
      );
    },
    handleKeyDown(_view, event) {
      if (!slash.value) return false;
      if (event.key === "Escape") {
        slash.value = null;
        event.stopPropagation();
        return true;
      }
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        const count = filteredCommands.value.length;
        slashIndex.value = count
          ? (slashIndex.value + (event.key === "ArrowDown" ? 1 : count - 1)) %
            count
          : 0;
        return true;
      }
      if (event.key === "Enter" && filteredCommands.value[slashIndex.value]) {
        execute(filteredCommands.value[slashIndex.value].id);
        return true;
      }
      return false;
    },
    handleDrop(_view, event) {
      const files = Array.from(event.dataTransfer?.files || []);
      if (!files.length || props.readonly) return false;
      emit("files", files);
      event.preventDefault();
      return true;
    },
    handlePaste(_view, event) {
      const files = Array.from(event.clipboardData?.files || []);
      if (!files.length || props.readonly) return false;
      emit("files", files);
      return true;
    },
    handleClick(_view, _position, event) {
      const link = (event.target as Element)?.closest("a");
      if (link && (event.metaKey || event.ctrlKey)) {
        const href = link.getAttribute("href") || "";
        if (/^https?:\/\//i.test(href) || /^mailto:/i.test(href))
          emit("open-url", href);
        return true;
      }
      return false;
    },
  },
  onCreate({ editor: instance }) {
    lastEmitted = instance.isEmpty ? "" : instance.getHTML();
  },
  onUpdate() {
    saveHTML();
    findSlash();
  },
  onSelectionUpdate() {
    findSlash();
  },
  onFocus() {
    nextTick(findSlash);
  },
  onBlur() {
    slash.value = null;
  },
});

watch(
  () => [props.modelValue, props.format] as const,
  ([value]) => {
    if (!editor.value || value === lastEmitted) return;
    const html = contentHTML(value);
    settingContent = true;
    editor.value.commands.setContent(html, { emitUpdate: false });
    lastEmitted = editor.value.isEmpty ? "" : editor.value.getHTML();
    settingContent = false;
  },
);
watch(
  () => props.readonly,
  (value) => editor.value?.setEditable(!value),
);
watch(
  () => props.toolbar,
  (value) => {
    if (value) nextTick(placeToolbar);
    if (!value) {
      headingOpen.value = false;
      linkOpen.value = false;
    }
  },
);

function execute(id: string) {
  const current = editor.value;
  if (!current || current.isDestroyed || props.readonly) return;
  const range = slash.value;
  slash.value = null;
  let chain = current.chain().focus();
  if (range) chain = chain.deleteRange({ from: range.from, to: range.to });
  if (id === "h1" || id === "h2" || id === "h3")
    chain.toggleHeading({ level: Number(id[1]) as 1 | 2 | 3 }).run();
  else if (id === "paragraph") chain.setParagraph().run();
  else if (id === "bullet") chain.toggleBulletList().run();
  else if (id === "ordered") chain.toggleOrderedList().run();
  else if (id === "check") chain.toggleTaskList().run();
  else if (id === "quote") chain.toggleBlockquote().run();
  else if (id === "rule") chain.setHorizontalRule().run();
  else if (id === "bold") chain.toggleBold().run();
  else if (id === "italic") chain.toggleItalic().run();
  else if (id === "underline") chain.toggleUnderline().run();
  else if (id === "strike") chain.toggleStrike().run();
  else if (id === "highlight") chain.toggleHighlight().run();
  else if (id === "code") chain.toggleCodeBlock().run();
  else if (id === "undo") chain.undo().run();
  else if (id === "redo") chain.redo().run();
  else {
    chain.run();
    if (id === "attachment") emit("attachment");
    if (id === "subtask") emit("subtask");
    if (id === "tag") emit("tag");
    if (id === "task-link") emit("link-task");
  }
  headingOpen.value = false;
}
function openLink() {
  slash.value = null;
  linkValue.value = editor.value?.getAttributes("link").href || "";
  linkError.value = "";
  linkOpen.value = !linkOpen.value;
  nextTick(() =>
    container.value
      ?.querySelector<HTMLInputElement>('[aria-label="Link URL"]')
      ?.focus(),
  );
}
function applyLink() {
  const value = linkValue.value.trim();
  if (!value)
    editor.value?.chain().focus().extendMarkRange("link").unsetLink().run();
  else {
    const href = /^(https?:|mailto:)/i.test(value) ? value : `https://${value}`;
    try {
      const parsed = new URL(href);
      if (!["http:", "https:", "mailto:"].includes(parsed.protocol))
        throw new Error("scheme");
      editor.value
        ?.chain()
        .focus()
        .extendMarkRange("link")
        .setLink({ href })
        .run();
    } catch {
      linkError.value = "Enter a valid web or email link.";
      return;
    }
  }
  linkOpen.value = false;
}
function reconcile() {
  const current = editor.value;
  if (!current || current.isDestroyed) return;
  // Flush pending WebKit DOM mutations before the native quit handshake reads
  // the ProseMirror document. This does not replace the document or its history.
  const view = current.view as unknown as { domObserver?: { flush(): void } };
  view.domObserver?.flush();
  saveHTML();
}
function focus() {
  if (editor.value && !editor.value.isDestroyed) editor.value.commands.focus();
}
function closeMenus() {
  slash.value = null;
  linkOpen.value = false;
  headingOpen.value = false;
}
function toggleChecklist() {
  execute("check");
}
defineExpose({ reconcile, focus, closeMenus, toggleChecklist, execute });
function placeToolbar() {
  const bounds = container.value
    ?.closest(".task-editor")
    ?.getBoundingClientRect();
  if (bounds) toolbarLeft.value = `${bounds.left + bounds.width / 2}px`;
}
function scrolled() {
  slash.value = null;
}
onMounted(() => {
  const panel = container.value?.closest(".task-editor");
  if (panel) {
    resizeObserver = new ResizeObserver(placeToolbar);
    resizeObserver.observe(panel);
  }
  document.addEventListener("scroll", scrolled, true);
  placeToolbar();
});
onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  document.removeEventListener("scroll", scrolled, true);
});
</script>

<template>
  <div ref="container" class="rich-notes" @keydown.esc="closeMenus">
    <EditorContent :editor="editor" />
    <Teleport to="body"
      ><div
        v-if="slash && filteredCommands.length"
        class="slash-menu"
        role="menu"
        aria-label="Insert content"
        :style="{
          left: slash.left + 'px',
          top: slash.top + 'px',
          maxHeight: slash.height + 'px',
        }"
        @mousedown.prevent
      >
        <template v-for="(item, index) in filteredCommands" :key="item.id">
          <div v-if="item.divider && index" class="slash-divider"></div>
          <button
            role="menuitem"
            :class="{ active: slashIndex === index }"
            @mouseenter="slashIndex = index"
            @click="execute(item.id)"
          >
            <component :is="item.icon" :size="17" />{{ item.name }}
          </button>
        </template>
      </div></Teleport
    >
    <div
      v-if="toolbar && !readonly && editor"
      class="rich-toolbar"
      role="toolbar"
      aria-label="Text formatting"
      :style="{ left: toolbarLeft }"
      @mousedown.prevent
    >
      <button
        aria-label="Focus mode"
        title="Focus mode"
        @click="emit('focus-mode')"
      >
        <PhArrowsOutSimple :size="17" />
      </button>
      <span class="tool-separator"></span>
      <div class="tool-anchor">
        <button
          aria-label="Heading"
          title="Heading"
          :class="{ active: editor.isActive('heading') }"
          @click="headingOpen = !headingOpen"
        >
          <PhTextH :size="18" />
        </button>
        <div v-if="headingOpen" class="heading-menu">
          <button @click="execute('paragraph')">Text</button
          ><button @click="execute('h1')">Heading 1</button
          ><button @click="execute('h2')">Heading 2</button
          ><button @click="execute('h3')">Heading 3</button>
        </div>
      </div>
      <button
        aria-label="Bold"
        title="Bold · ⌘B"
        :aria-pressed="editor.isActive('bold')"
        @click="execute('bold')"
      >
        <PhTextB :size="18" />
      </button>
      <button
        aria-label="Highlight"
        title="Highlight"
        :aria-pressed="editor.isActive('highlight')"
        @click="execute('highlight')"
      >
        <PhHighlighter :size="18" />
      </button>
      <span class="tool-separator"></span>
      <button
        aria-label="Check Item"
        title="Check Item"
        :aria-pressed="editor.isActive('taskList')"
        @click="execute('check')"
      >
        <PhCheckSquare :size="18" />
      </button>
      <button
        aria-label="Bulleted List"
        title="Bulleted List"
        :aria-pressed="editor.isActive('bulletList')"
        @click="execute('bullet')"
      >
        <PhListBullets :size="18" />
      </button>
      <button
        aria-label="Numbered List"
        title="Numbered List"
        :aria-pressed="editor.isActive('orderedList')"
        @click="execute('ordered')"
      >
        <PhListNumbers :size="18" />
      </button>
      <span class="tool-separator"></span>
      <button
        aria-label="Italic"
        title="Italic · ⌘I"
        :aria-pressed="editor.isActive('italic')"
        @click="execute('italic')"
      >
        <PhTextItalic :size="18" />
      </button>
      <button
        aria-label="Underline"
        title="Underline · ⌘U"
        :aria-pressed="editor.isActive('underline')"
        @click="execute('underline')"
      >
        <PhTextUnderline :size="18" />
      </button>
      <button
        aria-label="Strikethrough"
        title="Strikethrough"
        :aria-pressed="editor.isActive('strike')"
        @click="execute('strike')"
      >
        <PhTextStrikethrough :size="18" />
      </button>
      <button
        aria-label="Horizontal Line"
        title="Horizontal Line"
        @click="execute('rule')"
      >
        <PhMinus :size="18" />
      </button>
      <button
        aria-label="Undo"
        title="Undo · ⌘Z"
        :disabled="!editor.can().undo()"
        @click="execute('undo')"
      >
        <PhArrowCounterClockwise :size="18" />
      </button>
      <button
        aria-label="Redo"
        title="Redo · ⇧⌘Z"
        :disabled="!editor.can().redo()"
        @click="execute('redo')"
      >
        <PhArrowClockwise :size="18" />
      </button>
      <span class="tool-separator"></span>
      <button
        aria-label="Add link"
        title="Link"
        :aria-pressed="editor.isActive('link')"
        @click="openLink"
      >
        <PhLinkSimple :size="18" />
      </button>
      <button
        aria-label="Code block"
        title="Code block"
        :aria-pressed="editor.isActive('codeBlock')"
        @click="execute('code')"
      >
        <PhCode :size="18" />
      </button>
      <button
        aria-label="Quote"
        title="Quote"
        :aria-pressed="editor.isActive('blockquote')"
        @click="execute('quote')"
      >
        <PhQuotes :size="18" />
      </button>
      <span class="tool-separator"></span>
      <button
        aria-label="Attachment"
        title="Attachment"
        @click="emit('attachment')"
      >
        <PhPaperclip :size="18" />
      </button>
    </div>
    <form
      v-if="linkOpen"
      class="inline-link-dialog"
      role="dialog"
      aria-label="Insert link"
      @submit.prevent="applyLink"
      @keydown.esc.stop="linkOpen = false"
    >
      <label
        >Link<input
          v-model="linkValue"
          aria-label="Link URL"
          placeholder="https://"
      /></label>
      <p v-if="linkError" role="alert">{{ linkError }}</p>
      <div>
        <button type="button" @click="linkOpen = false">Cancel</button
        ><button type="submit">OK</button>
      </div>
    </form>
  </div>
</template>

<style scoped>
.rich-notes {
  position: relative;
  min-height: 94px;
}
.rich-notes :deep(.notes-prose) {
  min-height: 94px;
  padding: 0;
  border: 0;
  outline: 0;
  font-size: 14px;
  line-height: 1.6;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}
.rich-notes :deep(.notes-prose p) {
  margin: 0 0 5px;
}
.rich-notes :deep(.notes-prose p.is-editor-empty:first-child::before) {
  content: attr(data-placeholder);
  float: left;
  height: 0;
  color: var(--muted);
  pointer-events: none;
}
.rich-notes :deep(.notes-prose h1) {
  font-size: 26px;
  font-weight: 650;
  margin: 18px 0 8px;
  line-height: 1.3;
}
.rich-notes :deep(.notes-prose h2) {
  font-size: 22px;
  font-weight: 650;
  margin: 15px 0 7px;
  line-height: 1.35;
}
.rich-notes :deep(.notes-prose h3) {
  font-size: 18px;
  font-weight: 650;
  margin: 12px 0 6px;
  line-height: 1.4;
}
.rich-notes :deep(.notes-prose > :first-child) {
  margin-top: 0;
}
.rich-notes :deep(.notes-prose ul),
.rich-notes :deep(.notes-prose ol) {
  padding-left: 23px;
  margin: 4px 0 9px;
}
.rich-notes :deep(.notes-prose ul) {
  list-style: disc;
}
.rich-notes :deep(.notes-prose ol) {
  list-style: decimal;
}
.rich-notes :deep(.notes-prose li p) {
  margin: 0;
}
.rich-notes :deep(.notes-prose li) {
  margin: 3px 0;
}
.rich-notes :deep(.notes-prose ul[data-type="taskList"]) {
  list-style: none;
  padding: 0;
}
.rich-notes :deep(.notes-prose li[data-checked]) {
  display: flex;
  align-items: flex-start;
  gap: 7px;
}
.rich-notes :deep(.notes-prose li[data-checked] > label) {
  flex: 0 0 15px;
  margin-top: 3px;
  line-height: 17px;
  user-select: none;
}
.rich-notes :deep(.notes-prose li[data-checked] > label input) {
  width: 13px;
  height: 13px;
  margin: 1px 0 0;
  cursor: pointer;
  accent-color: var(--accent);
}
.rich-notes :deep(.notes-prose li[data-checked] > div) {
  flex: 1;
  min-width: 0;
}
.rich-notes :deep(.notes-prose li[data-checked="true"] > div > p) {
  color: var(--muted);
  text-decoration: line-through;
}
.rich-notes :deep(.notes-prose blockquote) {
  border-left: 3px solid var(--muted);
  padding-left: 12px;
  margin: 10px 0;
  color: var(--secondary);
}
.rich-notes :deep(.notes-prose hr) {
  border: 0;
  border-top: 1px solid var(--border);
  margin: 16px 0;
}
.rich-notes :deep(.notes-prose pre) {
  white-space: pre-wrap;
  background: var(--input);
  border-radius: 5px;
  padding: 12px;
  margin: 10px 0;
  font:
    12px/1.6 ui-monospace,
    monospace;
}
.rich-notes :deep(.notes-prose code) {
  font-family: ui-monospace, monospace;
  background: var(--input);
  border-radius: 3px;
  padding: 1px 3px;
}
.rich-notes :deep(.notes-prose a) {
  color: var(--accent);
  text-decoration: underline;
  cursor: pointer;
}
.rich-notes :deep(.notes-prose mark) {
  background: #f8e691;
  color: #27272a;
  padding: 0 2px;
}
.slash-menu {
  position: fixed;
  z-index: 95;
  width: 160px;
  padding: 4px;
  border-radius: 12px;
  background: var(--popover);
  box-shadow: 0 5px 15px #0003;
  max-height: 470px;
  overflow-y: auto;
  color: var(--text);
}
.slash-menu button {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 34px;
  padding: 0 10px;
  width: 100%;
  text-align: left;
  border-radius: 7px;
  font-size: 13px;
  white-space: nowrap;
}
.slash-menu button.active,
.slash-menu button:focus-visible {
  background: var(--input);
}
.slash-menu button svg {
  flex: 0 0 17px;
}
.slash-divider {
  height: 1px;
  background: var(--border);
  margin: 3px -4px;
}
.rich-toolbar {
  position: fixed;
  z-index: 65;
  bottom: 59px;
  left: calc(
    (var(--sidebar-width, 212px) + 62px + var(--list-width, 394px) + 100vw) / 2
  );
  transform: translateX(-50%);
  max-width: calc(100vw - 40px);
  display: flex;
  align-items: center;
  gap: 1px;
  padding: 4px 6px;
  background: var(--popover);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: 0 3px 12px #0002;
}
.rich-toolbar > button,
.tool-anchor > button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: 4px;
  flex-shrink: 0;
}
.rich-toolbar button:hover,
.rich-toolbar button[aria-pressed="true"],
.rich-toolbar button.active {
  background: var(--input);
}
.rich-toolbar button:disabled {
  color: var(--muted);
  opacity: 0.45;
}
.tool-separator {
  height: 16px;
  width: 1px;
  background: var(--border);
  margin: 0 3px;
  flex-shrink: 0;
}
.tool-anchor {
  position: relative;
}
.heading-menu {
  position: absolute;
  bottom: 34px;
  left: 0;
  padding: 4px;
  width: 125px;
  background: var(--popover);
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: 0 4px 15px #0002;
}
.heading-menu button {
  display: block;
  width: 100%;
  text-align: left;
  font-size: 13px;
  padding: 7px 8px;
  border-radius: 4px;
}
.inline-link-dialog {
  position: fixed;
  left: 50%;
  top: 40%;
  transform: translate(-50%, -50%);
  width: min(320px, calc(100vw - 32px));
  z-index: 90;
  background: var(--popover);
  border: 1px solid var(--border);
  box-shadow: 0 8px 40px #0003;
  border-radius: 11px;
  padding: 16px;
}
.inline-link-dialog label {
  font-size: 14px;
  display: block;
  font-weight: 600;
}
.inline-link-dialog input {
  display: block;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 5px;
  width: 100%;
  padding: 8px;
  margin-top: 10px;
  font-size: 13px;
  font-weight: 400;
}
.inline-link-dialog > div {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 14px;
}
.inline-link-dialog button {
  padding: 5px 14px;
  border: 1px solid var(--border);
  border-radius: 5px;
  font-size: 13px;
}
.inline-link-dialog button[type="submit"] {
  background: var(--accent);
  color: white;
  border-color: var(--accent);
}
.inline-link-dialog p {
  color: var(--danger);
  font-size: 12px;
}
@media (max-width: 1100px) {
  .rich-toolbar {
    left: 50%;
  }
}
@media (max-width: 650px) {
  .rich-toolbar {
    flex-wrap: wrap;
    width: 330px;
    justify-content: center;
    bottom: 51px;
  }
  .rich-notes :deep(.notes-prose) {
    font-size: 15px;
  }
}
</style>
