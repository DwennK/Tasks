<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  PhArrowSquareOut,
  PhArrowsClockwise,
  PhCloud,
  PhLinkBreak,
} from "@phosphor-icons/vue";
import { dispatch, isNative } from "../adapter";
import { structuredConflictPreview } from "../lib/cloud-conflict-preview";

type Provider = "google_drive" | "dropbox";
interface CloudStatus {
  provider: Provider | null;
  configured: boolean;
  connected: boolean;
  busy: boolean;
  autoSync: boolean;
  lastSyncAt: string | null;
  lastError: string | null;
  redirectUri: string;
  clientId: string;
  hasClientSecret: boolean;
  supported: boolean;
  disabledReason?: string | null;
  availableProviders?: Partial<Record<Provider, boolean>>;
}
interface SyncCoreStatus {
  device_id: string;
  pending_conflicts: number;
}
interface ConflictVersion {
  id: string;
  device_id: string;
  clock: number;
  value: unknown;
  truncated: boolean;
}
interface SyncConflict {
  kind: string;
  id: string;
  field: string;
  label: string;
  winner_id: string;
  versions: ConflictVersion[];
}
const providerNames: Record<Provider, string> = {
  google_drive: "Google Drive",
  dropbox: "Dropbox",
};
const state = ref<CloudStatus | null>(null);
const provider = ref<Provider>("google_drive");
const clientId = ref("");
const clientSecret = ref("");
const clearClientSecret = ref(false);
const autoSync = ref(false);
const pending = ref("");
const error = ref("");
const notice = ref("");
const confirmation = ref<"configure" | "disconnect" | null>(null);
const setupOpen = ref(false);
const advancedOpen = ref(false);
const coreStatus = ref<SyncCoreStatus | null>(null);
const conflicts = ref<SyncConflict[]>([]);
const conflictError = ref("");
const resolving = ref("");
const loadingConflicts = ref(false);
const builtInSwitch = ref(false);
let unlisten: UnlistenFn | undefined;
let disposed = false;
const available = computed(() => isNative && state.value?.supported === true);
const offeredProviders = computed(() =>
  (Object.keys(providerNames) as Provider[]).filter(
    (item) => state.value?.availableProviders?.[item],
  ),
);
const busy = computed(() => !!pending.value || !!state.value?.busy);
const cancelAvailable = computed(
  () =>
    !!state.value?.busy ||
    pending.value === "connect" ||
    pending.value === "run",
);
const dirty = computed(
  () =>
    provider.value !== (state.value?.provider || "google_drive") ||
    clientId.value.trim() !== (state.value?.clientId || "") ||
    autoSync.value !== !!state.value?.autoSync ||
    !!clientSecret.value ||
    clearClientSecret.value,
);
const credentialsChanged = computed(
  () =>
    provider.value !== state.value?.provider ||
    clientId.value.trim() !== state.value?.clientId,
);
const currentProviderName = computed(() =>
  state.value?.provider ? providerNames[state.value.provider] : "cloud storage",
);
const lastSync = computed(() => {
  if (!state.value?.lastSyncAt) return "Not synchronized yet";
  const date = new Date(state.value.lastSyncAt);
  return Number.isNaN(date.getTime())
    ? "Not synchronized yet"
    : date.toLocaleString();
});
const statusText = computed(() => {
  if (!isNative) return "Desktop app required";
  if (!state.value)
    return error.value ? "Status unavailable" : "Checking status…";
  if (!state.value.supported)
    return (
      state.value.disabledReason || "Cloud sync is unavailable in this session."
    );
  if (state.value.busy || pending.value === "connect")
    return pending.value === "connect"
      ? "Waiting for browser sign-in…"
      : "Working…";
  if (state.value.connected) return `Connected to ${currentProviderName.value}`;
  if (state.value.configured) return "Ready to connect";
  return "Not configured";
});

function displayError(cause: unknown): string {
  let message = cause instanceof Error ? cause.message : String(cause);
  if (clientSecret.value)
    message = message.split(clientSecret.value).join("[redacted]");
  return message
    .replace(
      /\b(access_token|refresh_token|client_secret|authorization_code)=([^\s&]+)/gi,
      "$1=[redacted]",
    )
    .replace(/Bearer\s+[^\s]+/gi, "Bearer [redacted]");
}
function acceptStatus(next: CloudStatus, resetDraft = false) {
  const preserveDraft = dirty.value && !!state.value && !resetDraft;
  state.value = next;
  if (!preserveDraft) {
    provider.value = next.provider || "google_drive";
    clientId.value = next.clientId;
    autoSync.value = next.autoSync;
    clientSecret.value = "";
    clearClientSecret.value = false;
  }
}
async function refresh() {
  if (!isNative) return;
  try {
    const next = await invoke<CloudStatus>("cloud_sync_status");
    if (disposed) return;
    acceptStatus(next);
    await refreshConflicts();
  } catch (cause) {
    error.value = displayError(cause);
  }
}
async function action(kind: "configure" | "connect" | "disconnect" | "run") {
  if (!available.value || busy.value) return;
  pending.value = kind;
  error.value = "";
  notice.value = "";
  confirmation.value = null;
  try {
    const next = await invoke<CloudStatus>(
      `cloud_sync_${kind}`,
      kind === "configure"
        ? {
            provider: provider.value,
            clientId: clientId.value.trim(),
            clientSecret:
              provider.value === "google_drive" && clientSecret.value
                ? clientSecret.value
                : undefined,
            clearClientSecret:
              provider.value === "google_drive" && clearClientSecret.value,
            autoSync: autoSync.value,
          }
        : undefined,
    );
    if (disposed) return;
    acceptStatus(next, kind === "configure");
    if (kind === "configure") builtInSwitch.value = false;
    if (kind === "configure")
      notice.value = next.connected
        ? "Configuration saved."
        : "Configuration saved. Connect to authorize your account.";
    else if (kind === "disconnect")
      notice.value = "Disconnected. Your local and cloud data are kept.";
    else if (kind === "run" && !next.lastError)
      notice.value = "Synchronization finished.";
    else if (kind === "connect" && next.connected)
      notice.value = "Account connected.";
  } catch (cause) {
    if (!disposed) error.value = displayError(cause);
  } finally {
    if (!disposed) {
      pending.value = "";
      await refresh();
    }
  }
}
function saveConfiguration() {
  if (
    !clientId.value.trim() &&
    !state.value?.availableProviders?.[provider.value]
  ) {
    error.value =
      provider.value === "google_drive"
        ? "Enter the Google Desktop client ID."
        : "Enter the Dropbox app key.";
    return;
  }
  if (state.value?.connected && credentialsChanged.value)
    confirmation.value = "configure";
  else void action("configure");
}
async function cancel() {
  try {
    acceptStatus(await invoke<CloudStatus>("cloud_sync_cancel"));
    notice.value =
      "Cancellation requested. Any change already applied is kept.";
  } catch (cause) {
    error.value = displayError(cause);
  }
}
function chooseBuiltInProvider(next: Provider) {
  if (busy.value || state.value?.provider === next) return;
  provider.value = next;
  clientId.value = "";
  clientSecret.value = "";
  clearClientSecret.value = false;
  builtInSwitch.value = true;
  if (state.value?.connected) confirmation.value = "configure";
  else void action("configure");
}
function cancelConfirmation() {
  confirmation.value = null;
  if (builtInSwitch.value && state.value) acceptStatus(state.value, true);
  builtInSwitch.value = false;
}
async function openLink(url: string) {
  try {
    if (isNative) await invoke("open_external_link", { url });
    else window.open(url, "_blank", "noopener,noreferrer");
  } catch (cause) {
    error.value = displayError(cause);
  }
}
const conflictKey = (conflict: SyncConflict) =>
  `${conflict.kind}:${conflict.id}:${conflict.field}`;
function conflictField(field: string) {
  const names: Record<string, string> = {
    $exists: "Deletion",
    title: "Title",
    name: "Name",
    notes: "Description",
    due_date: "Due date",
    due_time: "Due time",
    start_date: "Start date",
    completed_at: "Completion",
    wont_do_at: "Won’t Do",
    list_id: "List",
    tag_ids: "Tags",
    checklist: "Checklist",
    progress: "Progress",
    priority: "Priority",
    details_json: "Task details",
    preferences_json: "Display preferences",
    task_json: "Template content",
    value: "Saved preferences",
    content: "Attachment contents",
  };
  return names[field] || field.replace(/_/g, " ");
}
function conflictValue(conflict: SyncConflict, version: ConflictVersion) {
  if (conflict.field === "$exists")
    return version.value === false ? "Delete this item" : "Keep this item";
  if (version.value === null || version.value === "") return "Not set";
  if (conflict.kind === "attachments" && conflict.field === "content")
    return "Attachment file data from this device";
  const text =
    structuredConflictPreview(
      conflict.field,
      version.value,
      conflict.kind,
      conflict.id,
    ) ??
    (typeof version.value === "string"
      ? version.value
      : JSON.stringify(version.value, null, 2));
  return text.length > 800 ? `${text.slice(0, 800)}…` : text;
}
async function refreshConflicts() {
  if (!available.value || disposed || loadingConflicts.value) return;
  loadingConflicts.value = true;
  try {
    const [status, result] = await Promise.all([
      dispatch<SyncCoreStatus>("sync.status"),
      dispatch<{ items: SyncConflict[] }>("sync.conflicts"),
    ]);
    if (!disposed) {
      coreStatus.value = status;
      conflicts.value = result.items;
      conflictError.value = "";
    }
  } catch (cause) {
    if (!disposed) conflictError.value = displayError(cause);
  } finally {
    if (!disposed) loadingConflicts.value = false;
  }
}
async function resolveConflict(
  conflict: SyncConflict,
  version: ConflictVersion,
) {
  if (busy.value || resolving.value) return;
  resolving.value = conflictKey(conflict);
  conflictError.value = "";
  try {
    await dispatch("sync.resolve", {
      kind: conflict.kind,
      id: conflict.id,
      field: conflict.field,
      version_id: version.id,
    });
    if (!disposed) {
      await refreshConflicts();
      notice.value =
        "Version selected. Synchronize to send this choice to your other Macs.";
    }
  } catch (cause) {
    if (!disposed) conflictError.value = displayError(cause);
  } finally {
    if (!disposed) resolving.value = "";
  }
}
onMounted(async () => {
  if (!isNative) return;
  try {
    const dispose = await listen<CloudStatus>("cloud-sync-status", (event) => {
      if (!disposed) {
        acceptStatus(event.payload);
        if (!event.payload.busy) void refreshConflicts();
      }
    });
    if (disposed) {
      dispose();
      return;
    }
    unlisten = dispose;
  } catch (cause) {
    if (!disposed) error.value = displayError(cause);
  }
  if (!disposed) await refresh();
});
onBeforeUnmount(() => {
  disposed = true;
  unlisten?.();
  clientSecret.value = "";
});
</script>

<template>
  <section class="cloud-sync" aria-label="Cloud synchronization">
    <h2>Cloud Sync</h2>
    <p class="sync-intro">
      Keep your personal Tasks data in your Google Drive or Dropbox account and
      synchronize it between Macs. Use the same provider, account and app
      configuration on each Mac.
    </p>
    <div class="sync-status-card" aria-live="polite">
      <PhCloud :size="26" />
      <div>
        <strong>{{ statusText }}</strong
        ><span>{{ lastSync }}</span>
      </div>
      <button
        v-if="isNative"
        class="sync-icon"
        aria-label="Refresh cloud sync status"
        :disabled="!!pending"
        @click="refresh"
      >
        <PhArrowsClockwise :size="17" />
      </button>
    </div>
    <p v-if="!isNative" class="sync-help">
      Open Tasks on your Mac to configure and connect cloud synchronization.
      Browser previews cannot access your account or Keychain.
    </p>
    <div
      v-if="available && offeredProviders.length > 1"
      class="sync-provider-choices"
      role="group"
      aria-label="Account provider"
    >
      <button
        v-for="item in offeredProviders"
        :key="item"
        :aria-pressed="state?.provider === item"
        :disabled="busy"
        @click="chooseBuiltInProvider(item)"
      >
        {{ providerNames[item] }}
      </button>
    </div>
    <p
      v-if="available && !state?.configured"
      class="sync-help sync-setup-needed"
    >
      Cloud sync has not been configured for this installation. Use the Tasks
      app configuration supplied by its publisher, or
      <button
        @click="
          advancedOpen = true;
          setupOpen = true;
        "
      >
        open Advanced setup
      </button>
      to configure your own.
    </p>
    <details
      class="sync-advanced"
      :open="advancedOpen"
      @toggle="advancedOpen = ($event.target as HTMLDetailsElement).open"
    >
      <summary>
        Advanced app configuration<span v-if="dirty"> · Unsaved changes</span>
      </summary>
      <form @submit.prevent="saveConfiguration">
        <fieldset :disabled="!available || busy" class="sync-configuration">
          <label
            >Provider<select
              v-model="provider"
              aria-label="Cloud sync provider"
              @change="
                confirmation = null;
                clientSecret = '';
                clearClientSecret = false;
              "
            >
              <option value="google_drive">Google Drive</option>
              <option value="dropbox">Dropbox</option>
            </select></label
          >
          <label
            >{{ provider === "google_drive" ? "Desktop client ID" : "App key"
            }}<input
              v-model="clientId"
              :aria-label="
                provider === 'google_drive'
                  ? 'Google Desktop client ID'
                  : 'Dropbox app key'
              "
              :placeholder="
                provider === 'google_drive'
                  ? '…apps.googleusercontent.com'
                  : 'Dropbox app key'
              "
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              maxlength="512"
          /></label>
          <template v-if="provider === 'google_drive'">
            <label
              >Client secret
              <span class="optional"
                >(if supplied with your Desktop client)</span
              ><input
                v-model="clientSecret"
                type="password"
                aria-label="Google client secret"
                :placeholder="
                  state?.hasClientSecret && state?.provider === provider
                    ? 'Saved in Keychain — leave blank to keep'
                    : 'Optional client secret'
                "
                autocomplete="new-password"
                spellcheck="false"
                maxlength="1024"
                @input="clearClientSecret = false"
            /></label>
            <label
              v-if="state?.hasClientSecret && state?.provider === provider"
              class="sync-check"
              ><input
                v-model="clearClientSecret"
                type="checkbox"
                @change="clientSecret = ''"
              />Remove the saved client secret</label
            >
          </template>
          <label class="sync-check"
            ><input v-model="autoSync" type="checkbox" />Synchronize
            automatically while Tasks is running</label
          >
          <p class="sync-help">
            Sync local changes automatically and periodically check for updates.
            You can also synchronize manually.
          </p>
          <div class="sync-actions">
            <button
              class="sync-primary"
              :disabled="
                !dirty ||
                (!clientId.trim() && !state?.availableProviders?.[provider])
              "
            >
              Save Configuration</button
            ><span v-if="dirty" class="sync-help">Unsaved configuration</span>
          </div>
        </fieldset>
      </form>
      <details
        :open="setupOpen"
        class="sync-setup"
        @toggle="setupOpen = ($event.target as HTMLDetailsElement).open"
      >
        <summary>Set up {{ providerNames[provider] }}</summary>
        <template v-if="provider === 'google_drive'">
          <ol>
            <li>
              In Google Cloud, create or select a project and enable the Google
              Drive API.
            </li>
            <li>
              Configure its OAuth consent screen. Add your account as a test
              user if the project is in testing.
            </li>
            <li>
              Create an OAuth client with the <strong>Desktop app</strong> type.
              Copy its client ID and, when provided, client secret into the
              fields above.
            </li>
            <li>
              Save the configuration, then connect. Your browser opens Google’s
              sign-in and permission screen.
            </li>
          </ol>
          <p class="sync-help">
            Tasks requests access to its own hidden application data folder
            (<code>drive.appdata</code>).
          </p>
          <div class="sync-links">
            <button
              @click="
                openLink(
                  'https://console.cloud.google.com/apis/library/drive.googleapis.com',
                )
              "
            >
              Enable Drive API <PhArrowSquareOut :size="13" /></button
            ><button
              @click="openLink('https://console.cloud.google.com/auth/clients')"
            >
              OAuth clients <PhArrowSquareOut :size="13" /></button
            ><button
              @click="
                openLink(
                  'https://developers.google.com/identity/protocols/oauth2/native-app',
                )
              "
            >
              Google setup guide <PhArrowSquareOut :size="13" />
            </button>
          </div>
        </template>
        <template v-else>
          <ol>
            <li>
              In the Dropbox App Console, create a scoped app with
              <strong>App folder</strong> access.
            </li>
            <li>
              Under Permissions, enable <code>files.metadata.read</code>,
              <code>files.content.read</code> and
              <code>files.content.write</code>.
            </li>
            <li>
              Add this exact OAuth redirect URI in the app’s settings:
              <code class="sync-redirect">{{
                state?.redirectUri || "http://127.0.0.1:49764/oauth/callback"
              }}</code>
            </li>
            <li>
              Copy its app key above, save the configuration, then connect
              through your browser.
            </li>
          </ol>
          <div class="sync-links">
            <button
              @click="openLink('https://www.dropbox.com/developers/apps')"
            >
              Dropbox App Console <PhArrowSquareOut :size="13" /></button
            ><button
              @click="openLink('https://developers.dropbox.com/oauth-guide')"
            >
              Dropbox setup guide <PhArrowSquareOut :size="13" />
            </button>
          </div>
        </template>
      </details>
    </details>
    <div v-if="confirmation" class="sync-confirmation" role="alert">
      <strong>{{
        confirmation === "disconnect"
          ? `Disconnect ${currentProviderName}?`
          : "Change the connected configuration?"
      }}</strong>
      <p>
        {{
          confirmation === "disconnect"
            ? "This disconnects synchronization on this Mac. Your local tasks and the data already stored in your cloud account are kept."
            : `Saving this change disconnects ${currentProviderName} on this Mac. Your existing data is kept. Connect again with the new configuration before synchronizing.`
        }}
      </p>
      <div class="sync-actions">
        <button class="sync-primary" @click="action(confirmation)">
          {{
            confirmation === "disconnect" ? "Disconnect" : "Save and Disconnect"
          }}</button
        ><button @click="cancelConfirmation">Cancel</button>
      </div>
    </div>
    <div class="sync-actions sync-connection-actions">
      <button
        v-if="!state?.connected"
        class="sync-primary"
        :disabled="!available || busy || !state?.configured || dirty"
        @click="action('connect')"
      >
        Connect {{ providerNames[provider] }}
      </button>
      <template v-else
        ><button
          class="sync-primary"
          :disabled="busy || dirty"
          @click="action('run')"
        >
          <PhArrowsClockwise :size="16" />Synchronize Now</button
        ><button :disabled="busy" @click="confirmation = 'disconnect'">
          <PhLinkBreak :size="16" />Disconnect
        </button></template
      >
      <button v-if="cancelAvailable && available" @click="cancel">
        Cancel operation
      </button>
    </div>
    <p v-if="dirty && state?.configured" class="sync-help">
      Save your configuration changes before connecting or synchronizing.
    </p>
    <p v-if="error || state?.lastError" class="sync-error" role="alert">
      {{ error || displayError(state?.lastError) }}
    </p>
    <p v-else-if="notice" class="sync-notice" role="status">{{ notice }}</p>
    <section
      v-if="available"
      class="sync-conflicts"
      aria-label="Sync conflicts"
    >
      <div class="sync-conflicts-heading">
        <h3>
          Conflicting changes
          <span v-if="coreStatus?.pending_conflicts">{{
            coreStatus.pending_conflicts
          }}</span>
        </h3>
        <button
          :disabled="loadingConflicts || !!resolving"
          @click="refreshConflicts"
        >
          Refresh
        </button>
      </div>
      <p v-if="conflictError" class="sync-error" role="alert">
        {{ conflictError }}
      </p>
      <p v-else-if="!conflicts.length" class="sync-help">
        {{
          loadingConflicts
            ? "Checking for conflicting changes…"
            : "No conflicting changes to review."
        }}
      </p>
      <p v-else class="sync-help">
        These fields changed on more than one Mac. Review the saved versions and
        choose which value to keep.
      </p>
      <article
        v-for="conflict in conflicts"
        :key="conflictKey(conflict)"
        class="sync-conflict"
      >
        <h4>
          {{
            conflict.kind === "settings"
              ? conflict.id === "saved_filters"
                ? "Saved filters"
                : conflict.id === "summary_templates"
                  ? "Summary templates"
                  : "Preferences"
              : conflict.label || "Item"
          }}
          <span>{{ conflictField(conflict.field) }}</span>
        </h4>
        <div
          v-for="(version, index) in conflict.versions"
          :key="version.id"
          class="sync-conflict-version"
        >
          <div class="sync-version-heading">
            <strong>{{
              version.device_id === coreStatus?.device_id
                ? "This Mac"
                : `Other device · Version ${index + 1}`
            }}</strong
            ><span v-if="version.id === conflict.winner_id"
              >Currently applied</span
            >
          </div>
          <pre>{{ conflictValue(conflict, version) }}</pre>
          <p v-if="version.truncated" class="sync-help">
            Preview shortened. Choosing this version restores the full value.
          </p>
          <button
            :disabled="busy || !!resolving"
            @click="resolveConflict(conflict, version)"
          >
            {{
              resolving === conflictKey(conflict)
                ? "Applying…"
                : conflict.field === "$exists" && version.value === false
                  ? "Apply deletion"
                  : "Keep this version"
            }}
          </button>
        </div>
      </article>
    </section>
    <p class="sync-help sync-storage">
      Your local database remains available offline. Disconnecting keeps your
      data. Account credentials are handled by the macOS app; Tasks never asks
      you to paste an access token.
    </p>
  </section>
</template>

<style scoped>
.cloud-sync {
  min-width: 0;
  color: var(--text);
}
.cloud-sync h2 {
  margin: 0 0 18px;
  font-size: 20px;
  font-weight: 600;
}
.sync-intro {
  font-size: 13px;
  line-height: 1.6;
  color: var(--secondary);
  margin: 0 0 18px;
}
.sync-provider-choices {
  display: flex;
  gap: 8px;
  margin-bottom: 18px;
}
.sync-provider-choices button {
  padding: 9px 14px;
  border: 1px solid var(--border);
  border-radius: 6px;
  font-size: 13px;
}
.sync-provider-choices button[aria-pressed="true"] {
  color: var(--accent);
  border-color: var(--accent);
  background: var(--input);
}
.sync-advanced {
  margin: 20px 0;
}
.sync-advanced > summary {
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  padding: 10px 0;
}
.sync-advanced > summary span {
  font-size: 11px;
  color: var(--secondary);
}
.sync-advanced > form {
  padding-top: 12px;
}
.sync-setup-needed {
  margin-bottom: 12px;
}
.sync-setup-needed button {
  color: var(--accent);
  padding: 0;
  font-size: inherit;
}
.sync-status-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 15px;
  background: var(--input);
  border-radius: 9px;
  margin-bottom: 20px;
}
.sync-status-card > div {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 4px;
}
.sync-status-card strong {
  font-size: 14px;
  font-weight: 500;
  overflow-wrap: anywhere;
}
.sync-status-card span {
  font-size: 12px;
  color: var(--secondary);
}
.sync-configuration {
  border: 0;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}
.sync-configuration > label {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}
.sync-configuration input:not([type="checkbox"]),
.sync-configuration select {
  min-width: 0;
  width: 100%;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--popover);
  border: 1px solid var(--border);
  color: var(--text);
  font-size: 13px;
}
.sync-configuration input:focus-visible,
.sync-configuration select:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.sync-configuration .sync-check {
  flex-direction: row;
  align-items: flex-start;
  gap: 8px;
  line-height: 1.5;
}
.sync-check input {
  accent-color: var(--accent);
  margin-top: 3px;
  flex-shrink: 0;
}
.optional {
  color: var(--secondary);
  font-size: 11px;
}
.sync-help {
  font-size: 12px;
  line-height: 1.5;
  color: var(--secondary);
  margin: 0;
}
.sync-configuration > .sync-help {
  margin-top: -8px;
}
.sync-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
}
.sync-actions button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 12px;
  font-size: 12px;
  border-radius: 6px;
  background: var(--input);
  min-height: 34px;
}
.sync-actions .sync-primary {
  background: var(--accent);
  color: white;
}
.sync-icon {
  padding: 7px;
  border-radius: 5px;
}
.sync-icon:hover {
  background: var(--hover);
}
.sync-setup {
  margin: 22px 0;
  padding: 15px 0;
  border-block: 1px solid var(--border);
}
.sync-setup summary {
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
}
.sync-setup ol {
  padding-left: 20px;
  margin: 15px 0;
  font-size: 12px;
  line-height: 1.7;
}
.sync-setup li {
  padding-left: 3px;
  margin-bottom: 8px;
}
.sync-setup code {
  font-size: 11px;
  overflow-wrap: anywhere;
}
.sync-redirect {
  display: block;
  margin-top: 6px;
  padding: 8px;
  background: var(--input);
  border-radius: 4px;
  user-select: text;
}
.sync-links {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 14px;
  margin-top: 12px;
}
.sync-links button {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--accent);
  font-size: 12px;
  padding: 3px 0;
}
.sync-confirmation {
  padding: 14px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--input);
  margin-bottom: 18px;
}
.sync-confirmation strong {
  font-size: 13px;
}
.sync-confirmation p {
  font-size: 12px;
  line-height: 1.6;
  margin: 8px 0 14px;
}
.sync-connection-actions {
  margin-bottom: 12px;
}
.sync-error {
  color: var(--danger);
  font-size: 12px;
  line-height: 1.6;
  overflow-wrap: anywhere;
}
.sync-notice {
  font-size: 12px;
  line-height: 1.6;
}
.sync-storage {
  margin-top: 22px;
  padding-top: 16px;
  border-top: 1px solid var(--border);
}
.sync-conflicts {
  margin-top: 22px;
  padding-top: 16px;
  border-top: 1px solid var(--border);
}
.sync-conflicts-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 12px;
}
.sync-conflicts h3 {
  font-size: 14px;
  font-weight: 500;
  margin: 0;
}
.sync-conflicts h3 span {
  font-size: 12px;
  color: var(--secondary);
  margin-left: 6px;
}
.sync-conflicts-heading > button {
  color: var(--accent);
  font-size: 12px;
}
.sync-conflict {
  margin-top: 16px;
}
.sync-conflict h4 {
  font-size: 13px;
  font-weight: 500;
  margin: 0 0 9px;
  overflow-wrap: anywhere;
}
.sync-conflict h4 span {
  display: block;
  font-size: 11px;
  color: var(--secondary);
  margin-top: 3px;
}
.sync-conflict-version {
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: 7px;
  margin: 6px 0;
}
.sync-version-heading {
  display: flex;
  align-items: center;
  gap: 8px;
  justify-content: space-between;
}
.sync-version-heading strong {
  font-size: 11px;
  font-weight: 500;
}
.sync-version-heading span {
  font-size: 10px;
  color: var(--secondary);
}
.sync-conflict-version pre {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-family: inherit;
  font-size: 12px;
  line-height: 1.5;
  margin: 9px 0;
  user-select: text;
}
.sync-conflict-version > button {
  margin-top: 6px;
  color: var(--accent);
  font-size: 12px;
  padding: 4px 0;
}
@media (max-width: 600px) {
  .sync-actions button {
    min-height: 40px;
  }
}
</style>
