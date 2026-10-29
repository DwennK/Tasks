<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { Task } from "../types";
import { useTasks } from "../composables/useTasks";
import { beginEditorWrite } from "./editorWork";

const props = defineProps<{ task: Task }>();
const { tasks, mutate, flushAll } = useTasks();
const draft = ref(props.task.progress || 0);
const saving = ref(false);
const error = ref("");
const automatic = computed(() => props.task.progress_mode === "checklist");
const readonly = computed(
  () => automatic.value || !!props.task.completed_at || !!props.task.wont_do_at,
);
const explanation = computed(() =>
  automatic.value
    ? "Progress calculated from check items"
    : props.task.completed_at
      ? "Task completed"
      : "Drag to set progress. 100% completes the task.",
);

watch(
  () => [props.task.id, props.task.progress] as const,
  () => {
    draft.value = props.task.progress || 0;
    error.value = "";
  },
);

async function save(event: Event) {
  const progress = Number((event.target as HTMLInputElement).value);
  const taskId = props.task.id;
  if (
    readonly.value ||
    saving.value ||
    (progress === props.task.progress && progress !== 100)
  )
    return;
  const finish = beginEditorWrite();
  saving.value = true;
  error.value = "";
  try {
    if (!(await flushAll()))
      throw new Error("Save or retry your pending task changes first.");
    const current = tasks.value.find((task) => task.id === taskId);
    if (
      !current ||
      current.deleted_at ||
      current.completed_at ||
      current.wont_do_at
    )
      throw new Error("This task is no longer available for editing.");
    if (current.progress_mode === "checklist")
      throw new Error("Progress is now calculated from check items.");
    if (progress === 100) {
      await mutate("tasks.complete", { id: taskId });
    } else {
      await mutate("tasks.update", {
        id: taskId,
        progress,
        expected_version: current.version,
      });
    }
    finish();
  } catch (cause) {
    if (props.task.id === taskId) {
      draft.value = props.task.progress || 0;
      error.value = cause instanceof Error ? cause.message : String(cause);
    }
    finish(cause);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div
    v-if="!task.is_note && !task.deleted_at"
    class="task-progress"
    :class="{ 'has-progress': draft > 0, automatic, saving }"
    :style="{ '--task-progress': `${draft}%` }"
    :title="explanation"
  >
    <div
      v-if="readonly"
      class="progress-track"
      role="progressbar"
      :aria-label="automatic ? 'Checklist progress' : 'Task progress'"
      :aria-valuenow="draft"
      :aria-valuemin="0"
      :aria-valuemax="100"
      :aria-valuetext="`${draft}%`"
    >
      <span />
    </div>
    <input
      v-else
      v-model.number="draft"
      class="progress-slider"
      type="range"
      min="0"
      max="100"
      step="1"
      aria-label="Task progress"
      :aria-valuetext="`${draft}%`"
      :disabled="saving"
      @change="save"
    />
    <span class="progress-value" aria-hidden="true">{{ draft }}%</span>
    <p v-if="error" class="progress-error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
.task-progress {
  position: relative;
  flex-shrink: 0;
  height: 14px;
  margin: -7px 24px 0;
  z-index: 1;
}
.progress-slider,
.progress-track {
  position: absolute;
  top: 5px;
  width: 100%;
  height: 3px;
  margin: 0;
  border: 0;
  border-radius: 2px;
  background: var(--border);
}
.progress-slider {
  top: -2px;
  height: 18px;
  appearance: none;
  -webkit-appearance: none;
  background: linear-gradient(
      to right,
      var(--accent) var(--task-progress),
      var(--border) var(--task-progress)
    )
    center / 100% 3px no-repeat;
  cursor: pointer;
}
.progress-slider::-webkit-slider-thumb {
  appearance: none;
  -webkit-appearance: none;
  width: 11px;
  height: 11px;
  border: 2px solid var(--surface, white);
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 0 1px var(--accent);
  opacity: 0;
}
.progress-slider::-moz-range-thumb {
  width: 8px;
  height: 8px;
  border: 2px solid var(--surface, white);
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 0 1px var(--accent);
  opacity: 0;
}
.task-progress:hover .progress-slider::-webkit-slider-thumb,
.progress-slider:focus-visible::-webkit-slider-thumb {
  opacity: 1;
}
.task-progress:hover .progress-slider::-moz-range-thumb,
.progress-slider:focus-visible::-moz-range-thumb {
  opacity: 1;
}
.progress-track > span {
  display: block;
  width: var(--task-progress);
  height: 100%;
  border-radius: inherit;
  background: var(--accent);
}
.progress-value {
  position: absolute;
  top: 12px;
  right: 0;
  color: var(--muted);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  line-height: 14px;
  opacity: 0;
  pointer-events: none;
}
.task-progress:hover .progress-value,
.task-progress:focus-within .progress-value,
.has-progress .progress-value {
  opacity: 1;
}
.progress-error {
  position: absolute;
  inset: 12px 0 auto;
  margin: 0;
  padding: 5px 8px;
  border-radius: 4px;
  background: var(--surface, white);
  color: var(--danger, #d14545);
  font-size: 12px;
  box-shadow: 0 2px 8px #0001;
}
.saving {
  opacity: 0.6;
}
</style>
