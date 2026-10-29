<script setup lang="ts">
import { computed, ref } from "vue";
import {
  PhArrowElbowUpLeft,
  PhCheck,
  PhMagnifyingGlass,
} from "@phosphor-icons/vue";
import AppDialog from "./AppDialog.vue";
import type { Task, TaskList } from "../types";
import { dateLabel, localDate } from "../composables/useTasks";

const props = defineProps<{ task: Task; tasks: Task[]; lists: TaskList[] }>();
const emit = defineEmits<{ select: [task: Task | null]; close: [] }>();
const query = ref("");
const search = ref<HTMLInputElement>();
const results = ref<HTMLElement>();
const taskMap = computed(
  () => new Map(props.tasks.map((task) => [task.id, task])),
);
const listMap = computed(
  () => new Map(props.lists.map((list) => [list.id, list])),
);
const excludedIds = computed(() => {
  const children = new Map<string, string[]>();
  for (const task of props.tasks) {
    if (!task.parent_id) continue;
    const siblings = children.get(task.parent_id) || [];
    siblings.push(task.id);
    children.set(task.parent_id, siblings);
  }
  const excluded = new Set<string>();
  const pending = [props.task.id];
  while (pending.length) {
    const id = pending.pop()!;
    if (excluded.has(id)) continue;
    excluded.add(id);
    pending.push(...(children.get(id) || []));
  }
  return excluded;
});
function normalize(value: string) {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLocaleLowerCase();
}
function isNote(task: Task) {
  return !!(task as Task & { is_note?: boolean }).is_note;
}
function breadcrumb(task: Task) {
  const path: string[] = [];
  const visited = new Set([task.id]);
  let parentId = task.parent_id;
  while (parentId && !visited.has(parentId)) {
    visited.add(parentId);
    const parent = taskMap.value.get(parentId);
    if (!parent) break;
    path.unshift(parent.title || "Untitled task");
    parentId = parent.parent_id;
  }
  path.unshift(listMap.value.get(task.list_id)?.name || "Inbox");
  return path.join(" › ");
}
const candidates = computed(() => {
  const needle = normalize(query.value.trim());
  return props.tasks
    .filter((task) => {
      const list = listMap.value.get(task.list_id);
      if (
        excludedIds.value.has(task.id) ||
        task.deleted_at ||
        task.completed_at ||
        task.wont_do_at ||
        isNote(task) ||
        list?.archived_at ||
        (list as (TaskList & { list_type?: string }) | undefined)?.list_type ===
          "note"
      )
        return false;
      return !needle || normalize(task.title).includes(needle);
    })
    .sort((a, b) => {
      if (a.id === props.task.parent_id) return -1;
      if (b.id === props.task.parent_id) return 1;
      const aList = listMap.value.get(a.list_id),
        bList = listMap.value.get(b.list_id);
      return (
        (aList?.position || 0) - (bList?.position || 0) ||
        (aList?.name || "").localeCompare(bList?.name || "") ||
        a.position - b.position ||
        a.title.localeCompare(b.title)
      );
    });
});
function select(task: Task | null) {
  if (task?.id === props.task.parent_id) return;
  emit("select", task);
}
function resultButtons() {
  return [
    ...(results.value?.querySelectorAll<HTMLButtonElement>(
      "button:not(:disabled)",
    ) || []),
  ];
}
function searchKey(event: KeyboardEvent) {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    resultButtons()[0]?.focus();
  } else if (event.key === "Enter" && !event.isComposing) {
    event.preventDefault();
    const first = candidates.value.find(
      (task) => task.id !== props.task.parent_id,
    );
    if (first) select(first);
  }
}
function resultKey(event: KeyboardEvent) {
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  const buttons = resultButtons();
  const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
  if (index < 0) return;
  event.preventDefault();
  if (event.key === "ArrowUp" && index === 0) search.value?.focus();
  else {
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? buttons.length - 1
          : Math.min(
              buttons.length - 1,
              Math.max(0, index + (event.key === "ArrowDown" ? 1 : -1)),
            );
    buttons[next]?.focus();
  }
}
</script>

<template>
  <AppDialog
    title="Link Parent Task"
    panel-class="parent-task-dialog"
    @close="emit('close')"
  >
    <div class="parent-task-picker">
      <div class="parent-search">
        <PhMagnifyingGlass :size="19" />
        <input
          ref="search"
          v-model="query"
          autofocus
          aria-label="Find parent task"
          placeholder="Search"
          autocomplete="off"
          @keydown="searchKey"
        />
      </div>
      <button
        v-if="task.parent_id"
        type="button"
        class="top-level-option"
        @click="select(null)"
      >
        <PhArrowElbowUpLeft :size="17" /><span>Move to top level</span>
      </button>
      <div
        ref="results"
        class="parent-results"
        role="group"
        aria-label="Available parent tasks"
        @keydown="resultKey"
      >
        <button
          v-for="candidate in candidates"
          :key="candidate.id"
          type="button"
          class="parent-task-option"
          :class="{ current: candidate.id === task.parent_id }"
          :disabled="candidate.id === task.parent_id"
          :aria-label="
            candidate.id === task.parent_id
              ? `${candidate.title}, current parent task`
              : `Link parent task ${candidate.title}`
          "
          @click="select(candidate)"
        >
          <span
            class="parent-task-checkbox"
            :class="{ selected: candidate.id === task.parent_id }"
            aria-hidden="true"
          >
            <PhCheck
              v-if="candidate.id === task.parent_id"
              :size="10"
              weight="bold"
            />
          </span>
          <span class="parent-task-title">{{ candidate.title }}</span>
          <span class="parent-task-list" :title="breadcrumb(candidate)">{{
            listMap.get(candidate.list_id)?.name || "Inbox"
          }}</span>
          <span
            v-if="candidate.due_date"
            class="parent-task-date"
            :class="{ overdue: candidate.due_date < localDate() }"
            >{{ dateLabel(candidate.due_date) }}</span
          >
        </button>
        <p v-if="!candidates.length" class="parent-empty" role="status">
          {{ query.trim() ? "No matching tasks" : "No available parent tasks" }}
        </p>
      </div>
      <span class="parent-search-status" aria-live="polite"
        >{{ candidates.length }}
        {{ candidates.length === 1 ? "task" : "tasks" }} found</span
      >
    </div>
  </AppDialog>
</template>

<style scoped>
:global(.dialog.parent-task-dialog) {
  position: relative;
  width: 560px;
  height: 426px;
  max-width: calc(100vw - 40px);
  max-height: calc(100dvh - 60px);
  padding: 0 20px 20px;
  border: 0;
  border-radius: 12px;
  overflow: hidden;
}
:global(.parent-task-dialog > header) {
  position: absolute;
  top: 15px;
  right: 20px;
  height: 24px;
  margin: 0;
  z-index: 1;
}
:global(.parent-task-dialog > header h2) {
  display: none;
}
:global(.parent-task-dialog > header .icon-button) {
  width: 24px;
  height: 24px;
  color: var(--secondary);
}
.parent-task-picker {
  height: 100%;
  display: flex;
  flex-direction: column;
  color: var(--text);
}
.parent-search {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 0 0 55px;
  min-height: 55px;
  padding: 0 40px 0 2px;
  border-bottom: 1px solid var(--border);
}
.parent-search:focus-within {
  border-bottom-color: var(--control-accent, #303c70);
}
.parent-search > svg {
  color: var(--secondary);
  flex-shrink: 0;
}
.parent-search input {
  min-width: 0;
  width: 100%;
  flex: 1;
  border: 0;
  background: transparent;
  padding: 0;
  height: 38px;
  font-size: 16px;
  color: var(--text);
}
.parent-search input::placeholder {
  color: var(--muted);
  opacity: 0.55;
}
.parent-search input:focus-visible {
  outline: none;
}
.top-level-option {
  width: 100%;
  display: flex;
  align-items: center;
  flex: 0 0 36px;
  gap: 8px;
  padding: 0;
  margin-top: 6px;
  border-bottom: 1px solid var(--border);
  font-size: 13px;
  color: var(--secondary);
  text-align: left;
}
.top-level-option:hover {
  color: var(--text);
  background: var(--hover);
}
.parent-results {
  min-height: 0;
  flex: 1;
  overflow-y: auto;
  padding-top: 6px;
}
.parent-task-option {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 36px;
  padding: 0;
  border-radius: 4px;
  text-align: left;
}
.parent-task-option:hover,
.parent-task-option:focus-visible {
  background: var(--hover);
}
.parent-task-option.current {
  opacity: 1;
  color: var(--secondary);
}
.parent-task-checkbox {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  border: 1px solid var(--muted);
  border-radius: 50%;
  flex-shrink: 0;
}
.parent-task-checkbox.selected {
  color: var(--popover);
  border-color: var(--control-accent, #303c70);
  background: var(--control-accent, #303c70);
}
.parent-task-title {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: 14px;
  line-height: 20px;
}
.parent-task-list {
  flex: 0 1 auto;
  max-width: 150px;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  color: var(--muted);
  font-size: 12px;
  line-height: 18px;
}
.parent-task-date {
  flex-shrink: 0;
  color: var(--accent);
  font-size: 14px;
  line-height: 20px;
}
.parent-task-date.overdue {
  color: var(--danger, #e65a55);
}
.parent-empty {
  margin: 0;
  padding: 38px 10px;
  font-size: 13px;
  color: var(--muted);
  text-align: center;
}
.parent-search-status {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
@media (max-width: 600px) {
  .parent-task-list {
    max-width: 90px;
  }
}
</style>
