<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { PhCaretDown, PhCaretRight } from "@phosphor-icons/vue";
import TaskRow from "./TaskRow.vue";
import { taskDropTarget } from "../composables/taskDrag";
import type { Task } from "../types";

const props = withDefaults(
  defineProps<{
    task: Task;
    tasks: Task[];
    tasksById?: Map<string, Task>;
    tasksByParent?: Map<string, Task[]>;
    selectedId: string | null;
    selectedIds: Set<string>;
    listName?: string;
    listColor?: string;
    draggable?: boolean;
    showCompleted?: boolean;
    ancestorIds?: string[];
    expansionCommand?: { revision: number; expanded: boolean } | null;
    addBelowId?: string | null;
    addBelowTitle?: string;
    addBelowBusy?: boolean;
    editingTitleId?: string | null;
    quitting?: boolean;
  }>(),
  { showCompleted: false, ancestorIds: () => [] },
);
const emit = defineEmits<{
  select: [id: string, event: MouseEvent];
  complete: [task: Task];
  restore: [id: string];
  drop: [event: DragEvent, task: Task];
  context: [event: MouseEvent, task: Task];
  dragstart: [event: DragEvent, task: Task];
  dragend: [event: DragEvent, task: Task];
  "update-add-below": [title: string];
  "create-below": [];
  "cancel-add-below": [];
  "edit-title": [id: string, event: MouseEvent];
  "finish-title": [id: string, action: "blur" | "escape" | "below" | "subtask"];
}>();
const expanded = ref(props.expansionCommand?.expanded ?? false);
const addBelowInput = ref<HTMLInputElement>();
const composingBelow = ref(false);
watch(
  () => props.expansionCommand?.revision,
  () => {
    if (props.expansionCommand)
      expanded.value = props.expansionCommand.expanded;
  },
);
watch(
  () => props.addBelowId,
  async (id) => {
    if (id && isDescendant(id)) expanded.value = true;
    if (id !== props.task.id) return;
    await nextTick();
    addBelowInput.value?.focus();
  },
  { immediate: true },
);
function submitBelow(event?: KeyboardEvent) {
  if (event?.isComposing || composingBelow.value || props.addBelowBusy) return;
  emit("create-below");
}
function blurBelow() {
  if (composingBelow.value || props.addBelowBusy) return;
  if (props.addBelowTitle?.trim()) emit("create-below");
  else emit("cancel-add-below");
}
const lineage = computed(() => [...props.ancestorIds, props.task.id]);
const children = computed(() =>
  (props.tasksByParent
    ? props.tasksByParent.get(props.task.id) || []
    : props.tasks.filter((task) => task.parent_id === props.task.id)
  )
    .filter((task) => {
      if (task.parent_id !== props.task.id || lineage.value.includes(task.id))
        return false;
      if (props.task.deleted_at) return !!task.deleted_at;
      if (task.deleted_at) return false;
      if (props.task.wont_do_at) return !!task.wont_do_at;
      if (task.wont_do_at) return false;
      if (props.task.completed_at) return !!task.completed_at;
      return props.showCompleted || !task.completed_at;
    })
    .sort(
      (a, b) =>
        a.position - b.position || a.created_at.localeCompare(b.created_at),
    ),
);
const childRegion = computed(() => `task-children-${props.task.id}`);
function isDescendant(id: string | null) {
  const visited = new Set<string>();
  let task = id
    ? props.tasksById?.get(id) || props.tasks.find((task) => task.id === id)
    : undefined;
  while (task?.parent_id && !visited.has(task.id)) {
    visited.add(task.id);
    if (task.parent_id === props.task.id) return true;
    task =
      props.tasksById?.get(task.parent_id) ||
      props.tasks.find((value) => value.id === task?.parent_id);
  }
  return false;
}
watch(
  () => props.selectedId,
  () => {
    if (isDescendant(props.selectedId)) expanded.value = true;
  },
  { immediate: true },
);
function disclosureKey(event: KeyboardEvent) {
  if (event.key === "ArrowRight") {
    event.preventDefault();
    event.stopPropagation();
    expanded.value = true;
  }
  if (event.key === "ArrowLeft") {
    event.preventDefault();
    event.stopPropagation();
    expanded.value = false;
  }
}
defineExpose({
  setExpanded(value: boolean) {
    expanded.value = value;
  },
});
</script>

<template>
  <div class="task-tree-node" :data-task-id="task.id">
    <div
      v-if="taskDropTarget?.id === task.id"
      class="task-drop-indicator"
      :data-position="taskDropTarget.position"
      aria-hidden="true"
    />
    <div class="task-tree-line">
      <TaskRow
        :task="task"
        :selected="task.id === selectedId"
        :checked="selectedIds.has(task.id)"
        :subtasks="children.length"
        :draggable="draggable"
        :list-name="listName"
        :list-color="listColor"
        :editing-title="editingTitleId === task.id"
        :quitting="quitting"
        @edit-title="(id, event) => emit('edit-title', id, event)"
        @finish-title="(id, action) => emit('finish-title', id, action)"
        @select="(id, event) => emit('select', id, event)"
        @complete="emit('complete', $event)"
        @restore="emit('restore', $event)"
        @drop="(event, value) => emit('drop', event, value)"
        @context="(event, value) => emit('context', event, value)"
        @dragstart="emit('dragstart', $event, task)"
        @dragend="emit('dragend', $event, task)"
      />
      <button
        v-if="children.length"
        class="subtask-disclosure"
        :aria-label="`${expanded ? 'Collapse' : 'Expand'} subtasks for ${task.title}`"
        :aria-expanded="expanded"
        :aria-controls="childRegion"
        :draggable="false"
        @click.stop="expanded = !expanded"
        @keydown="disclosureKey"
        @dragstart.stop.prevent
      >
        <PhCaretDown v-if="expanded" :size="11" /><PhCaretRight
          v-else
          :size="11"
        />
      </button>
    </div>
    <div
      v-if="expanded && children.length"
      :id="childRegion"
      class="task-tree-children"
      role="group"
      :aria-label="`Subtasks of ${task.title}`"
    >
      <TaskTreeRow
        v-for="child in children"
        :key="child.id"
        :task="child"
        :tasks="tasks"
        :tasks-by-id="tasksById"
        :tasks-by-parent="tasksByParent"
        :selected-id="selectedId"
        :selected-ids="selectedIds"
        :list-name="listName"
        :list-color="listColor"
        :draggable="draggable"
        :show-completed="showCompleted"
        :ancestor-ids="lineage"
        :expansion-command="expansionCommand"
        :add-below-id="addBelowId"
        :add-below-title="addBelowTitle"
        :add-below-busy="addBelowBusy"
        :editing-title-id="editingTitleId"
        :quitting="quitting"
        @edit-title="(id, event) => emit('edit-title', id, event)"
        @finish-title="(id, action) => emit('finish-title', id, action)"
        @update-add-below="emit('update-add-below', $event)"
        @create-below="emit('create-below')"
        @cancel-add-below="emit('cancel-add-below')"
        @select="(id, event) => emit('select', id, event)"
        @complete="emit('complete', $event)"
        @restore="emit('restore', $event)"
        @drop="(event, value) => emit('drop', event, value)"
        @context="(event, value) => emit('context', event, value)"
        @dragstart="(event, value) => emit('dragstart', event, value)"
        @dragend="(event, value) => emit('dragend', event, value)"
      />
    </div>
    <form
      v-if="addBelowId === task.id"
      class="task-add-below"
      @submit.prevent="submitBelow()"
      @keydown.esc.stop.prevent="emit('cancel-add-below')"
    >
      <span class="task-add-below-checkbox" aria-hidden="true"></span>
      <input
        ref="addBelowInput"
        :value="addBelowTitle"
        :disabled="addBelowBusy"
        :aria-label="`New task below ${task.title}`"
        placeholder="Add task"
        autocomplete="off"
        @input="
          emit('update-add-below', ($event.target as HTMLInputElement).value)
        "
        @compositionstart="composingBelow = true"
        @compositionend="composingBelow = false"
        @keydown.enter.stop.prevent="submitBelow($event)"
        @blur="blurBelow"
      />
    </form>
  </div>
</template>

<style scoped>
.task-tree-node {
  position: relative;
}
.task-drop-indicator {
  position: absolute;
  z-index: 4;
  left: 17px;
  right: 7px;
  height: 2px;
  background: var(--accent);
  pointer-events: none;
}
.task-drop-indicator[data-position="before"] {
  top: -1px;
}
.task-drop-indicator[data-position="after"] {
  bottom: -1px;
}
.task-drop-indicator::before {
  content: "";
  position: absolute;
  left: -3px;
  top: -2px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
}
.task-add-below {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 42px;
  margin-left: 17px;
  padding: 0 8px;
  border-bottom: 1px solid var(--border);
}
.task-add-below-checkbox {
  flex: 0 0 15px;
  height: 15px;
  border: 1.5px solid var(--muted);
  border-radius: 4px;
}
.task-add-below input {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: none;
  padding: 8px 0;
  color: var(--text);
  background: transparent;
  font-size: 14px;
}
.task-add-below input::placeholder {
  color: var(--muted);
}
.task-tree-node,
.task-tree-line {
  min-width: 0;
}
.task-tree-line {
  position: relative;
}
.subtask-disclosure {
  position: absolute;
  z-index: 1;
  left: 0;
  top: 50%;
  transform: translateY(-50%);
  width: 15px;
  height: 26px;
  padding: 0;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--muted, #999aa1);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.subtask-disclosure:hover {
  color: var(--text, #26272b);
  background: var(--hover, #e8e9ef);
}
.subtask-disclosure:focus-visible {
  outline: 2px solid var(--accent, #313d70);
  outline-offset: -1px;
}
.task-tree-children {
  margin-left: 20px;
  min-width: 0;
}
@media (max-width: 720px) {
  .subtask-disclosure {
    width: 12px;
  }
  .task-tree-children {
    margin-left: 18px;
  }
}
</style>
