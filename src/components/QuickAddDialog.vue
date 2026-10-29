<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import {
  PhCalendarDots,
  PhCheck,
  PhList,
  PhNote,
  PhPlus,
} from "@phosphor-icons/vue";
import TaskDatePicker from "./TaskDatePicker.vue";
import { localDate as currentLocalDate } from "../composables/useTasks";
import { registerEscapeLayer } from "../lib/escape-layers";
import { splitTaskLines } from "../lib/task-input";
import type { Task, TaskList } from "../types";

const props = defineProps<{
  title: string;
  datePatch: Record<string, unknown>;
  priority: number;
  listId: string;
  lists: TaskList[];
  settings: Record<string, unknown>;
  busy?: boolean;
}>();
const emit = defineEmits<{
  "update:title": [value: string];
  "update:datePatch": [value: Record<string, unknown>];
  "update:priority": [value: number];
  "update:listId": [value: string];
  submit: [];
  close: [];
  "batch-paste": [text: string];
}>();

type Popup = "date" | "priority" | "list";
const panel = ref<HTMLElement>();
const titleInput = ref<HTMLInputElement>();
const popupRoot = ref<HTMLElement>();
const dateButton = ref<HTMLButtonElement>();
const priorityButton = ref<HTMLButtonElement>();
const listButton = ref<HTMLButtonElement>();
const popup = ref<Popup | null>(null);
const popupPosition = ref({ left: "12px", top: "12px" });
const composing = ref(false);
let previousFocus: HTMLElement | null = null;
let popupObserver: ResizeObserver | undefined;
let releaseEscapeLayer: (() => void) | undefined;
const priorities = [
  { value: 3, label: "High Priority" },
  { value: 2, label: "Medium Priority" },
  { value: 1, label: "Low Priority" },
  { value: 0, label: "No Priority" },
];
const availableLists = computed(() =>
  props.lists
    .filter((list) => !list.archived_at)
    .slice()
    .sort(
      (a, b) =>
        Number(b.id === "inbox") - Number(a.id === "inbox") ||
        Number(b.pinned) - Number(a.pinned) ||
        a.position - b.position ||
        a.name.localeCompare(b.name),
    ),
);
const selectedList = computed(() =>
  props.lists.find((list) => list.id === props.listId),
);
const listName = computed(() => selectedList.value?.name || "Inbox");
const dueDate = computed(() =>
  typeof props.datePatch.due_date === "string" ? props.datePatch.due_date : "",
);
function localDate(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
const dateText = computed(() => {
  if (!dueDate.value) return "No Date";
  const today = new Date();
  if (dueDate.value === currentLocalDate()) return "Today";
  today.setDate(today.getDate() + 1);
  if (dueDate.value === localDate(today)) return "Tomorrow";
  const date = new Date(`${dueDate.value}T12:00:00`);
  return Number.isNaN(date.getTime())
    ? dueDate.value
    : date.toLocaleDateString("en", {
        month: "short",
        day: "numeric",
        ...(date.getFullYear() !== today.getFullYear()
          ? { year: "numeric" }
          : {}),
      });
});
const dateLabel = computed(() => {
  const time = props.datePatch.due_time;
  if (!dueDate.value || typeof time !== "string" || !time)
    return dateText.value;
  if (String(props.settings.time_format) !== "12")
    return `${dateText.value} ${time}`;
  const [hours, minutes] = time.split(":").map(Number);
  return `${dateText.value} ${hours % 12 || 12}:${String(minutes).padStart(2, "0")} ${hours < 12 ? "AM" : "PM"}`;
});
// The picker reads only date fields; each nullable field starts with the same
// safe defaults as a new task before the parent's derived metadata is applied.
const pickerTask = computed<Task>(() => ({
  id: "quick-add-draft",
  is_note: false,
  title: props.title,
  notes: "",
  notes_text: "",
  notes_format: "plain",
  list_id: props.listId,
  parent_id: null,
  due_date: null,
  due_time: null,
  start_date: null,
  start_time: null,
  duration_minutes: null,
  timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",
  reminder_at: null,
  reminders: [],
  recurrence: null,
  recurrence_next_id: null,
  tag_ids: [],
  linked_task_ids: [],
  attachments: [],
  checklist: [],
  comments: [],
  pinned: false,
  wont_do_at: null,
  section_id: null,
  priority: props.priority,
  position: 0,
  completed_at: null,
  deleted_at: null,
  created_at: "",
  updated_at: "",
  version: 0,
  progress: 0,
  progress_mode: "manual",
  ...props.datePatch,
}));
function anchor(kind = popup.value) {
  return kind === "date"
    ? dateButton.value
    : kind === "priority"
      ? priorityButton.value
      : listButton.value;
}
function positionPopup() {
  const trigger = anchor();
  const content = popupRoot.value;
  if (!popup.value || !trigger || !content) return;
  const bounds = trigger.getBoundingClientRect();
  const width = content.offsetWidth;
  const height = content.offsetHeight;
  const below = bounds.bottom + 7;
  const above = bounds.top - height - 7;
  const top =
    below + height <= window.innerHeight - 12 || above < 12
      ? Math.min(below, window.innerHeight - height - 12)
      : above;
  popupPosition.value = {
    left: `${Math.max(12, Math.min(bounds.left, window.innerWidth - width - 12))}px`,
    top: `${Math.max(12, top)}px`,
  };
}
function closePopup(restoreFocus = true) {
  const trigger = anchor();
  popup.value = null;
  popupObserver?.disconnect();
  if (restoreFocus) nextTick(() => trigger?.focus());
}
async function togglePopup(kind: Popup) {
  if (popup.value === kind) {
    closePopup();
    return;
  }
  popupObserver?.disconnect();
  popup.value = kind;
  await nextTick();
  positionPopup();
  if (popupRoot.value) {
    popupObserver = new ResizeObserver(positionPopup);
    popupObserver.observe(popupRoot.value);
  }
  const selected = popupRoot.value?.querySelector<HTMLElement>(
    '[aria-checked="true"], [aria-selected="true"][role="tab"]',
  );
  (selected || focusTargets(popupRoot.value)[0])?.focus();
}
function saveDate(patch: Record<string, unknown>) {
  emit("update:datePatch", patch);
  closePopup();
}
function selectPriority(value: number) {
  emit("update:priority", value);
  closePopup();
}
function selectList(value: string) {
  emit("update:listId", value);
  closePopup();
}
function submit() {
  if (!props.busy && props.title.trim() && !composing.value) emit("submit");
}
function titleKeydown(event: KeyboardEvent) {
  if (event.key !== "Enter") return;
  if (event.isComposing || event.keyCode === 229 || composing.value) return;
  event.preventDefault();
  submit();
}
function titlePaste(event: ClipboardEvent) {
  if (props.busy) return;
  const text = event.clipboardData?.getData("text/plain") || "";
  if (splitTaskLines(text).length < 2) return;
  event.preventDefault();
  emit("batch-paste", text);
}
function focusTargets(container?: HTMLElement) {
  return Array.from(
    container?.querySelectorAll<HTMLElement>(
      'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex="0"]',
    ) || [],
  ).filter(
    (element) => element.tabIndex >= 0 && element.getClientRects().length,
  );
}
function keydown(event: KeyboardEvent) {
  event.stopPropagation();
  if (event.isComposing || composing.value || event.keyCode === 229) return;
  if (event.key === "Escape") {
    event.preventDefault();
    if (popup.value) closePopup();
    else emit("close");
    return;
  }
  if (
    popup.value &&
    popup.value !== "date" &&
    ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)
  ) {
    event.preventDefault();
    const items = focusTargets(popupRoot.value);
    const index = items.indexOf(document.activeElement as HTMLElement);
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : (index + (event.key === "ArrowUp" ? -1 : 1) + items.length) %
            items.length;
    items[next]?.focus();
    return;
  }
  if (event.key !== "Tab") return;
  const items = [
    ...focusTargets(panel.value),
    ...focusTargets(popupRoot.value),
  ];
  const index = items.indexOf(document.activeElement as HTMLElement);
  if (index < 0 || (event.shiftKey && index === 0)) {
    event.preventDefault();
    (event.shiftKey ? items.at(-1) : items[0])?.focus();
  } else if (!event.shiftKey && index === items.length - 1) {
    event.preventDefault();
    items[0]?.focus();
  }
}
function outsidePopup(event: PointerEvent) {
  const target = event.target as Node;
  if (
    popup.value &&
    !popupRoot.value?.contains(target) &&
    !anchor()?.contains(target)
  )
    closePopup(false);
}
function escapeFromOutside(event: KeyboardEvent) {
  // A mounted date picker owns Escape, including its nested menus, even when
  // WebKit leaves focus on the title input after clicking a date control.
  if (popup.value === "date" || composing.value) return;
  const target = event.target;
  if (
    target instanceof Node &&
    (panel.value?.contains(target) || popupRoot.value?.contains(target))
  )
    return;
  event.preventDefault();
  event.stopImmediatePropagation();
  if (popup.value) closePopup();
  else emit("close");
}
onMounted(async () => {
  previousFocus = document.activeElement as HTMLElement;
  releaseEscapeLayer = registerEscapeLayer(escapeFromOutside);
  document.addEventListener("pointerdown", outsidePopup, true);
  window.addEventListener("resize", positionPopup);
  await nextTick();
  titleInput.value?.focus();
});
onBeforeUnmount(() => {
  releaseEscapeLayer?.();
  popupObserver?.disconnect();
  document.removeEventListener("pointerdown", outsidePopup, true);
  window.removeEventListener("resize", positionPopup);
  if (previousFocus?.isConnected) previousFocus.focus();
});
</script>

<template>
  <Teleport to="body">
    <div class="quick-add-backdrop" @pointerdown.self="emit('close')">
      <section
        ref="panel"
        class="quick-add-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Add Task"
        tabindex="-1"
        @keydown="keydown"
      >
        <div class="quick-title-field">
          <span v-if="!title" class="quick-title-placeholder" aria-hidden="true"
            ><PhPlus :size="16" /><span
              >Add task to "{{ listName }}"</span
            ></span
          >
          <input
            ref="titleInput"
            :value="title"
            :placeholder="`Add task to &quot;${listName}&quot;`"
            aria-label="Quick task title"
            autocomplete="off"
            autofocus
            @input="
              emit('update:title', ($event.target as HTMLInputElement).value)
            "
            @keydown="titleKeydown"
            @paste="titlePaste"
            @compositionstart="composing = true"
            @compositionend="composing = false"
          />
        </div>
        <div class="quick-add-footer">
          <button
            ref="dateButton"
            type="button"
            class="quick-metadata quick-date"
            :class="{ dated: dueDate }"
            aria-label="Quick due date"
            aria-haspopup="dialog"
            :aria-expanded="popup === 'date'"
            :title="dueDate || 'No Date'"
            @click="togglePopup('date')"
          >
            <PhCalendarDots :size="16" /><span>{{ dateLabel }}</span>
          </button>
          <button
            ref="priorityButton"
            type="button"
            class="quick-metadata quick-priority"
            :class="`quick-priority-${priority}`"
            aria-label="Quick priority"
            aria-haspopup="menu"
            :aria-expanded="popup === 'priority'"
            :title="priorities.find((item) => item.value === priority)?.label"
            @click="togglePopup('priority')"
          >
            <svg
              width="17"
              height="17"
              viewBox="0 0 20 20"
              fill="none"
              stroke="currentColor"
              stroke-width="1.4"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
              focusable="false"
            >
              <path
                d="M3 2v15M3 3h13l-3 4.5 3 4.5H3"
                :fill="priority ? 'currentColor' : 'none'"
              />
            </svg>
          </button>
          <button
            ref="listButton"
            type="button"
            class="quick-metadata quick-list"
            aria-label="Quick list"
            aria-haspopup="menu"
            :aria-expanded="popup === 'list'"
            :title="listName"
            @click="togglePopup('list')"
          >
            <span v-if="selectedList?.icon" class="quick-list-emoji">{{
              selectedList.icon
            }}</span>
            <svg
              v-else-if="listId === 'inbox'"
              width="17"
              height="17"
              viewBox="0 0 20 20"
              fill="none"
              stroke="currentColor"
              stroke-width="1.4"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
              focusable="false"
            >
              <path
                d="M6 3.5h8c.6 0 1.1.3 1.4.9L18 9.5v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4l1.6-5.1c.3-.6.8-.9 1.4-.9Z"
              />
              <path d="M3 9.5h4l1.5 3h4l1.5-3h4" />
            </svg>
            <component
              :is="selectedList?.list_type === 'note' ? PhNote : PhList"
              v-else
              :size="17"
            />
            <span>{{ listName }}</span>
          </button>
          <button
            type="button"
            class="quick-add-submit"
            aria-label="Add Task"
            :disabled="busy || !title.trim()"
            @click="submit"
          >
            Add
          </button>
        </div>
      </section>
    </div>
    <div
      v-if="popup"
      ref="popupRoot"
      class="quick-add-popup"
      :style="popupPosition"
      @keydown="keydown"
    >
      <TaskDatePicker
        v-if="popup === 'date'"
        :task="pickerTask"
        :settings="settings"
        :new-task="true"
        @save="saveDate"
        @close="closePopup()"
      />
      <div
        v-else-if="popup === 'priority'"
        class="quick-choice-menu"
        role="menu"
        aria-label="Quick task priority"
      >
        <button
          v-for="item in priorities"
          :key="item.value"
          type="button"
          role="menuitemradio"
          :aria-checked="priority === item.value"
          @click="selectPriority(item.value)"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            stroke-width="1.4"
            stroke-linecap="round"
            stroke-linejoin="round"
            :class="`quick-priority-${item.value}`"
            aria-hidden="true"
            focusable="false"
          >
            <path
              d="M3 2v15M3 3h13l-3 4.5 3 4.5H3"
              :fill="item.value ? 'currentColor' : 'none'"
            />
          </svg>
          <span>{{ item.label }}</span>
          <PhCheck
            v-if="priority === item.value"
            :size="14"
            class="quick-menu-check"
          />
        </button>
      </div>
      <div
        v-else
        class="quick-choice-menu quick-list-menu"
        role="menu"
        aria-label="Quick task list"
      >
        <button
          v-for="list in availableLists"
          :key="list.id"
          type="button"
          role="menuitemradio"
          :aria-checked="listId === list.id"
          @click="selectList(list.id)"
        >
          <span v-if="list.icon" class="quick-list-emoji">{{ list.icon }}</span>
          <svg
            v-else-if="list.id === 'inbox'"
            width="16"
            height="16"
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            stroke-width="1.4"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            focusable="false"
          >
            <path
              d="M6 3.5h8c.6 0 1.1.3 1.4.9L18 9.5v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4l1.6-5.1c.3-.6.8-.9 1.4-.9Z"
            />
            <path d="M3 9.5h4l1.5 3h4l1.5-3h4" />
          </svg>
          <component
            v-else
            :is="list.list_type === 'note' ? PhNote : PhList"
            :size="16"
            :style="
              settings.show_list_color === false
                ? undefined
                : { color: list.color }
            "
          />
          <span>{{ list.name }}</span>
          <PhCheck
            v-if="listId === list.id"
            :size="14"
            class="quick-menu-check"
          />
        </button>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.quick-add-backdrop {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 20vh;
  background: transparent;
  -webkit-app-region: no-drag;
}
.quick-add-dialog {
  --quick-accent: var(--control-accent, #394576);
  width: min(600px, calc(100vw - 24px));
  height: 114px;
  padding: 16px 20px;
  border-radius: 13px;
  background: var(--popover, #fff);
  color: var(--text, #242424);
  box-shadow: 0 20px 70px #0004;
  opacity: 1;
}
.quick-title-field {
  position: relative;
  height: 38px;
}
.quick-title-placeholder {
  position: absolute;
  left: 11px;
  right: 12px;
  top: 0;
  height: 38px;
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  color: color-mix(in srgb, var(--text, #242424) 21%, var(--popover, #fff));
  pointer-events: none;
}
.quick-title-placeholder > svg {
  flex-shrink: 0;
}
.quick-title-placeholder > span {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.quick-title-field input {
  width: 100%;
  height: 38px;
  padding: 6px 10px;
  border: 1px solid var(--border, #e6e7eb);
  border-radius: 10px;
  outline: none;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 14px;
}
.quick-title-field input::placeholder {
  color: transparent;
}
.quick-title-field input:focus {
  border-color: var(--quick-accent);
}
.quick-add-footer {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 24px;
  margin-top: 20px;
}
.quick-add-footer button {
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.quick-metadata {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 24px;
  padding: 2px 3px;
  border: 0;
  border-radius: 4px;
  color: inherit;
  background: transparent;
  min-width: 0;
}
.quick-metadata:hover {
  background: var(--hover, #f1f1f4);
}
.quick-metadata:focus-visible,
.quick-add-submit:focus-visible {
  outline: 2px solid var(--quick-accent);
  outline-offset: 2px;
}
.quick-metadata svg {
  flex-shrink: 0;
}
.quick-metadata > span:last-child {
  transform: translateY(0.5px);
}
.quick-date {
  flex-shrink: 0;
}
.quick-date.dated {
  color: var(--accent, #587bff);
}
.quick-priority {
  flex-shrink: 0;
  width: 20px;
  padding-inline: 0;
}
.quick-priority-1 {
  color: #527de7;
}
.quick-priority-2 {
  color: #dfa647;
}
.quick-priority-3 {
  color: #dc6666;
}
.quick-list {
  flex: 0 1 auto;
  gap: 4px;
  justify-content: flex-start;
}
.quick-list > span:last-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.quick-list-emoji {
  width: 17px;
  flex-shrink: 0;
  font-size: 15px;
  line-height: 20px;
  text-align: center;
}
.quick-add-submit {
  flex-shrink: 0;
  width: 60px;
  height: 24px;
  padding: 0;
  margin-left: auto;
  border: 0;
  border-radius: 8px;
  background: var(--quick-accent);
  color: #fff;
}
.quick-add-submit:disabled {
  opacity: 1;
  background: color-mix(in srgb, var(--quick-accent) 42%, var(--popover, #fff));
  color: color-mix(in srgb, var(--popover, #fff) 58%, transparent);
  cursor: default;
}
.quick-add-popup {
  position: fixed;
  z-index: 101;
  max-width: calc(100vw - 24px);
  -webkit-app-region: no-drag;
}
.quick-add-popup :deep(.task-date-picker) {
  position: static;
  max-height: calc(100vh - 24px);
}
.quick-choice-menu {
  width: 190px;
  max-width: calc(100vw - 24px);
  max-height: min(360px, calc(100vh - 24px));
  overflow-y: auto;
  padding: 5px;
  border-radius: 10px;
  background: var(--popover, #fff);
  color: var(--text, #242424);
  box-shadow: 0 6px 22px #0003;
}
.quick-choice-menu button {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  min-height: 32px;
  padding: 6px 8px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}
.quick-choice-menu button > svg {
  flex-shrink: 0;
}
.quick-choice-menu button > span:not(.quick-list-emoji) {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.quick-choice-menu button:hover,
.quick-choice-menu button:focus-visible {
  background: var(--hover, #f1f1f4);
  outline: none;
}
.quick-menu-check {
  margin-left: auto;
}
.quick-list-menu {
  width: 220px;
}
@media (max-width: 420px) {
  .quick-add-footer {
    gap: 7px;
  }
  .quick-add-dialog {
    padding-inline: 16px;
  }
  .quick-add-submit {
    width: 54px;
  }
}
</style>
