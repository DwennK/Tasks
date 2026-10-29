<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  PhArrowCircleDown,
  PhArrowsClockwise,
  PhCheckCircle,
} from "@phosphor-icons/vue";
import { isNative } from "../adapter";
import appPackage from "../../package.json";

interface UpdateStatus {
  currentVersion: string;
  phase:
    | "idle"
    | "checking"
    | "current"
    | "available"
    | "downloading"
    | "ready"
    | "saving"
    | "installing"
    | "error";
  version: string | null;
  notes: string | null;
  downloaded: number;
  total: number | null;
  error: string | null;
  checkedAt: string | null;
  supported: boolean;
  disabledReason: string | null;
  automatic: boolean;
}
const status = ref<UpdateStatus | null>(null);
const error = ref("");
const pending = ref(false);
let disposed = false;
let unlisten: UnlistenFn | undefined;
const busy = computed(
  () =>
    pending.value ||
    ["checking", "downloading", "saving", "installing"].includes(
      status.value?.phase || "",
    ),
);
const available = computed(() => isNative && status.value?.supported);
const progress = computed(() =>
  status.value?.total
    ? Math.min(
        100,
        Math.round((status.value.downloaded / status.value.total) * 100),
      )
    : null,
);
const headline = computed(() => {
  switch (status.value?.phase) {
    case "checking":
      return "Checking for updates…";
    case "current":
      return "You’re up to date";
    case "available":
      return `Tasks ${status.value.version} is available`;
    case "downloading":
      return "Downloading update…";
    case "ready":
      return "Ready to install";
    case "saving":
      return "Saving your changes…";
    case "installing":
      return "Installing and restarting…";
    case "error":
      return "Update interrupted";
    default:
      return "Keep Tasks up to date";
  }
});
async function action(command: string) {
  if (busy.value || !available.value) return;
  pending.value = true;
  error.value = "";
  try {
    const next = await invoke<UpdateStatus | null>(command);
    if (next && !disposed) status.value = next;
  } catch (reason) {
    if (!disposed) error.value = String(reason);
  } finally {
    pending.value = false;
  }
}
async function setAutomatic(event: Event) {
  const input = event.target as HTMLInputElement;
  const old = status.value?.automatic ?? true;
  const enabled = input.checked;
  input.checked = old;
  if (busy.value || !available.value) return;
  pending.value = true;
  error.value = "";
  try {
    status.value = await invoke<UpdateStatus>("update_set_automatic", {
      enabled,
    });
  } catch (reason) {
    error.value = String(reason);
  } finally {
    pending.value = false;
  }
}
onMounted(async () => {
  if (!isNative) return;
  try {
    unlisten = await listen<UpdateStatus>("tasks-update-status", (event) => {
      if (!disposed)
        status.value = {
          ...event.payload,
          automatic: status.value?.automatic ?? event.payload.automatic,
        };
    });
    if (disposed) {
      unlisten();
      return;
    }
    const next = await invoke<UpdateStatus>("update_status");
    if (!disposed) status.value = next;
  } catch (reason) {
    if (!disposed) error.value = String(reason);
  }
});
onBeforeUnmount(() => {
  disposed = true;
  unlisten?.();
});
</script>

<template>
  <section class="updates" aria-label="Application updates">
    <h2>Updates</h2>
    <div class="update-card">
      <PhCheckCircle
        v-if="status?.phase === 'current'"
        :size="42"
        weight="duotone"
        aria-hidden="true"
      />
      <PhArrowCircleDown
        v-else
        :size="42"
        weight="duotone"
        aria-hidden="true"
      />
      <h3 role="status" aria-live="polite">{{ headline }}</h3>
      <p>
        Installed version {{ status?.currentVersion || appPackage.version }}
      </p>
      <p v-if="!isNative" class="update-help">
        Open the desktop app to check for and install updates.
      </p>
      <p v-else-if="status?.disabledReason" class="update-help">
        {{ status.disabledReason }}
      </p>
      <template v-if="available">
        <template v-if="status?.phase === 'downloading'">
          <progress
            aria-label="Update download"
            :value="progress ?? undefined"
            max="100"
          ></progress>
          <p>
            {{
              progress === null
                ? "Downloading and verifying…"
                : `${progress}% downloaded`
            }}
          </p>
        </template>
        <p v-if="status?.phase === 'ready'" class="update-help">
          The download is verified. Tasks will save your changes before
          restarting.
        </p>
        <div class="update-actions">
          <button
            v-if="status?.phase === 'available'"
            class="primary"
            :disabled="busy"
            @click="action('download_update')"
          >
            Download update
          </button>
          <button
            v-else-if="status?.phase === 'ready'"
            class="primary"
            :disabled="busy"
            @click="action('install_update')"
          >
            Install and restart
          </button>
          <button v-else :disabled="busy" @click="action('check_for_updates')">
            <PhArrowsClockwise :size="16" aria-hidden="true" /> Check for
            updates
          </button>
        </div>
        <p v-if="status?.checkedAt" class="update-help">
          Last checked {{ new Date(status.checkedAt).toLocaleString() }}
        </p>
      </template>
    </div>
    <p v-if="error || status?.error" class="update-error" role="alert">
      {{ error || status?.error }}
    </p>
    <label class="update-preference">
      <input
        type="checkbox"
        :checked="status?.automatic ?? true"
        :disabled="!available || busy"
        @change="setAutomatic"
      />
      <span>Automatically check for updates</span>
    </label>
    <p class="update-help">
      Check on launch and every six hours while Tasks is open. Downloads and
      restarts happen only when you choose.
    </p>
    <div v-if="status?.notes && status.version" class="release-notes">
      <h3>What’s new in {{ status.version }}</h3>
      <pre>{{ status.notes }}</pre>
    </div>
    <p class="update-help">
      Updates are delivered through GitHub Releases. Your tasks stay in their
      local database.
    </p>
  </section>
</template>

<style scoped>
.updates {
  color: var(--text);
}
h2 {
  font-size: 17px;
  margin: 0 0 20px;
}
.update-card {
  padding: 26px 20px;
  border: 1px solid var(--border);
  border-radius: 12px;
  text-align: center;
  background: var(--surface);
}
.update-card > svg {
  color: var(--accent);
  margin: 0 auto 8px;
}
h3 {
  font-size: 16px;
  font-weight: 600;
  margin: 8px 0;
}
p {
  font-size: 13px;
  line-height: 1.6;
  margin: 8px 0;
}
.update-help {
  color: var(--muted);
  font-size: 12px;
}
.update-actions {
  display: flex;
  justify-content: center;
  margin: 18px 0 8px;
}
button {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 9px 16px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--surface);
  color: var(--text);
  cursor: pointer;
}
button.primary {
  color: white;
  background: var(--accent);
  border-color: var(--accent);
}
button:disabled {
  cursor: default;
  opacity: 0.55;
}
button:focus-visible,
input:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 3px;
}
.update-preference {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 22px 0 6px;
  font-size: 13px;
}
input {
  accent-color: var(--accent);
}
progress {
  width: 100%;
  max-width: 320px;
  height: 8px;
  accent-color: var(--accent);
}
.release-notes {
  border-top: 1px solid var(--border);
  margin-top: 24px;
  padding-top: 12px;
}
pre {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font: inherit;
  font-size: 13px;
  line-height: 1.7;
}
.update-error {
  color: #d94757;
  overflow-wrap: anywhere;
}
</style>
