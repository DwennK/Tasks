<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  PhCheck,
  PhCalendarBlank,
  PhFlag,
  PhPushPin,
  PhCopy,
  PhArrowSquareOut,
  PhTag,
  PhPlus,
  PhLink,
  PhList,
  PhTrash,
  PhXSquare,
  PhArrowCounterClockwise,
  PhCaretRight,
} from "@phosphor-icons/vue";
import type { Task, TaskList, Tag } from "../types";
const props = defineProps<{
  task: Task;
  x: number;
  y: number;
  count?: number;
  lists: TaskList[];
  tags: Tag[];
}>();
const emit = defineEmits<{
  close: [];
  action: [name: string, value?: unknown];
}>();
const panel = ref<HTMLElement>();
const submenu = ref<string | null>(null);
const position = computed(() => ({
  left: `${Math.min(props.x, window.innerWidth - 246)}px`,
  top: `${Math.max(8, Math.min(props.y, window.innerHeight - 430))}px`,
}));
function act(name: string, value?: unknown) {
  emit("action", name, value);
  emit("close");
}
function outside(event: MouseEvent) {
  if (!panel.value?.contains(event.target as Node)) emit("close");
}
function keyboard(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.stopPropagation();
    emit("close");
  }
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  const items = [
    ...panel.value!.querySelectorAll<HTMLButtonElement>("button"),
  ].filter((el) => el.offsetParent !== null);
  const index = items.indexOf(document.activeElement as HTMLButtonElement);
  items[
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? items.length - 1
        : (index + (event.key === "ArrowDown" ? 1 : items.length - 1)) %
          items.length
  ]?.focus();
}
onMounted(() => {
  document.addEventListener("mousedown", outside);
  panel.value?.querySelector("button")?.focus();
});
onBeforeUnmount(() => document.removeEventListener("mousedown", outside));
</script>
<template>
  <Teleport to="body"
    ><div
      ref="panel"
      class="task-context"
      :style="position"
      role="menu"
      aria-label="Task actions"
      @keydown="keyboard"
    >
      <div v-if="(count || 1) > 1" class="context-caption">
        {{ count }} tasks selected
      </div>
      <template v-if="task.deleted_at"
        ><button role="menuitem" @click="act('restore')">
          <PhArrowCounterClockwise />Restore
        </button></template
      >
      <template v-else>
        <button v-if="!task.is_note" role="menuitem" @click="act('complete')">
          <PhCheck />{{ task.completed_at ? "Mark as Undone" : "Complete" }}
        </button>
        <button v-if="!task.is_note" role="menuitem" @click="act('wont_do')">
          <PhXSquare />{{ task.wont_do_at ? "Mark as Undone" : "Won’t Do" }}
        </button>
        <div class="context-rule"></div>
        <button role="menuitem" @click="act('date')">
          <PhCalendarBlank />Date<kbd>⌘D</kbd>
        </button>
        <div class="context-priorities">
          <span>Priority</span
          ><button
            v-for="p in [3, 2, 1, 0]"
            :key="p"
            :aria-label="
              [
                'No priority',
                'Low priority',
                'Medium priority',
                'High priority',
              ][p]
            "
            :class="{ active: task.priority === p }"
            @click="act('priority', p)"
          >
            <PhFlag
              :class="`priority-color-${p}`"
              :weight="p ? 'fill' : 'regular'"
            />
          </button>
        </div>
        <button role="menuitem" @click="act('pin')">
          <PhPushPin />{{ task.pinned ? "Unpin" : "Pin" }}
        </button>
        <button
          role="menuitem"
          @click="submenu = submenu === 'move' ? null : 'move'"
          :aria-expanded="submenu === 'move'"
        >
          <PhArrowSquareOut />Move to<PhCaretRight class="context-arrow" />
        </button>
        <div v-if="submenu === 'move'" class="context-submenu">
          <button
            v-for="list in lists.filter((l) => !l.archived_at)"
            :key="list.id"
            @click="act('move', list.id)"
          >
            <PhList />{{ list.name
            }}<PhCheck v-if="task.list_id === list.id" class="context-arrow" />
          </button>
        </div>
        <button role="menuitem" @click="act('tags')">
          <PhTag />Tags<PhCaretRight class="context-arrow" />
        </button>
        <div class="context-rule"></div>
        <button v-if="!task.is_note" role="menuitem" @click="act('subtask')">
          <PhPlus />Add Subtask<kbd>⇧↩</kbd>
        </button>
        <button role="menuitem" @click="act('convert')">
          <PhList />{{ task.is_note ? "Convert to Task" : "Convert to Note" }}
        </button>
        <button role="menuitem" @click="act('duplicate')">
          <PhCopy />Duplicate
        </button>
        <button role="menuitem" @click="act('copy_link')">
          <PhLink />Copy Task Link
        </button>
        <button role="menuitem" @click="act('template')">
          <PhList />Save as Template
        </button>
        <div class="context-rule"></div>
        <button role="menuitem" class="danger-text" @click="act('delete')">
          <PhTrash />Delete<kbd>⌘⌫</kbd>
        </button>
      </template>
    </div></Teleport
  >
</template>
<style scoped>
.task-context {
  position: fixed;
  z-index: 300;
  width: 238px;
  max-height: calc(100vh - 16px);
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: var(--popover);
  padding: 5px;
  box-shadow: 0 8px 28px #0003;
  font-size: 13px;
  color: var(--text);
}
.task-context button {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 9px;
  min-height: 28px;
  padding: 5px 8px;
  text-align: left;
  border-radius: 4px;
  white-space: nowrap;
}
.task-context button:hover,
.task-context button:focus-visible {
  background: var(--accent);
  color: white;
  outline: none;
}
.task-context svg {
  width: 16px;
  height: 16px;
}
.task-context kbd,
.context-arrow {
  margin-left: auto;
  color: var(--muted);
  font-family: inherit;
  font-size: 11px;
}
.context-rule {
  height: 1px;
  background: var(--border);
  margin: 4px 7px;
}
.context-caption {
  font-size: 11px;
  color: var(--muted);
  padding: 7px;
}
.context-priorities {
  display: flex;
  align-items: center;
  padding: 2px 8px;
  gap: 3px;
}
.context-priorities span {
  margin-right: auto;
  font-size: 12px;
  color: var(--secondary);
}
.context-priorities button {
  width: 29px;
  min-height: 28px;
  justify-content: center;
  padding: 4px;
}
.context-priorities button.active {
  background: var(--hover);
}
.context-submenu {
  margin: 3px 6px 5px 16px;
  padding-left: 5px;
  border-left: 1px solid var(--border);
  max-height: 180px;
  overflow: auto;
}
.priority-color-3 {
  color: #e36b6c;
}
.priority-color-2 {
  color: #e9b157;
}
.priority-color-1 {
  color: #5e89e8;
}
.priority-color-0 {
  color: var(--muted);
}
</style>
