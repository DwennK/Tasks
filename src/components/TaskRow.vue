<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import {
  PhCheck,
  PhTextAlignLeft,
  PhCaretRight,
  PhArrowCounterClockwise,
  PhPushPin,
  PhRepeat,
  PhPaperclip,
  PhDotsThree,
  PhX,
  PhNote,
} from "@phosphor-icons/vue";
import type { Task } from "../types";
import { dateLabel, localDate } from "../composables/useTasks";
import { useTaskTitleField } from "../composables/useTaskTitleField";
import { canDropTask, clearTaskDrag, draggedTask, taskDropPosition, taskDropTarget } from "../composables/taskDrag";
const props = defineProps<{
  task: Task;
  selected?: boolean;
  subtasks?: number;
  listName?: string;
  draggable?: boolean;
  checked?: boolean;
  listColor?: string;
  editingTitle?: boolean;
  quitting?: boolean;
}>();
const emit = defineEmits<{
  select: [id: string, event: MouseEvent];
  context: [event: MouseEvent, task: Task];
  complete: [task: Task];
  restore: [id: string];
  drop: [event: DragEvent, task: Task];
  "edit-title": [id: string, event: MouseEvent];
  "finish-title": [id: string, action: "blur" | "escape" | "below" | "subtask"];
}>();
const titleInput = ref<HTMLInputElement>();
const titleField = useTaskTitleField(() => props.task, titleInput);
watch(
  () => props.editingTitle,
  async (editing) => {
    if (!editing || props.task.deleted_at) return;
    await nextTick();
    titleInput.value?.focus();
    const end = titleInput.value?.value.length || 0;
    titleInput.value?.setSelectionRange(end, end);
  },
  { immediate: true },
);
function titleKey(event: KeyboardEvent) {
  if (
    event.isComposing ||
    titleField.composing.value ||
    event.keyCode === 229
  ) {
    event.stopPropagation();
    return;
  }
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    titleField.reconcile();
    emit("finish-title", props.task.id, "escape");
  } else if (
    event.key === "Enter" &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.altKey
  ) {
    event.preventDefault();
    event.stopPropagation();
    titleField.reconcile();
    emit("finish-title", props.task.id, event.shiftKey ? "subtask" : "below");
  }
}
function titleBlur() {
  titleField.blur();
  if (!titleField.composing.value && !props.quitting)
    emit("finish-title", props.task.id, "blur");
}
function endComposition(event: CompositionEvent) {
  titleField.compositionEnd(event);
  if (document.activeElement !== titleInput.value && !props.quitting)
    emit("finish-title", props.task.id, "blur");
}
function titleClick(event: MouseEvent) {
  if (!props.editingTitle || event.metaKey || event.ctrlKey || event.shiftKey)
    emit("edit-title", props.task.id, event);
}
function drag(event: DragEvent, task: Task) {
  if (!props.draggable || props.editingTitle) {
    event.preventDefault();
    return;
  }
  if (event.dataTransfer) {
    draggedTask.value = task;
    taskDropTarget.value = null;
    event.dataTransfer.setData(
      "application/x-tasks",
      JSON.stringify({ kind: "task", id: task.id }),
    );
    event.dataTransfer.effectAllowed = "move";
  }
}
function dragOver(event: DragEvent) {
  event.stopPropagation();
  if (!props.draggable || !canDropTask(draggedTask.value, props.task)) {
    taskDropTarget.value = null;
    if (event.dataTransfer) event.dataTransfer.dropEffect = "none";
    return;
  }
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  taskDropTarget.value = { id: props.task.id, position: taskDropPosition(event) };
}
function dragLeave(event: DragEvent) {
  const row = event.currentTarget as HTMLElement;
  if (event.relatedTarget instanceof Node && row.contains(event.relatedTarget)) return;
  if (taskDropTarget.value?.id === props.task.id) taskDropTarget.value = null;
}
function drop(event: DragEvent) {
  if (props.draggable && canDropTask(draggedTask.value, props.task))
    emit("drop", event, props.task);
  clearTaskDrag();
}
</script>
<template>
  <div
    class="task-row"
    :class="{
      selected: selected || checked,
      dismissed: task.wont_do_at,
      completed: task.completed_at,
      'in-trash': task.deleted_at,
      'is-dragging': draggedTask?.id === task.id,
    }"
    :style="listColor ? { borderLeft: `2px solid ${listColor}` } : undefined"
    :draggable="draggable && !editingTitle"
    @dragstart="drag($event, task)"
    @dragover="dragOver"
    @dragleave="dragLeave"
    @dragend="clearTaskDrag"
    @drop.stop.prevent="drop"
    @click="emit('select', task.id, $event)"
    @contextmenu.prevent="emit('context', $event, task)"
    role="group"
    :aria-label="task.title"
    :aria-current="selected ? 'true' : undefined"
  >
    <PhNote
      v-if="task.is_note && !task.deleted_at"
      class="task-note-icon"
      :size="17"
    />
    <button
      v-else-if="!task.deleted_at"
      class="task-checkbox"
      :class="[`priority-${task.priority}`, { checked: task.completed_at }]"
      @click.stop="emit('complete', task)"
      :aria-label="`${task.completed_at ? 'Reopen' : 'Complete'} ${task.title}`"
    >
      <PhX v-if="task.wont_do_at" :size="12" weight="bold" />
      <PhCheck v-else-if="task.completed_at" :size="12" weight="bold" />
    </button>
    <button
      v-else
      class="icon-button restore-row"
      @click.stop="emit('restore', task.id)"
      :aria-label="`Restore ${task.title}`"
    >
      <PhArrowCounterClockwise :size="17" />
    </button>
    <input
      v-if="editingTitle && !task.deleted_at"
      ref="titleInput"
      class="task-row-title task-row-title-input"
      aria-label="Edit task title"
      autocomplete="off"
      spellcheck="false"
      @focus="titleField.focus"
      @beforeinput="titleField.beforeInput"
      @input="titleField.input"
      @compositionstart="titleField.compositionStart"
      @compositionend="endComposition"
      @blur="titleBlur"
      @keydown="titleKey"
      @click.stop="titleClick"
      @pointerdown="
        ($event.metaKey || $event.ctrlKey || $event.shiftKey) &&
        $event.preventDefault()
      "
      @dragstart.stop.prevent
    />
    <button
      v-else
      class="task-row-title"
      :aria-label="`Open task ${task.title}`"
      @click.stop="titleClick"
    >
      {{ task.title }}
    </button>
    <PhPushPin v-if="task.pinned" class="note-indicator" :size="12" />
    <PhRepeat v-if="task.recurrence" class="note-indicator" :size="12" />
    <PhPaperclip
      v-if="task.attachments?.length"
      class="note-indicator"
      :size="12"
    />
    <span v-if="listName" class="task-list-label">{{ listName }}</span>
    <span v-if="subtasks" class="subtask-count"
      ><PhCaretRight :size="10" />{{ subtasks }}</span
    >
    <PhTextAlignLeft v-if="task.notes" class="note-indicator" :size="12" />
    <span
      v-if="task.due_date"
      class="due-label"
      :class="{
        overdue: !task.completed_at && task.due_date < localDate(),
        today: task.due_date === localDate(),
      }"
      >{{ dateLabel(task.due_date) }}</span
    >
    <button
      class="icon-button task-row-more"
      :aria-label="`Actions for ${task.title}`"
      @click.stop="emit('context', $event, task)"
    >
      <PhDotsThree :size="17" />
    </button>
  </div>
</template>
<style scoped>
.task-row.is-dragging {
  opacity: 0.4;
}
.task-row-title-input {
  height: 100%;
  padding: 0;
  border: 0;
  border-radius: 0;
  outline: none;
  background: transparent;
  color: inherit;
  font: inherit;
  line-height: inherit;
  text-align: left;
}
.task-note-icon {
  flex-shrink: 0;
  color: var(--muted);
}
.task-row-more {
  display: none;
  width: 20px;
  height: 23px;
  margin-left: 2px;
}
.task-row:hover .task-row-more,
.task-row:focus-within .task-row-more {
  display: flex;
}
.task-row.dismissed .task-row-title {
  color: var(--muted);
  text-decoration: line-through;
}
.task-row.dismissed .task-checkbox {
  color: var(--muted);
}
</style>
