<script setup lang="ts">
import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { PhCopy, PhDownloadSimple } from "@phosphor-icons/vue";
import type { Task } from "../types";
import { isNative } from "../adapter";
import AppDialog from "./AppDialog.vue";
import { serializeTask, type ExportFormat } from "./taskExport";
import { beginEditorWrite } from "./editorWork";

const props = defineProps<{ task: Task; tasks?: Task[] }>();
const emit = defineEmits<{ close: [] }>();
const format = ref<ExportFormat>("markdown");
const pending = ref(false);
const error = ref("");
const status = ref("");
const content = computed(() =>
  serializeTask(props.task, props.tasks || [], format.value),
);
function selectFormat(value: ExportFormat) {
  format.value = value;
  status.value = "";
  error.value = "";
}
async function copy() {
  pending.value = true;
  error.value = "";
  status.value = "";
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    if (isNative) await invoke("copy_text", { text: content.value });
    else await navigator.clipboard.writeText(content.value);
    status.value = "Copied";
  } catch (cause) {
    error.value = `Could not copy the task. ${String(cause)}`;
    failure = error.value;
  } finally {
    pending.value = false;
    finishWork(failure);
  }
}
async function download() {
  pending.value = true;
  error.value = "";
  status.value = "";
  const finishWork = beginEditorWrite();
  let failure: unknown;
  const name =
    props.task.title.replace(/[\x00-\x1f/\\:*?"<>|]/g, "-").trim() || "Task";
  try {
    if (isNative) {
      const saved = await invoke<boolean>("export_text", {
        name,
        content: content.value,
        format: format.value,
      });
      if (saved) status.value = "Downloaded";
    } else {
      const url = URL.createObjectURL(
        new Blob([content.value], {
          type: `${format.value === "markdown" ? "text/markdown" : "text/plain"};charset=utf-8`,
        }),
      );
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `${name}.${format.value === "markdown" ? "md" : "txt"}`;
      anchor.click();
      setTimeout(() => URL.revokeObjectURL(url), 5000);
      status.value = "Downloaded";
    }
  } catch (cause) {
    error.value = `Could not export the task. ${String(cause)}`;
    failure = error.value;
  } finally {
    pending.value = false;
    finishWork(failure);
  }
}
</script>

<template>
  <AppDialog
    title="Export"
    panel-class="task-export-dialog"
    @close="emit('close')"
  >
    <div class="task-export">
      <div class="export-formats" role="tablist" aria-label="Export format">
        <button
          role="tab"
          :aria-selected="format === 'markdown'"
          :disabled="pending"
          @click="selectFormat('markdown')"
        >
          Markdown
        </button>
        <button
          role="tab"
          :aria-selected="format === 'text'"
          :disabled="pending"
          @click="selectFormat('text')"
        >
          Plain Text
        </button>
      </div>
      <textarea
        aria-label="Export preview"
        :value="content"
        readonly
        spellcheck="false"
      ></textarea>
      <footer>
        <span v-if="error" class="export-error" role="alert">{{ error }}</span>
        <span v-else class="export-status" role="status">{{ status }}</span>
        <button class="export-copy" :disabled="pending" @click="copy">
          <PhCopy :size="18" />Copy
        </button>
        <button
          class="primary-button export-download"
          :disabled="pending"
          @click="download"
        >
          <PhDownloadSimple :size="18" />Download
        </button>
      </footer>
    </div>
  </AppDialog>
</template>

<style scoped>
:global(.dialog.task-export-dialog) {
  width: 620px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 48px);
  padding: 18px 24px 16px;
}
:global(.task-export-dialog > header) {
  position: relative;
  justify-content: center;
  margin: 0 0 12px;
  min-height: 24px;
}
:global(.task-export-dialog > header h2) {
  font-size: 16px;
  font-weight: 600;
}
:global(.task-export-dialog > header .icon-button) {
  position: absolute;
  left: -16px;
  top: -10px;
  width: 23px;
  height: 23px;
  opacity: 0.65;
}
.task-export {
  display: flex;
  flex-direction: column;
  gap: 20px;
}
.export-formats {
  display: flex;
  align-self: center;
  padding: 2px;
  border-radius: 18px;
  background: #f2f2f2;
  height: 28px;
}
.export-formats button {
  padding: 3px 10px;
  color: var(--secondary);
  font-size: 13px;
  border-radius: 15px;
  min-width: 80px;
}
.export-formats button[aria-selected="true"] {
  background: var(--popover);
  color: var(--text);
  box-shadow: 0 0 0 1px var(--border);
}
.task-export textarea {
  resize: none;
  width: 100%;
  height: 379px;
  min-height: 160px;
  border: 0;
  border-radius: 10px;
  background: #f7f7f7;
  color: var(--text);
  padding: 15px 18px;
  line-height: 1.6;
  font-size: 14px;
  font-family: inherit;
  outline-offset: 2px;
}
.task-export footer {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 10px;
  min-height: 30px;
}
.task-export footer button {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  min-height: 30px;
  border-radius: 8px;
  padding: 4px 9px;
  font-size: 13px;
}
.export-copy {
  border: 1px solid var(--border);
  min-width: 88px;
}
.export-download {
  min-width: 88px;
}
.export-status,
.export-error {
  margin-right: auto;
  font-size: 11px;
  color: var(--secondary);
}
:global([data-theme="dark"]) .export-formats,
:global([data-theme="dark"]) .task-export textarea {
  background: var(--input);
}
.export-error {
  color: var(--danger, #d54b4b);
  max-width: 300px;
}
@media (max-height: 720px) {
  .task-export textarea {
    height: max(160px, calc(100vh - 240px));
  }
}
@media (max-width: 650px) {
  :global(.dialog.task-export-dialog) {
    padding: 18px 16px 16px;
  }
  .task-export footer {
    flex-wrap: wrap;
  }
  .export-error {
    flex-basis: 100%;
    max-width: none;
  }
}
</style>
