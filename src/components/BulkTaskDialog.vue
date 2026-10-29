<script setup lang="ts">
import { computed, ref } from "vue";
import { PhTag } from "@phosphor-icons/vue";
import AppDialog from "./AppDialog.vue";
import TaskDatePicker from "./TaskDatePicker.vue";
import type { Tag, Task } from "../types";

const props = defineProps<{
  mode: "date" | "tags";
  task: Task;
  tasks: Task[];
  tags: Tag[];
  settings: Record<string, unknown>;
  busy: boolean;
  error: string;
}>();
const emit = defineEmits<{
  close: [];
  "apply-date": [patch: Record<string, unknown>];
  "apply-tags": [changes: Record<string, boolean>];
}>();
const query = ref("");
const tagChanges = ref<Record<string, boolean>>({});
const filteredTags = computed(() =>
  props.tags.filter((tag) =>
    tag.name.toLocaleLowerCase().includes(query.value.toLocaleLowerCase()),
  ),
);
function tagState(id: string) {
  if (id in tagChanges.value) return tagChanges.value[id] ? "all" : "none";
  const count = props.tasks.filter((task) => task.tag_ids.includes(id)).length;
  return count === props.tasks.length ? "all" : count ? "mixed" : "none";
}
</script>

<template>
  <AppDialog
    :title="
      mode === 'date'
        ? `Set date for ${tasks.length} tasks`
        : `Tags for ${tasks.length} tasks`
    "
    @close="!busy && emit('close')"
  >
    <p class="bulk-help">
      {{
        mode === "date"
          ? "Apply this date and reminder schedule to all selected tasks."
          : "Changed tags apply to every selected task. Other tags stay unchanged."
      }}
    </p>
    <div v-if="mode === 'date'" class="bulk-date-content" :inert="busy">
      <TaskDatePicker
        :task="task"
        :settings="settings"
        @save="emit('apply-date', $event)"
        @close="emit('close')"
      />
    </div>
    <form
      v-else
      class="dialog-form"
      @submit.prevent="emit('apply-tags', tagChanges)"
    >
      <input
        v-model="query"
        aria-label="Search tags"
        placeholder="Search tags"
        autofocus
      />
      <div class="bulk-tag-options">
        <label v-for="tag in filteredTags" :key="tag.id">
          <input
            type="checkbox"
            :aria-label="tag.name"
            :checked="tagState(tag.id) === 'all'"
            :indeterminate="tagState(tag.id) === 'mixed'"
            :disabled="busy"
            @change="
              tagChanges[tag.id] = ($event.target as HTMLInputElement).checked
            "
          />
          <PhTag :size="16" :style="{ color: tag.color }" />{{ tag.name }}
        </label>
        <p v-if="!filteredTags.length" class="bulk-help">No matching tags</p>
      </div>
      <footer>
        <button
          type="button"
          class="text-button"
          :disabled="busy"
          @click="emit('close')"
        >
          Cancel</button
        ><button
          class="primary-button"
          :disabled="busy || !Object.keys(tagChanges).length"
        >
          {{ busy ? "Saving…" : "Apply Tags" }}
        </button>
      </footer>
    </form>
    <p v-if="error" class="bulk-error" role="alert">{{ error }}</p>
  </AppDialog>
</template>

<style scoped>
.bulk-help {
  color: var(--muted);
  font-size: 12px;
  line-height: 1.5;
  margin: 0 0 14px;
}
.bulk-date-content {
  display: flex;
  justify-content: center;
}
.bulk-date-content :deep(.task-date-picker) {
  box-shadow: none;
  padding-top: 0;
  max-height: calc(100vh - 220px);
}
.bulk-tag-options {
  max-height: 320px;
  overflow-y: auto;
}
.bulk-tag-options label {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 9px 0;
  flex-direction: row;
}
.bulk-tag-options input {
  width: auto;
}
.bulk-error {
  color: var(--danger, #d3484f);
  font-size: 12px;
  margin: 12px 0 0;
}
</style>
