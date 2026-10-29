<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  PhCopy,
  PhDownloadSimple,
  PhNote,
  PhPlus,
  PhTrash,
} from "@phosphor-icons/vue";
import { isNative } from "../adapter";
import type { Task, TaskList, Tag } from "../types";
import { beginEditorWrite } from "./editorWork";
import { serializeNotes } from "./taskExport";
import {
  buildSummary,
  defaultSummaryOptions,
  summaryMarkdown,
  summaryRange,
  type SummaryField,
  type SummaryOptions,
  type SummaryPreset,
} from "../lib/task-summary";

const props = withDefaults(
  defineProps<{
    tasks: Task[];
    lists: TaskList[];
    tags: Tag[];
    presets?: SummaryPreset[];
    weekStart?: number;
    embedded?: boolean;
  }>(),
  { presets: () => [], weekStart: 1, embedded: false },
);
const emit = defineEmits<{
  "save-presets": [presets: SummaryPreset[]];
  "create-note": [
    note: {
      title: string;
      notes: string;
      notes_format: "markdown";
      is_note: true;
    },
  ];
  "open-task": [id: string];
}>();
const options = ref<SummaryOptions>(
  defaultSummaryOptions(new Date(), props.weekStart),
);
const selectedPreset = ref("");
const presetName = ref("");
const savePresetOpen = ref(false);
const tab = ref<"preview" | "tasks">("preview");
const format = ref<"markdown" | "text">("markdown");
const pending = ref(false);
const error = ref("");
const status = ref("");
const dateError = computed(() =>
  !options.value.from || !options.value.to
    ? "Choose a start and end date."
    : options.value.from > options.value.to
      ? "The end date must be on or after the start date."
      : "",
);
const groups = computed(() =>
  buildSummary(props.tasks, props.lists, options.value),
);
const count = computed(() =>
  groups.value.reduce((total, group) => total + group.entries.length, 0),
);
const generated = computed(() =>
  summaryMarkdown(groups.value, props.lists, props.tags, options.value),
);
const preview = ref(generated.value);
const edited = ref(false);
const sourceChanged = ref(false);
const fieldChoices: { value: SummaryField; label: string }[] = [
  { value: "list", label: "List" },
  { value: "due", label: "Due date" },
  { value: "completed", label: "Completion date" },
  { value: "priority", label: "Priority" },
  { value: "tags", label: "Tags" },
  { value: "progress", label: "Progress" },
  { value: "notes", label: "Description" },
];
watch(generated, (value) => {
  status.value = "";
  if (edited.value) sourceChanged.value = true;
  else preview.value = value;
});
function regenerate() {
  preview.value = generated.value;
  edited.value = false;
  sourceChanged.value = false;
}
function setRange(event: Event) {
  options.value.range = (event.target as HTMLSelectElement)
    .value as SummaryOptions["range"];
  if (options.value.range !== "custom")
    Object.assign(
      options.value,
      summaryRange(options.value.range, new Date(), props.weekStart),
    );
}
function applyPreset() {
  const preset = props.presets.find((item) => item.id === selectedPreset.value);
  if (!preset) return;
  options.value = JSON.parse(JSON.stringify(preset.options));
  if (options.value.range !== "custom")
    Object.assign(
      options.value,
      summaryRange(options.value.range, new Date(), props.weekStart),
    );
}
function savePreset() {
  const name = presetName.value.trim();
  if (!name || props.presets.length >= 100) return;
  // JSON copies Vue's reactive proxy into a serializable value.
  emit("save-presets", [
    ...props.presets,
    {
      id: crypto.randomUUID(),
      name,
      options: JSON.parse(JSON.stringify(options.value)),
    },
  ]);
  presetName.value = "";
  savePresetOpen.value = false;
}
function removePreset() {
  if (!selectedPreset.value) return;
  emit(
    "save-presets",
    props.presets.filter((item) => item.id !== selectedPreset.value),
  );
  selectedPreset.value = "";
}
const exportedContent = () =>
  format.value === "markdown"
    ? preview.value
    : serializeNotes(preview.value, "markdown", "text");
async function transfer(action: "copy" | "save") {
  if (pending.value || dateError.value) return;
  error.value = "";
  status.value = "";
  pending.value = true;
  const finish = beginEditorWrite();
  let failure: unknown;
  try {
    const content = exportedContent();
    if (action === "copy") {
      if (isNative) await invoke("copy_text", { text: content });
      else await navigator.clipboard.writeText(content);
      status.value = "Copied";
    } else {
      const name = `Task Summary ${options.value.from} to ${options.value.to}`;
      if (isNative) {
        if (
          await invoke<boolean>("export_text", {
            name,
            content,
            format: format.value,
          })
        )
          status.value = "Saved";
      } else {
        const url = URL.createObjectURL(
          new Blob([content], {
            type:
              format.value === "markdown"
                ? "text/markdown;charset=utf-8"
                : "text/plain;charset=utf-8",
          }),
        );
        const link = document.createElement("a");
        link.href = url;
        link.download = `${name}.${format.value === "markdown" ? "md" : "txt"}`;
        link.click();
        setTimeout(() => URL.revokeObjectURL(url), 5000);
        status.value = "Saved";
      }
    }
  } catch (cause) {
    error.value = `Could not ${action === "copy" ? "copy" : "save"} the summary. ${String(cause)}`;
    failure = error.value;
  } finally {
    pending.value = false;
    finish(failure);
  }
}
function createNote() {
  if (dateError.value) return;
  emit("create-note", {
    title: `Summary · ${options.value.from} – ${options.value.to}`,
    notes: preview.value,
    notes_format: "markdown",
    is_note: true,
  });
}
</script>

<template>
  <section class="task-summary" aria-label="Task summary">
    <header class="summary-header" data-tauri-drag-region="deep">
      <h1 v-if="!embedded">Summary</h1>
      <div class="summary-template-controls" data-tauri-drag-region="false">
        <select
          v-model="selectedPreset"
          aria-label="Summary template"
          @change="applyPreset"
        >
          <option value="">Templates</option>
          <option v-for="preset in presets" :key="preset.id" :value="preset.id">
            {{ preset.name }}
          </option>
        </select>
        <button
          v-if="selectedPreset"
          class="icon-button"
          aria-label="Delete summary template"
          @click="removePreset"
        >
          <PhTrash :size="17" />
        </button>
        <button
          class="icon-button"
          aria-label="Save summary template"
          :aria-expanded="savePresetOpen"
          :disabled="presets.length >= 100"
          @click="savePresetOpen = !savePresetOpen"
        >
          <PhPlus :size="18" />
        </button>
      </div>
    </header>
    <div class="summary-content">
      <form
        v-if="savePresetOpen"
        class="summary-template-form"
        @submit.prevent="savePreset"
      >
        <label
          >Template name<input
            v-model="presetName"
            autofocus
            maxlength="100"
            placeholder="Weekly report"
            aria-label="Summary template name"
        /></label>
        <button
          class="primary-button"
          :disabled="!presetName.trim() || !!dateError"
        >
          Save Template
        </button>
        <button type="button" @click="savePresetOpen = false">Cancel</button>
      </form>
      <div class="summary-dates">
        <label
          >Time range<select
            :value="options.range"
            aria-label="Summary time range"
            @change="setRange"
          >
            <option value="this_week">This week</option>
            <option value="last_week">Last week</option>
            <option value="this_month">This month</option>
            <option value="custom">Custom</option>
          </select></label
        >
        <label
          >From<input
            v-model="options.from"
            type="date"
            aria-label="Summary start date"
            @input="options.range = 'custom'"
        /></label>
        <label
          >To<input
            v-model="options.to"
            type="date"
            aria-label="Summary end date"
            @input="options.range = 'custom'"
        /></label>
        <label
          >Date to use<select
            v-model="options.basis"
            aria-label="Summary date basis"
          >
            <option value="completed">Completion date</option>
            <option value="due">Due date</option>
            <option value="created">Creation date</option>
          </select></label
        >
      </div>
      <p v-if="dateError" class="summary-error" role="alert">{{ dateError }}</p>
      <details class="summary-options">
        <summary>Filters &amp; display options</summary>
        <div class="summary-filter-grid">
          <label
            >Lists<select
              v-model="options.list_ids"
              multiple
              size="4"
              aria-label="Summary lists"
            >
              <option v-for="list in lists" :key="list.id" :value="list.id">
                {{ list.name }}{{ list.archived_at ? " (archived)" : "" }}
              </option></select
            ><small>No selection includes all lists.</small></label
          >
          <label
            >Tags<select
              v-model="options.tag_ids"
              multiple
              size="4"
              aria-label="Summary tags"
            >
              <option v-for="tag in tags" :key="tag.id" :value="tag.id">
                {{ tag.name }}
              </option></select
            ><small>No selection includes all tags.</small></label
          >
          <fieldset>
            <legend>Status</legend>
            <label
              v-for="item in [
                { value: 'active', label: 'Active' },
                { value: 'completed', label: 'Completed' },
                { value: 'wont_do', label: 'Won’t Do' },
              ]"
              :key="item.value"
              ><input
                v-model="options.statuses"
                type="checkbox"
                :value="item.value"
              />{{ item.label }}</label
            ><small>No selection includes all statuses.</small>
          </fieldset>
          <fieldset>
            <legend>Priority</legend>
            <label
              v-for="item in [
                { value: 3, label: 'High' },
                { value: 2, label: 'Medium' },
                { value: 1, label: 'Low' },
                { value: 0, label: 'None' },
              ]"
              :key="item.value"
              ><input
                v-model="options.priorities"
                type="checkbox"
                :value="item.value"
              />{{ item.label }}</label
            >
          </fieldset>
        </div>
        <div class="summary-display">
          <label
            >Group by<select
              v-model="options.group_by"
              aria-label="Summary group by"
            >
              <option value="list">List</option>
              <option value="status">Status</option>
              <option value="none">None</option>
            </select></label
          >
          <label
            >Sort by<select
              v-model="options.sort_by"
              aria-label="Summary sort by"
            >
              <option value="date">Date</option>
              <option value="title">Title</option>
              <option value="priority">Priority</option>
            </select></label
          >
          <label class="summary-checkbox"
            ><input v-model="options.include_subtasks" type="checkbox" />Include
            subtasks</label
          >
        </div>
        <fieldset class="summary-fields">
          <legend>Fields to show</legend>
          <label v-for="field in fieldChoices" :key="field.value"
            ><input
              v-model="options.fields"
              type="checkbox"
              :value="field.value"
            />{{ field.label }}</label
          >
        </fieldset>
        <button
          class="summary-reset"
          @click="
            options.list_ids = [];
            options.tag_ids = [];
            options.statuses = [];
            options.priorities = [];
          "
        >
          Clear filters
        </button>
      </details>
      <div class="summary-preview-heading">
        <div class="summary-tabs" role="tablist" aria-label="Summary view">
          <button
            role="tab"
            :aria-selected="tab === 'preview'"
            @click="tab = 'preview'"
          >
            Text preview</button
          ><button
            role="tab"
            :aria-selected="tab === 'tasks'"
            @click="tab = 'tasks'"
          >
            Tasks
          </button>
        </div>
        <span aria-live="polite"
          >{{ count }} {{ count === 1 ? "task" : "tasks"
          }}{{ options.include_subtasks ? ", including subtasks" : "" }}</span
        >
      </div>
      <p class="summary-hint">
        {{
          options.basis === "completed"
            ? "Includes tasks completed within the selected dates."
            : options.basis === "due"
              ? "Includes tasks due within the selected dates."
              : "Includes tasks created within the selected dates."
        }}
        Each matching task and subtask is counted once.
      </p>
      <div v-if="sourceChanged" class="summary-edits" role="status">
        <span
          >Your edited preview is preserved. Filters or tasks have
          changed.</span
        ><button @click="regenerate">Regenerate preview</button>
      </div>
      <textarea
        v-if="tab === 'preview'"
        v-model="preview"
        class="summary-preview"
        aria-label="Summary text preview"
        spellcheck="false"
        @input="edited = true"
      />
      <div v-else class="summary-matches" aria-label="Summary matching tasks">
        <p v-if="!count" class="summary-empty">No tasks match these filters.</p>
        <section v-for="group in groups" :key="group.id">
          <h2>
            {{ group.label }} <small>{{ group.entries.length }}</small>
          </h2>
          <button
            v-for="entry in group.entries"
            :key="entry.task.id"
            class="summary-match"
            :aria-label="`Open summary task ${entry.task.title}`"
            @click="emit('open-task', entry.task.id)"
          >
            <span :class="{ completed: !!entry.task.completed_at }"
              >{{ entry.task.title
              }}<small v-if="entry.parentPath">{{
                entry.parentPath
              }}</small></span
            ><time>{{ entry.date }}</time>
          </button>
        </section>
      </div>
    </div>
    <footer class="summary-footer">
      <span v-if="error" class="summary-error" role="alert">{{ error }}</span
      ><span v-else class="summary-status" role="status">{{ status }}</span>
      <select v-model="format" aria-label="Summary export format">
        <option value="markdown">Markdown</option>
        <option value="text">Plain text</option>
      </select>
      <button :disabled="pending || !!dateError" @click="transfer('copy')">
        <PhCopy :size="17" />Copy
      </button>
      <button :disabled="pending || !!dateError" @click="transfer('save')">
        <PhDownloadSimple :size="17" />Save As
      </button>
      <button :disabled="pending || !!dateError" @click="createNote">
        <PhNote :size="17" />Create Note
      </button>
    </footer>
  </section>
</template>

<style scoped>
.task-summary {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--surface);
  color: var(--text);
}
.summary-header {
  min-height: 57px;
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 4px 24px 0;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.summary-header h1 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  letter-spacing: -0.4px;
  flex: 1;
}
.summary-template-controls,
.summary-template-form {
  display: flex;
  align-items: center;
  gap: 8px;
}
.summary-template-controls {
  margin-left: auto;
}
.summary-content {
  overflow: auto;
  flex: 1;
  min-height: 0;
  padding: 22px 26px;
}
.summary-dates {
  display: grid;
  grid-template-columns: repeat(4, minmax(100px, 1fr));
  gap: 12px;
}
.task-summary label {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: var(--secondary);
}
.task-summary select,
.task-summary input:not([type="checkbox"]) {
  background: var(--popover);
  color: var(--text);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 7px 9px;
  font-size: 13px;
  min-width: 0;
}
.task-summary select:focus-visible,
.task-summary input:focus-visible,
.summary-preview:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.summary-options {
  margin-top: 18px;
  border-bottom: 1px solid var(--border);
  padding: 0 0 14px;
}
.summary-options summary {
  font-size: 13px;
  cursor: pointer;
  color: var(--secondary);
}
.summary-filter-grid {
  display: grid;
  grid-template-columns: 1.3fr 1.3fr 1fr 1fr;
  gap: 18px;
  margin-top: 16px;
}
.task-summary fieldset {
  border: 0;
  padding: 0;
  margin: 0;
  min-width: 0;
}
.task-summary legend {
  font-size: 12px;
  color: var(--secondary);
  margin-bottom: 8px;
}
.task-summary fieldset label,
.summary-display .summary-checkbox {
  flex-direction: row;
  align-items: center;
  color: var(--text);
  margin-bottom: 7px;
}
.task-summary input[type="checkbox"] {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
  margin: 0 3px 0 0;
  flex-shrink: 0;
}
.task-summary small,
.summary-hint {
  font-size: 11px;
  color: var(--secondary);
  line-height: 1.5;
}
.summary-display {
  display: flex;
  align-items: end;
  gap: 18px;
  margin-top: 18px;
  flex-wrap: wrap;
}
.summary-fields {
  display: flex;
  gap: 9px 18px;
  flex-wrap: wrap;
  margin-top: 18px !important;
}
.summary-fields legend {
  float: left;
  width: 100%;
}
.summary-reset {
  font-size: 12px;
  color: var(--accent);
  padding: 3px 0;
}
.summary-preview-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 18px;
}
.summary-preview-heading > span {
  font-size: 11px;
  color: var(--secondary);
  text-align: right;
}
.summary-tabs {
  display: flex;
  padding: 2px;
  border-radius: 6px;
  background: var(--input);
}
.summary-tabs button {
  padding: 5px 10px;
  font-size: 12px;
  border-radius: 4px;
}
.summary-tabs button[aria-selected="true"] {
  background: var(--popover);
  color: var(--text);
  box-shadow: 0 1px 3px #0001;
}
.summary-preview {
  width: 100%;
  min-height: 330px;
  resize: vertical;
  padding: 18px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--popover);
  color: var(--text);
  font:
    13px/1.7 -apple-system,
    BlinkMacSystemFont,
    sans-serif;
}
.summary-hint {
  margin: 10px 0 14px;
}
.summary-edits {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: space-between;
  font-size: 12px;
  padding: 10px 12px;
  margin-bottom: 10px;
  background: var(--input);
  border-radius: 6px;
}
.summary-edits button {
  color: var(--accent);
}
.summary-matches h2 {
  margin: 16px 0 7px;
  font-size: 14px;
  font-weight: 600;
}
.summary-matches h2 small {
  padding-left: 6px;
  font-weight: 400;
}
.summary-match {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 16px;
  padding: 10px 8px;
  border-bottom: 1px solid var(--border);
  border-radius: 4px;
  text-align: left;
  font-size: 14px;
}
.summary-match:hover {
  background: var(--hover);
}
.summary-match > span {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
}
.summary-match small {
  display: block;
  text-decoration: none;
}
.summary-match .completed {
  color: var(--secondary);
}
.summary-match time {
  font-size: 11px;
  color: var(--secondary);
  flex-shrink: 0;
}
.summary-empty {
  text-align: center;
  color: var(--secondary);
  padding: 60px 12px;
  font-size: 14px;
}
.summary-footer {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  padding: 12px 24px;
  border-top: 1px solid var(--border);
  flex-shrink: 0;
}
.summary-footer button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  font-size: 12px;
  padding: 7px 10px;
  border-radius: 5px;
}
.summary-footer button:hover:enabled {
  background: var(--hover);
}
.summary-status,
.summary-footer .summary-error {
  flex: 1;
  font-size: 12px;
}
.summary-error {
  color: var(--danger);
  font-size: 12px;
}
.summary-template-form {
  margin-bottom: 18px;
  align-items: end;
  flex-wrap: wrap;
}
.summary-template-form label {
  flex: 1;
  min-width: 160px;
}
.summary-template-form button {
  padding: 8px 12px;
  border-radius: 5px;
  font-size: 12px;
}
@media (max-width: 1000px) {
  .summary-filter-grid {
    grid-template-columns: 1fr 1fr;
  }
  .summary-dates {
    grid-template-columns: 1fr 1fr;
  }
}
@media (max-width: 600px) {
  .summary-header {
    padding: 12px;
    flex-wrap: wrap;
  }
  .summary-content {
    padding: 14px 12px;
  }
  .summary-dates {
    gap: 10px;
  }
  .summary-footer {
    padding: 10px 12px;
    gap: 4px;
  }
  .summary-footer .summary-status,
  .summary-footer .summary-error {
    flex-basis: 100%;
  }
  .summary-footer button {
    min-height: 40px;
  }
  .summary-template-controls select {
    max-width: 145px;
  }
  .summary-preview {
    padding: 12px;
    min-height: 280px;
  }
  .summary-preview-heading {
    align-items: flex-start;
  }
}
</style>
