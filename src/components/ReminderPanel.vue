<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { PhBellRinging, PhCheck, PhClock } from "@phosphor-icons/vue";
import { dispatch, isNative } from "../adapter";
import type { Task } from "../types";

type Reminder = Task & { reminder_at: string; reminder_generation: string };
type Action = "done" | "dismiss" | "snooze";
const reminders = ref<Reminder[]>([]);
const loading = ref(true);
const error = ref("");
const notice = ref("");
const busy = ref<string | null>(null);
const durations = ref<Record<string, number>>({});
let unlisten: UnlistenFn | undefined;
let disposed = false;
const snoozeOptions = [
  { minutes: 5, label: "5 minutes" },
  { minutes: 10, label: "10 minutes" },
  { minutes: 30, label: "30 minutes" },
  { minutes: 60, label: "1 hour" },
  { minutes: 180, label: "3 hours" },
  { minutes: 1440, label: "1 day" },
];
function key(reminder: Reminder) {
  return `${reminder.id}:${reminder.reminder_at}:${reminder.reminder_generation}`;
}
function scheduleLabel(reminder: Reminder) {
  const date = new Date(reminder.reminder_at);
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}
function setPending(pending: Reminder[]) {
  if (disposed) return;
  reminders.value = pending;
  const current = new Set(pending.map(key));
  for (const id of Object.keys(durations.value)) {
    if (!current.has(id)) delete durations.value[id];
  }
}
async function refresh() {
  try {
    const pending = isNative
      ? await invoke<Reminder[]>("pending_task_reminders")
      : await dispatch<Reminder[]>("reminders.pending");
    setPending(pending);
    error.value = "";
  } catch (cause) {
    error.value = String(cause);
  } finally {
    loading.value = false;
  }
}
async function resolve(reminder: Reminder, action: Action) {
  if (busy.value) return;
  busy.value = key(reminder);
  error.value = "";
  notice.value = "";
  const input = {
    id: reminder.id,
    reminder_at: reminder.reminder_at,
    generation: reminder.reminder_generation,
    action,
    ...(action === "snooze"
      ? { minutes: durations.value[key(reminder)] || 5 }
      : {}),
  };
  try {
    let result: { ok: boolean; stale: boolean };
    if (isNative) {
      const response = await invoke<{
        result: { ok: boolean; stale: boolean };
        pending: Reminder[];
      }>("resolve_task_reminder", { input });
      result = response.result;
      setPending(response.pending);
    } else {
      result = await dispatch("reminders.resolve", input);
      await refresh();
    }
    if (result.stale)
      notice.value = "This reminder has changed. The list has been refreshed.";
  } catch (cause) {
    error.value = String(cause);
  } finally {
    busy.value = null;
  }
}
onMounted(async () => {
  if (isNative) {
    const stop = await listen<Reminder[]>("task-reminders-changed", (event) => {
      setPending(event.payload);
    });
    if (disposed) stop();
    else unlisten = stop;
  }
  if (!disposed) await refresh();
});
onBeforeUnmount(() => {
  disposed = true;
  unlisten?.();
});
</script>

<template>
  <main class="reminder-panel" aria-label="Task reminders">
    <header class="reminder-header">
      <PhBellRinging :size="20" />
      <h1>Reminders</h1>
      <span v-if="reminders.length" class="reminder-count">{{
        reminders.length
      }}</span>
    </header>
    <div v-if="error" class="reminder-error" role="alert">
      {{ error }}
      <button type="button" @click="refresh">Retry</button>
    </div>
    <p v-if="notice" class="reminder-notice" role="status">{{ notice }}</p>
    <div class="reminder-scroll">
      <p v-if="loading" class="reminder-empty" role="status">
        Loading reminders…
      </p>
      <div v-else-if="!reminders.length" class="reminder-empty" role="status">
        <PhCheck :size="32" />
        <p>No reminders pending</p>
      </div>
      <article
        v-for="reminder in reminders"
        :key="key(reminder)"
        class="reminder-item"
        :aria-label="reminder.title"
        :aria-busy="busy === key(reminder)"
      >
        <h2>{{ reminder.title }}</h2>
        <p class="reminder-time">
          <PhClock :size="14" /> {{ scheduleLabel(reminder) }}
        </p>
        <div class="reminder-actions">
          <div class="reminder-snooze">
            <select
              :value="durations[key(reminder)] || 5"
              :aria-label="`Snooze duration for ${reminder.title}`"
              :disabled="!!busy"
              @change="
                durations[key(reminder)] = Number(
                  ($event.target as HTMLSelectElement).value,
                )
              "
            >
              <option
                v-for="option in snoozeOptions"
                :key="option.minutes"
                :value="option.minutes"
              >
                {{ option.label }}
              </option>
            </select>
            <button
              type="button"
              :disabled="!!busy"
              @click="resolve(reminder, 'snooze')"
            >
              Snooze
            </button>
          </div>
          <button
            class="reminder-dismiss"
            type="button"
            :disabled="!!busy"
            @click="resolve(reminder, 'dismiss')"
          >
            Dismiss
          </button>
          <button
            class="reminder-done"
            type="button"
            :disabled="!!busy"
            @click="resolve(reminder, 'done')"
          >
            <PhCheck :size="15" /> Done
          </button>
        </div>
      </article>
    </div>
  </main>
</template>

<style scoped>
.reminder-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--popover);
  color: var(--text);
  font-size: 13px;
}
.reminder-header {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 18px 20px 15px;
  border-bottom: 1px solid var(--border);
}
.reminder-header > svg {
  color: var(--accent);
}
.reminder-header h1 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}
.reminder-count {
  margin-left: auto;
  color: var(--secondary);
  font-variant-numeric: tabular-nums;
}
.reminder-scroll {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}
.reminder-item {
  padding: 19px 20px;
  border-bottom: 1px solid var(--border);
}
.reminder-item h2 {
  margin: 0 0 8px;
  font-size: 15px;
  line-height: 1.4;
  font-weight: 500;
  overflow-wrap: anywhere;
}
.reminder-time {
  display: flex;
  align-items: center;
  gap: 5px;
  color: var(--secondary);
  margin: 0 0 16px;
  font-size: 12px;
}
.reminder-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.reminder-actions button {
  min-height: 29px;
  padding: 5px 9px;
  border-radius: 5px;
  font-size: 12px;
}
.reminder-snooze {
  display: flex;
  align-items: center;
  border: 1px solid var(--border);
  border-radius: 5px;
  margin-right: auto;
}
.reminder-snooze select {
  max-width: 110px;
  padding: 5px 3px 5px 7px;
  color: var(--text);
  background: var(--popover);
  border: 0;
  font-size: 12px;
  border-radius: 5px;
}
.reminder-snooze select:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.reminder-snooze button {
  border-left: 1px solid var(--border);
  color: var(--accent);
  border-radius: 0 5px 5px 0;
}
.reminder-dismiss {
  color: var(--secondary);
}
.reminder-actions button:hover {
  background: var(--hover);
}
.reminder-actions .reminder-done {
  display: flex;
  align-items: center;
  gap: 4px;
  background: var(--accent);
  color: white;
}
.reminder-actions .reminder-done:hover {
  filter: brightness(0.95);
}
.reminder-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 210px;
  padding: 24px;
  color: var(--secondary);
}
.reminder-empty > svg {
  color: var(--muted);
}
.reminder-empty p {
  margin: 0;
}
.reminder-error,
.reminder-notice {
  margin: 10px 20px 0;
  padding: 10px;
  border-radius: 5px;
  background: var(--input);
  font-size: 12px;
  overflow-wrap: anywhere;
}
.reminder-error {
  color: var(--danger);
}
.reminder-error button {
  color: var(--accent);
  text-decoration: underline;
  margin-left: 4px;
}
.reminder-notice {
  color: var(--secondary);
}
</style>
