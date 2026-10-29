<script setup lang="ts">
import { computed, ref } from "vue";
import { PhCheck, PhRepeat } from "@phosphor-icons/vue";
import AppDialog from "./AppDialog.vue";
import { beginEditorWrite } from "./editorWork";
import { useTasks } from "../composables/useTasks";
import { parseTaskInput, splitTaskLines } from "../lib/task-input";
import type { Tag, Task, TaskList } from "../types";

const props = defineProps<{
  text: string;
  lists: TaskList[];
  tags: Tag[];
  settings: Record<string, unknown>;
  defaults?: Record<string, unknown>;
  overrides?: Record<string, unknown>;
}>();
const emit = defineEmits<{ close: []; created: [ids: string[]] }>();
const { mutate, flushAll } = useTasks();
const rows = ref(
  splitTaskLines(props.text).map((text, index) => ({
    id: index + 1,
    text,
    selected: true,
    created: "",
  })),
);
// Keep one clock reference while reviewing: metadata should not jump at
// midnight or between sequential writes in the same paste operation.
const referenceDate = new Date();
const busy = ref(false);
const error = ref("");
const listId = ref(
  String(props.defaults?.list_id || props.settings.default_list_id || "inbox"),
);
const availableLists = computed(() =>
  props.lists.filter((list) => !list.archived_at),
);
const previews = computed(() =>
  rows.value.map((row) => {
    const parsed = parseTaskInput(
      row.text,
      props.settings,
      props.lists,
      props.tags,
      referenceDate,
    );
    const inheritedTags = Array.isArray(props.defaults?.tag_ids)
      ? props.defaults.tag_ids
      : [];
    const parsedTags = Array.isArray(parsed.tag_ids) ? parsed.tag_ids : [];
    const tagIds = [...new Set([...inheritedTags, ...parsedTags])];
    const input: Record<string, unknown> = {
      ...props.defaults,
      list_id: listId.value,
      ...parsed,
      ...(tagIds.length ? { tag_ids: tagIds } : {}),
      // Explicit Quick Add controls take precedence over recognized text;
      // context defaults above still let each line choose its own date/list.
      ...props.overrides,
    };
    const date =
      typeof input.due_date === "string"
        ? input.due_date
        : "due_date" in input
          ? ""
          : defaultDate();
    const priority = Number(
      input.priority ?? props.settings.default_priority ?? 0,
    );
    const tags = tagIds
      .map((id) => props.tags.find((tag) => tag.id === id)?.name)
      .filter(Boolean);
    return {
      input,
      title: parsed.title,
      list:
        props.lists.find((list) => list.id === input.list_id)?.name || "Inbox",
      details: [
        date ? `${date}${input.due_time ? ` ${input.due_time}` : ""}` : "",
        priority ? `${["", "Low", "Medium", "High"][priority]} priority` : "",
        ...tags.map((name) => `#${name}`),
      ]
        .filter(Boolean)
        .join(" · "),
    };
  }),
);
const pending = computed(() =>
  rows.value.filter((row) => row.selected && !row.created && row.text.trim()),
);
const createdCount = computed(
  () => rows.value.filter((row) => row.created).length,
);
const allSelected = computed(() =>
  rows.value.every((row) => row.created || row.selected),
);
function defaultDate() {
  if (!["today", "tomorrow"].includes(String(props.settings.default_date)))
    return "";
  const date = new Date(referenceDate);
  if (props.settings.default_date === "tomorrow")
    date.setDate(date.getDate() + 1);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
function selectAll(event: Event) {
  const selected = (event.target as HTMLInputElement).checked;
  for (const row of rows.value) if (!row.created) row.selected = selected;
}
async function createSelected() {
  if (busy.value || !pending.value.length) return;
  busy.value = true;
  error.value = "";
  const finish = beginEditorWrite();
  try {
    if (!(await flushAll()))
      throw new Error(
        "Save your pending task changes before adding these tasks.",
      );
    // New tasks inserted at the top must be created in reverse clipboard
    // order. Mark each successful write so retries cannot duplicate it.
    const toCreate = pending.value.slice();
    if (props.settings.default_add_to === "top") toCreate.reverse();
    for (const row of toCreate) {
      const preview = previews.value[row.id - 1]!;
      const task = await mutate<Task>("tasks.create", preview.input);
      row.created = task.id;
    }
    emit("created", rows.value.map((row) => row.created).filter(Boolean));
    emit("close");
    finish();
  } catch (cause) {
    error.value = `${createdCount.value ? `${createdCount.value} already added. ` : ""}${cause instanceof Error ? cause.message : "Tasks could not be added. Try again."}`;
    finish(cause);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <AppDialog title="Add Multiple Tasks" wide @close="!busy && emit('close')">
    <form class="dialog-form batch-add" @submit.prevent="createSelected">
      <p class="batch-help">
        One task per line. Review the titles, dates, lists and tags before
        adding them.
      </p>
      <div class="batch-toolbar">
        <label class="batch-select-all">
          <input
            type="checkbox"
            :checked="allSelected"
            :disabled="busy"
            @change="selectAll"
          />
          Select all
        </label>
        <label class="batch-destination">
          List
          <select
            v-model="listId"
            aria-label="Default list for pasted tasks"
            :disabled="busy"
          >
            <option
              v-for="list in availableLists"
              :key="list.id"
              :value="list.id"
            >
              {{ list.name }}
            </option>
          </select>
        </label>
      </div>
      <div class="batch-rows">
        <div
          v-for="(row, index) in rows"
          :key="row.id"
          class="batch-row"
          :class="{ added: row.created }"
        >
          <PhCheck
            v-if="row.created"
            class="batch-check"
            :size="19"
            aria-label="Added"
          />
          <input
            v-else
            v-model="row.selected"
            class="batch-checkbox"
            type="checkbox"
            :aria-label="`Include task ${row.id}`"
            :disabled="busy"
          />
          <div class="batch-row-content">
            <input
              v-model="row.text"
              :aria-label="`Task text ${row.id}`"
              :disabled="busy || !!row.created"
              :autofocus="index === 0"
            />
            <div class="batch-preview" :aria-label="`Preview task ${row.id}`">
              <strong>{{
                previews[index]?.title || "Enter a task title"
              }}</strong>
              <span
                >{{ previews[index]?.list
                }}<template v-if="previews[index]?.details">
                  · {{ previews[index]?.details }}</template
                ></span
              >
              <span
                v-if="previews[index]?.input.recurrence"
                class="batch-repeat"
                ><PhRepeat :size="13" /> Repeats</span
              >
              <span v-if="row.created" class="batch-added-label">Added</span>
            </div>
          </div>
        </div>
      </div>
      <p v-if="error" class="batch-error" role="alert">{{ error }}</p>
      <footer>
        <span class="batch-total" aria-live="polite"
          >{{ pending.length }} selected<template v-if="createdCount">
            · {{ createdCount }} added</template
          ></span
        >
        <button
          type="button"
          class="text-button"
          :disabled="busy"
          @click="emit('close')"
        >
          {{ createdCount ? "Close" : "Cancel" }}
        </button>
        <button class="primary-button" :disabled="busy || !pending.length">
          {{
            busy
              ? "Adding…"
              : `Add ${pending.length} ${pending.length === 1 ? "Task" : "Tasks"}`
          }}
        </button>
      </footer>
    </form>
  </AppDialog>
</template>

<style scoped>
.batch-help {
  margin: 0;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.5;
}
.batch-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.batch-select-all,
.batch-destination {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.batch-select-all input,
.batch-checkbox {
  width: 15px;
  height: 15px;
  flex: 0 0 15px;
  accent-color: var(--accent);
}
.batch-destination select {
  max-width: 220px;
  min-width: 110px;
}
.batch-rows {
  max-height: min(480px, 52vh);
  overflow-y: auto;
  overscroll-behavior: contain;
  padding-right: 4px;
}
.batch-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 12px 0;
  border-top: 1px solid var(--border);
}
.batch-row > input,
.batch-check {
  margin-top: 12px;
}
.batch-row-content {
  min-width: 0;
  flex: 1;
}
.batch-preview {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 8px;
  padding-top: 6px;
  color: var(--muted);
  font-size: 11px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}
.batch-preview strong {
  width: 100%;
  font-weight: 500;
  color: var(--text);
}
.batch-repeat {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.batch-check,
.batch-added-label {
  color: var(--accent);
}
.added input {
  opacity: 0.65;
}
.batch-error {
  margin: 0;
  color: var(--danger, #d3484f);
  font-size: 12px;
}
.batch-total {
  margin-right: auto;
  color: var(--muted);
  font-size: 12px;
}
@media (max-width: 540px) {
  .batch-toolbar {
    align-items: flex-start;
  }
  .batch-destination select {
    max-width: 145px;
  }
  .batch-add footer {
    flex-wrap: wrap;
  }
  .batch-total {
    width: 100%;
  }
}
</style>
