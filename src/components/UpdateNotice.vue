<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isNative } from "../adapter";
const emit = defineEmits<{ open: [] }>();
const version = ref<string | null>(null);
const dismissed = ref<string | null>(null);
let disposed = false;
let unlisten: UnlistenFn | undefined;
function accept(status: { phase: string; version: string | null }) {
  if (!disposed)
    version.value = ["available", "ready"].includes(status.phase)
      ? status.version
      : null;
}
onMounted(async () => {
  if (!isNative) return;
  try {
    unlisten = await listen<{ phase: string; version: string | null }>(
      "tasks-update-status",
      (event) => accept(event.payload),
    );
    if (disposed) {
      unlisten();
      return;
    }
    accept(await invoke("update_status"));
  } catch {
    /* Automatic checks stay quiet when offline or unavailable. */
  }
});
onBeforeUnmount(() => {
  disposed = true;
  unlisten?.();
});
</script>
<template>
  <aside
    v-if="version && version !== dismissed"
    class="update-notice"
    aria-label="Update available"
  >
    <span role="status">Tasks {{ version }} is available</span>
    <button @click="emit('open')">View update</button>
    <button aria-label="Dismiss update notice" @click="dismissed = version">
      ×
    </button>
  </aside>
</template>
<style scoped>
.update-notice {
  position: fixed;
  bottom: 18px;
  right: 18px;
  z-index: 35;
  display: flex;
  align-items: center;
  gap: 12px;
  max-width: calc(100vw - 36px);
  padding: 12px 14px;
  border-radius: 10px;
  background: var(--surface);
  color: var(--text);
  border: 1px solid var(--border);
  box-shadow: 0 5px 25px #0002;
  font-size: 12px;
}
button {
  color: var(--accent);
  cursor: pointer;
  white-space: nowrap;
}
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 3px;
}
@media (max-width: 500px) {
  .update-notice {
    left: 12px;
    right: 12px;
    bottom: 12px;
    flex-wrap: wrap;
  }
}
</style>
