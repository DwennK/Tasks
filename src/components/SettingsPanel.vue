<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeMount,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import {
  PhBellRinging,
  PhCaretUpDown,
  PhCaretRight,
  PhCheck,
  PhCheckSquare,
  PhClock,
  PhCloud,
  PhDotsThree,
  PhFunnel,
  PhInfo,
  PhKeyboard,
  PhList,
  PhPaintRoller,
  PhQuestion,
  PhSliders,
  PhSquare,
  PhTag,
  PhTrash,
  PhX,
  PhXSquare,
} from "@phosphor-icons/vue";
import type { TaskList, TaskTemplate } from "../types";
import { defaultHotkeys, keyChord, type HotkeyAction } from "../lib/task-input";
import { registerEscapeLayer } from "../lib/escape-layers";
import TaskNavigationGlyph from "./TaskNavigationGlyph.vue";
import CloudSyncSettings from "./CloudSyncSettings.vue";
import UpdateSettings from "./UpdateSettings.vue";
import { getVersion } from "@tauri-apps/api/app";
import { isNative } from "../adapter";
import appPackage from "../../package.json";

type Section =
  | "smart"
  | "notifications"
  | "date"
  | "appearance"
  | "more"
  | "hotkeys"
  | "sync"
  | "updates"
  | "about";
type RecordedAction = HotkeyAction | "global_quick_add" | "global_show_hide";
const props = withDefaults(
  defineProps<{
    settings: Record<string, unknown>;
    lists: TaskList[];
    templates?: TaskTemplate[];
    standalone?: boolean;
    tags?: { id: string; name: string }[];
    initialSection?: Section;
    version?: string;
  }>(),
  {
    tags: () => [],
    templates: () => [],
    standalone: false,
    initialSection: "smart",
    version: appPackage.version,
  },
);
const emit = defineEmits<{
  "update-setting": [key: string, value: unknown];
  close: [];
  "notification-settings": [];
  "template-action": [
    action: "apply" | "rename" | "delete",
    id: string,
    name?: string,
  ];
}>();
const displayedVersion = ref(props.version);
onMounted(async () => {
  if (isNative) {
    try {
      displayedVersion.value = await getVersion();
    } catch {
      /* Keep packaged version if native metadata is unavailable. */
    }
  }
});
const panel = ref<HTMLElement>();
const section = ref<Section>(props.initialSection);
watch(
  () => props.initialSection,
  (next) => changeSection(next),
);
const editingTemplate = ref<{ id: string; name: string } | null>(null);
const deletingTemplate = ref<string | null>(null);
function templateItems(template: TaskTemplate) {
  const data = template.task;
  const checks = Array.isArray(data.checklist) ? data.checklist : [];
  const children = Array.isArray(data.subtasks) ? data.subtasks : [];
  return [
    ...checks.map((item) => String(item?.text || "")),
    ...children.map((item) => String(item?.title || "")),
  ]
    .filter(Boolean)
    .slice(0, 5);
}
function templateNotes(template: TaskTemplate) {
  const notes = String(template.task.notes || "");
  return template.task.notes_format === "html"
    ? new DOMParser().parseFromString(notes, "text/html").body.textContent || ""
    : notes;
}
function renameTemplate() {
  const edit = editingTemplate.value;
  if (!edit || !edit.name.trim()) return;
  emit("template-action", "rename", edit.id, edit.name.trim());
  editingTemplate.value = null;
}
const appearanceTab = ref<"theme" | "icons" | "display">("theme");
const interfaceStyleOpen = ref(false);
const interfacePanel = ref<HTMLElement>();
const interfaceTrigger = ref<HTMLButtonElement>();
const hotkeyTab = ref<"hotkeys" | "shortcuts">("hotkeys");
const recording = ref<RecordedAction | null>(null);
const firstKey = ref("");
const recordingError = ref("");
let sequenceTimer: ReturnType<typeof setTimeout> | undefined;
let priorFocus: HTMLElement | null = null;
let unregisterEscapeLayer: (() => void) | undefined;
const sections = [
  { id: "smart" as Section, name: "Smart Lists", icon: PhFunnel },
  {
    id: "notifications" as Section,
    name: "Notifications",
    icon: PhBellRinging,
  },
  { id: "date" as Section, name: "Date & Time", icon: PhClock },
  { id: "appearance" as Section, name: "Appearance", icon: PhPaintRoller },
  { id: "more" as Section, name: "More", icon: PhSliders },
  { id: "sync" as Section, name: "Cloud Sync", icon: PhCloud },
  { id: "hotkeys" as Section, name: "Hotkeys", icon: PhKeyboard },
  { id: "updates" as Section, name: "Updates", icon: PhCloud },
  { id: "about" as Section, name: "About", icon: PhInfo },
];
const defaults: Record<string, unknown> = {
  theme: "light",
  theme_color: "default",
  interface_style: "card",
  app_icon: "default",
  show_dock_icon: true,
  dock_count: "today",
  launch_at_login: false,
  launch_minimized: false,
  global_quick_add_shortcut: "",
  global_show_hide_shortcut: "",
  font_size: "normal",
  sidebar_count: "all",
  completed_style: "strikethrough",
  show_list_color: false,
  notifications: false,
  completion_sound: true,
  week_start: 1,
  show_week_numbers: false,
  time_format: 24,
  show_timezone: false,
  date_recognition: "remove",
  tag_recognition: "remove",
  default_priority: 0,
  default_list_id: "inbox",
  default_tag_id: null,
  default_date: "none",
  default_reminder_timed: "none",
  default_reminder_allday: "none",
  default_add_to: "top",
  overdue_position: "top",
};
const smartGroups = [
  [
    { id: "all", name: "All", icon: TaskNavigationGlyph, glyph: "all" },
    { id: "today", name: "Today", icon: TaskNavigationGlyph, glyph: "today" },
    {
      id: "tomorrow",
      name: "Tomorrow",
      icon: TaskNavigationGlyph,
      glyph: "tomorrow",
    },
    {
      id: "upcoming",
      name: "Next 7 Days",
      icon: TaskNavigationGlyph,
      glyph: "upcoming",
    },
    { id: "inbox", name: "Inbox", icon: TaskNavigationGlyph, glyph: "inbox" },
  ],
  [
    { id: "tags", name: "Tags", icon: PhTag },
    {
      id: "filters",
      name: "Filters",
      icon: TaskNavigationGlyph,
      glyph: "filters",
    },
  ],
  [
    { id: "completed", name: "Completed", icon: PhCheckSquare },
    { id: "wont_do", name: "Won't Do", icon: PhXSquare },
    { id: "trash", name: "Trash", icon: PhTrash },
    { id: "summary", name: "Summary", icon: PhList },
  ],
];
const smartDefaults: Record<string, string> = {
  all: "hide",
  today: "show",
  tomorrow: "hide",
  upcoming: "show",
  inbox: "show",
  tags: "show",
  filters: "show",
  completed: "show",
  wont_do: "auto",
  trash: "show",
  summary: "auto",
};
const smartVisibility = computed(() => ({
  ...smartDefaults,
  ...((props.settings.smart_lists as Record<string, string>) || {}),
}));
const colors = [
  { id: "default", name: "Default", colors: ["#ffffff", "#ffffff"] },
  { id: "sky", name: "Sky", colors: ["#7a94fc", "#4870f2"] },
  { id: "turquoise", name: "Turquoise", colors: ["#8bd9c9", "#2cbaa2"] },
  { id: "teal", name: "Teal", colors: ["#9dd8db", "#5bb2bb"] },
  { id: "matcha", name: "Matcha", colors: ["#c8d2bd", "#9bab8b"] },
  { id: "sunshine", name: "Sunshine", colors: ["#ffda8a", "#f8b37e"] },
  { id: "peach", name: "Peach", colors: ["#f9b9b4", "#f882af"] },
  { id: "lilac", name: "Lilac", colors: ["#d7bfdb", "#999bd4"] },
  { id: "ebony", name: "Ebony", colors: ["#c6aea3", "#a58266"] },
  { id: "navy", name: "Navy", colors: ["#414f82", "#28304f"] },
  { id: "gray", name: "Gray", colors: ["#55575b", "#323541"] },
  { id: "dark", name: "Dark", colors: ["#242424", "#202020"] },
];
const fontChoices = [
  { id: "normal", label: "Normal", size: 14 },
  { id: "large", label: "Large", size: 16 },
  { id: "extra-large", label: "Extra-Large", size: 18 },
];
const countChoices = [
  { id: "all", label: "Show(All)", today: "3", inbox: "16" },
  { id: "tasks", label: "Show(Hide Note)", today: "3", inbox: "10" },
  { id: "hidden", label: "Hide(All)", today: "", inbox: "" },
];
const hotkeys = computed(() => ({
  ...defaultHotkeys,
  ...((props.settings.hotkeys as Record<HotkeyAction, string>) || {}),
  global_quick_add: value<string>("global_quick_add_shortcut"),
  global_show_hide: value<string>("global_show_hide_shortcut"),
}));
const hotkeyGroups: {
  name: string;
  description?: string;
  items: {
    id?: RecordedAction;
    name: string;
    chord?: string;
    readonly?: boolean;
  }[];
}[] = [
  {
    name: "General",
    items: [
      { id: "print_task", name: "Print" },
      { id: "search", name: "Open Command Menu" },
      { id: "shortcuts", name: "Shortcuts" },
      { id: "settings", name: "Settings" },
    ],
  },
  {
    name: "Global Action",
    description:
      "Shortcuts that can respond even when the app is in the background.",
    items: [
      { id: "global_quick_add", name: "Quick Add" },
      { id: "global_show_hide", name: "Show/Hide Main Window" },
    ],
  },
  {
    name: "Task",
    items: [
      { id: "quick_add", name: "Add task" },
      { name: "Add task below", chord: "enter" },
      { id: "add_subtask", name: "Add subtask", readonly: true },
      { id: "toggle_all_groups", name: "Toggle all groups" },
      { id: "toggle_all_subtasks", name: "Expand/Collapse all subtasks" },
    ],
  },
  {
    name: "Edit Tasks",
    description: "These shortcuts only work when your cursor is on the task.",
    items: [
      { id: "complete_task", name: "Complete tasks" },
      { id: "pin_task", name: "Pin tasks" },
      { id: "delete_task", name: "Delete tasks" },
      { id: "set_date", name: "Set date" },
      { id: "clear_date", name: "Clear date" },
      { id: "set_today", name: "Set today" },
      { id: "set_tomorrow", name: "Set tomorrow" },
      { id: "set_next_week", name: "Set next week" },
      { id: "priority_none", name: "Set no priority" },
      { id: "priority_low", name: "Set low priority" },
      { id: "priority_medium", name: "Set medium priority" },
      { id: "priority_high", name: "Set high priority" },
    ],
  },
  {
    name: "Navigation",
    items: [
      { id: "all", name: "Go to All" },
      { id: "today", name: "Go to Today" },
      { id: "tomorrow", name: "Go to Tomorrow" },
      { id: "upcoming", name: "Go to Next 7 Days" },
      { id: "inbox", name: "Go to Inbox" },
      { id: "completed", name: "Go to Completed" },
      { id: "wont_do", name: "Go to Won't Do" },
      { id: "trash", name: "Go to Trash" },
    ],
  },
  {
    name: "Switch Views",
    items: [
      { id: "list_view", name: "List View" },
      { id: "kanban_view", name: "Kanban View" },
    ],
  },
];
function value<T>(key: string): T {
  return (props.settings[key] ?? defaults[key]) as T;
}
function update(key: string, next: unknown) {
  emit("update-setting", key, next);
}
function updateConfirmedBoolean(
  key: "launch_at_login" | "launch_minimized" | "notifications",
  event: Event,
) {
  const input = event.target as HTMLInputElement;
  const next = input.checked;
  // Keep the persisted value visible until native permission/registration and
  // the settings write succeed, even when a rejected request changes no props.
  input.checked = value<boolean>(key);
  update(key, next);
}
const rememberedDateMode = ref<"remove" | "keep">(
  props.settings.date_recognition === "keep" ? "keep" : "remove",
);
watch(
  () => props.settings.date_recognition,
  (mode) => {
    if (mode === "remove" || mode === "keep") rememberedDateMode.value = mode;
  },
);
const dateRecognitionEnabled = computed(
  () => value<string>("date_recognition") !== "off",
);
const removeRecognizedDate = computed(() =>
  dateRecognitionEnabled.value
    ? value<string>("date_recognition") === "remove"
    : rememberedDateMode.value === "remove",
);
function toggleDateRecognition(event: Event) {
  const enabled = (event.target as HTMLInputElement).checked;
  if (!enabled) {
    const mode = value<string>("date_recognition");
    if (mode === "keep" || mode === "remove") rememberedDateMode.value = mode;
  }
  update("date_recognition", enabled ? rememberedDateMode.value : "off");
}
function toggleDateRemoval(event: Event) {
  if (!dateRecognitionEnabled.value) return;
  rememberedDateMode.value = (event.target as HTMLInputElement).checked
    ? "remove"
    : "keep";
  update("date_recognition", rememberedDateMode.value);
}
function selected(event: Event) {
  return (event.target as HTMLSelectElement).value;
}
function changeSection(id: Section) {
  section.value = id;
  interfaceStyleOpen.value = false;
  cancelRecording();
  nextTick(() =>
    panel.value?.querySelector(".settings-pane")?.scrollTo({ top: 0 }),
  );
}
function setSmart(id: string, mode: string) {
  update("smart_lists", { ...smartVisibility.value, [id]: mode });
}
function selectColor(id: string) {
  update("theme_color", id);
  if (id === "dark") update("theme", "dark");
  else if (value("theme") === "dark") update("theme", "light");
}
async function openInterfaceStyle() {
  interfaceStyleOpen.value = true;
  await nextTick();
  interfacePanel.value
    ?.querySelector<HTMLButtonElement>('[aria-checked="true"]')
    ?.focus();
}
function closeInterfaceStyle() {
  interfaceStyleOpen.value = false;
  nextTick(() => interfaceTrigger.value?.focus());
}
function selectInterfaceStyle(style: "flat" | "card") {
  update("interface_style", style);
}
function resetDefaults() {
  for (const key of [
    "date_recognition",
    "tag_recognition",
    "default_priority",
    "default_list_id",
    "default_tag_id",
    "default_date",
    "default_reminder_timed",
    "default_reminder_allday",
    "default_add_to",
    "overdue_position",
  ])
    update(key, defaults[key]);
}
function displayHotkey(chord: string) {
  if (!chord) return "Not set";
  if (chord.includes(" "))
    return chord
      .split(" ")
      .map((k) => k.toUpperCase())
      .join(" then ");
  return chord
    .replace("mod+", "⌘")
    .replace("ctrl+", "⌃")
    .replace("alt+", "⌥")
    .replace("shift+", "⇧")
    .replace("enter", "↩")
    .replace("backspace", "⌫")
    .replace("delete", "⌦")
    .replace("escape", "⎋")
    .replace("space", "Space")
    .toUpperCase();
}
function startRecording(action: RecordedAction) {
  cancelRecording();
  recording.value = action;
  recordingError.value = "";
}
function cancelRecording() {
  recording.value = null;
  firstKey.value = "";
  if (sequenceTimer) clearTimeout(sequenceTimer);
}
function globalSettingKey(action: RecordedAction) {
  if (action === "global_quick_add") return "global_quick_add_shortcut";
  if (action === "global_show_hide") return "global_show_hide_shortcut";
  return null;
}
function saveHotkey(action: RecordedAction, chord: string) {
  if (
    [
      "mod+q",
      "mod+w",
      "mod+h",
      "mod+m",
      "mod+tab",
      "mod+space",
      "mod+ctrl+q",
    ].includes(chord)
  ) {
    recordingError.value =
      "This shortcut is reserved by macOS. Choose another combination.";
    firstKey.value = "";
    return;
  }
  if (
    globalSettingKey(action) &&
    chord &&
    !/^(?:(?:mod|ctrl|alt)\+)(?:(?:ctrl|alt|shift)\+)*(?:[a-z0-9]|f(?:[1-9]|1[0-9]|2[0-4])|space|enter|tab|backspace|delete|escape|arrowup|arrowdown|arrowleft|arrowright|home|end|pageup|pagedown)$/.test(
      chord,
    )
  ) {
    recordingError.value =
      "Use Command, Control or Option with a key. Global shortcuts cannot use sequences.";
    firstKey.value = "";
    return;
  }
  const duplicate = Object.entries(hotkeys.value).find(
    ([id, key]) => id !== action && chord && key === chord,
  );
  if (duplicate) {
    recordingError.value =
      "This shortcut is already in use. Choose another combination.";
    firstKey.value = "";
    return;
  }
  const globalKey = globalSettingKey(action);
  if (globalKey) update(globalKey, chord);
  else
    update("hotkeys", {
      ...((props.settings.hotkeys as Record<string, string>) || {}),
      [action]: chord,
    });
  cancelRecording();
}
function handleKey(event: KeyboardEvent) {
  if (event.defaultPrevented || event.isComposing || event.keyCode === 229)
    return;
  if (interfaceStyleOpen.value) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopImmediatePropagation();
      closeInterfaceStyle();
      return;
    }
    if (
      ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
    ) {
      event.preventDefault();
      event.stopImmediatePropagation();
      const style =
        value<string>("interface_style") === "card" ? "flat" : "card";
      selectInterfaceStyle(style);
      nextTick(() =>
        interfacePanel.value
          ?.querySelector<HTMLButtonElement>(`[data-style="${style}"]`)
          ?.focus(),
      );
      return;
    }
  }
  if (recording.value) {
    event.preventDefault();
    event.stopImmediatePropagation();
    if (event.key === "Escape") {
      cancelRecording();
      return;
    }
    const global = !!globalSettingKey(recording.value);
    const chord = keyChord(event, { physical: global });
    if (!chord) return;
    if (global || chord.includes("+") || chord.length !== 1 || chord === "?") {
      saveHotkey(recording.value, chord);
      return;
    }
    if (firstKey.value) {
      saveHotkey(recording.value, `${firstKey.value} ${chord}`);
      return;
    }
    firstKey.value = chord;
    sequenceTimer = setTimeout(() => {
      if (recording.value && firstKey.value)
        saveHotkey(recording.value, firstKey.value);
    }, 900);
    return;
  }
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopImmediatePropagation();
    emit("close");
    return;
  }
  if (event.key !== "Tab") return;
  const focusable = (
    interfaceStyleOpen.value ? interfacePanel.value : panel.value
  )?.querySelectorAll<HTMLElement>(
    'button:not(:disabled):not([tabindex="-1"]), select, input, [tabindex="0"]',
  );
  if (!focusable?.length) return;
  const first = focusable[0],
    last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last?.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first?.focus();
  }
}
function handleNonEscapeKey(event: KeyboardEvent) {
  // Escape belongs to the current modal layer, including any dialog above
  // Settings. Other keys still use capture for the shortcut recorder.
  if (event.key !== "Escape") handleKey(event);
}
onBeforeMount(() => {
  unregisterEscapeLayer = registerEscapeLayer(handleKey);
});
onMounted(async () => {
  priorFocus = document.activeElement as HTMLElement;
  document.addEventListener("keydown", handleNonEscapeKey, true);
  await nextTick();
  panel.value?.focus();
});
onBeforeUnmount(() => {
  unregisterEscapeLayer?.();
  document.removeEventListener("keydown", handleNonEscapeKey, true);
  cancelRecording();
  priorFocus?.focus();
});
</script>

<template>
  <Teleport to="body" :disabled="standalone"
    ><div
      class="settings-backdrop"
      :class="{ standalone }"
      @mousedown.self="!standalone && emit('close')"
    >
      <section
        ref="panel"
        class="settings-window"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        tabindex="-1"
      >
        <div
          v-if="standalone"
          class="native-settings-drag"
          data-tauri-drag-region
        ></div>
        <aside class="settings-sidebar" :inert="interfaceStyleOpen">
          <button
            v-if="!standalone"
            class="settings-close"
            aria-label="Close settings"
            @click="emit('close')"
          >
            <PhX :size="9" weight="bold" />
          </button>
          <h1>Settings</h1>
          <nav aria-label="Settings categories">
            <button
              v-for="item in sections"
              :key="item.id"
              :class="{
                active: section === item.id,
                'separate-group': ['hotkeys', 'about'].includes(item.id),
              }"
              @click="changeSection(item.id)"
              :aria-current="section === item.id ? 'page' : undefined"
            >
              <component :is="item.icon" :size="18" /><span>{{
                item.name
              }}</span>
            </button>
          </nav>
        </aside>
        <div
          class="settings-pane"
          :inert="interfaceStyleOpen"
          :class="{
            'appearance-pane': section === 'appearance',
            'display-pane':
              section === 'appearance' && appearanceTab === 'display',
            'hotkeys-pane': section === 'hotkeys',
          }"
        >
          <template v-if="section === 'smart'"
            ><h2>Smart List</h2>
            <div
              v-for="(group, index) in smartGroups"
              :key="index"
              class="settings-group"
            >
              <label
                v-for="item in group"
                :key="item.id"
                class="preference-row smart-row"
                ><component
                  :is="item.icon"
                  :size="19"
                  :kind="item.glyph" /><span>{{ item.name }}</span
                ><span class="select-wrap"
                  ><select
                    :aria-label="`${item.name} visibility`"
                    :value="smartVisibility[item.id]"
                    @change="setSmart(item.id, selected($event))"
                  >
                    <option value="show">Show</option>
                    <option value="hide">Hidden</option>
                    <option value="auto">Show if not empty</option></select
                  ><PhCaretUpDown :size="10" /></span
              ></label></div
          ></template>
          <CloudSyncSettings v-else-if="section === 'sync'" />
          <UpdateSettings v-else-if="section === 'updates'" />
          <template v-else-if="section === 'notifications'"
            ><h2>Notifications</h2>
            <div class="settings-group">
              <div class="preference-row">
                <span>Notification</span
                ><button
                  class="settings-link"
                  @click="emit('notification-settings')"
                >
                  Go to System Preferences
                </button>
              </div>
              <label class="preference-row"
                ><span>Task Reminders</span
                ><input
                  class="native-switch"
                  type="checkbox"
                  role="switch"
                  aria-label="Task reminders"
                  :checked="value<boolean>('notifications')"
                  @change="
                    updateConfirmedBoolean('notifications', $event)
                  " /></label
              ><label class="preference-row"
                ><span>Completion Sound</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Completion sound"
                    :value="
                      value<boolean>('completion_sound') ? 'jingle' : 'none'
                    "
                    @change="
                      update('completion_sound', selected($event) === 'jingle')
                    "
                  >
                    <option value="jingle">Jingle</option>
                    <option value="none">None</option></select
                  ><PhCaretUpDown :size="10" /></span
              ></label>
            </div>
            <p class="preference-help">
              Task reminders are delivered by macOS while Tasks is running.
              Closing the window keeps reminders active; quitting stops them.
            </p></template
          >
          <template v-else-if="section === 'date'"
            ><h2>Date &amp; Time</h2>
            <div class="settings-group">
              <label class="preference-row"
                ><span>Start Week on</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Start week on"
                    :value="value<number>('week_start')"
                    @change="update('week_start', Number(selected($event)))"
                  >
                    <option :value="1">Monday</option>
                    <option :value="0">Sunday</option>
                    <option :value="6">Saturday</option></select
                  ><PhCaretUpDown :size="10" /></span
              ></label>
            </div>
            <div class="settings-group">
              <label class="preference-row"
                ><span>Show Week Numbers(W)</span
                ><input
                  class="native-switch"
                  type="checkbox"
                  role="switch"
                  aria-label="Show week numbers"
                  :checked="value<boolean>('show_week_numbers')"
                  @change="
                    update(
                      'show_week_numbers',
                      ($event.target as HTMLInputElement).checked,
                    )
                  " /></label
              ><label class="preference-row"
                ><span>Time Format</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Time format"
                    :value="String(value<string>('time_format'))"
                    @change="update('time_format', Number(selected($event)))"
                  >
                    <option value="24">24-hour</option>
                    <option value="12">12-hour</option></select
                  ><PhCaretUpDown :size="10" /></span
              ></label>
            </div>
            <div class="settings-group">
              <label class="preference-row"
                ><span>Time Zone</span
                ><input
                  class="native-switch"
                  type="checkbox"
                  role="switch"
                  aria-label="Time zone"
                  :checked="value<boolean>('show_timezone')"
                  @change="
                    update(
                      'show_timezone',
                      ($event.target as HTMLInputElement).checked,
                    )
                  "
              /></label>
              <p class="group-description">
                If enabled, you can select the time zone when setting time for
                tasks.
              </p>
            </div></template
          >
          <template v-else-if="section === 'appearance'"
            ><div
              class="settings-segments"
              role="tablist"
              aria-label="Appearance"
            >
              <button
                :class="{ selected: appearanceTab === 'theme' }"
                role="tab"
                :aria-selected="appearanceTab === 'theme'"
                @click="appearanceTab = 'theme'"
              >
                Theme</button
              ><button
                :class="{ selected: appearanceTab === 'icons' }"
                role="tab"
                :aria-selected="appearanceTab === 'icons'"
                @click="appearanceTab = 'icons'"
              >
                App icons</button
              ><button
                :class="{ selected: appearanceTab === 'display' }"
                role="tab"
                :aria-selected="appearanceTab === 'display'"
                @click="appearanceTab = 'display'"
              >
                Display
              </button>
            </div>
            <template v-if="appearanceTab === 'theme'"
              ><div class="settings-group">
                <button
                  ref="interfaceTrigger"
                  class="preference-row interface-style-trigger"
                  aria-haspopup="dialog"
                  aria-label="Interface Style"
                  @click="openInterfaceStyle"
                >
                  <span>Interface Style</span
                  ><span class="interface-style-value"
                    >{{ value("interface_style") === "flat" ? "Flat" : "Card"
                    }}<PhCaretRight :size="13"
                  /></span></button
                ><label class="preference-row"
                  ><span>Follow System Dark Mode</span
                  ><span class="select-wrap"
                    ><select
                      aria-label="Follow system dark mode"
                      :value="
                        value<string>('theme') === 'system'
                          ? 'enabled'
                          : 'disabled'
                      "
                      @change="
                        update(
                          'theme',
                          selected($event) === 'enabled'
                            ? 'system'
                            : value<string>('theme_color') === 'dark'
                              ? 'dark'
                              : 'light',
                        )
                      "
                    >
                      <option value="enabled">Enabled</option>
                      <option value="disabled">Disabled</option></select
                    ><PhCaretUpDown :size="10" /></span
                ></label>
              </div>
              <h3 class="choice-heading">Color Series</h3>
              <div class="color-series">
                <button
                  v-for="color in colors"
                  :key="color.id"
                  class="theme-choice"
                  :aria-label="`${color.name} theme`"
                  :aria-pressed="value<string>('theme_color') === color.id"
                  @click="selectColor(color.id)"
                >
                  <span
                    class="color-swatch"
                    :class="{ 'default-swatch': color.id === 'default' }"
                    :style="{
                      background: `linear-gradient(${color.colors[0]}, ${color.colors[1]})`,
                    }"
                    ><PhDotsThree
                      v-if="color.id === 'dark'"
                      class="dark-swatch-dots"
                      :size="32"
                      weight="bold" /><span
                      v-if="value<string>('theme_color') === color.id"
                      class="choice-check"
                      ><PhCheck :size="13" weight="bold" /></span></span
                  ><span>{{ color.name }}</span>
                </button>
              </div></template
            >
            <template v-else-if="appearanceTab === 'icons'">
              <div class="app-icon-choices" aria-label="App icon">
                <button
                  v-for="icon in ['default', 'rounded']"
                  :key="icon"
                  class="app-icon-choice"
                  :aria-label="`${icon === 'default' ? 'Default' : 'Rounded'} app icon`"
                  :aria-pressed="value<string>('app_icon') === icon"
                  @click="update('app_icon', icon)"
                >
                  <img
                    :src="`/icons/app-${icon}.png`"
                    alt=""
                    draggable="false"
                  />
                  <span
                    v-if="value<string>('app_icon') === icon"
                    class="choice-check"
                    ><PhCheck :size="13" weight="bold"
                  /></span>
                </button>
              </div>
              <div class="settings-group dock-settings">
                <label class="preference-row"
                  ><span>Show app icon in Dock</span
                  ><input
                    class="native-switch"
                    type="checkbox"
                    role="switch"
                    aria-label="Show app icon in Dock"
                    :checked="value<boolean>('show_dock_icon')"
                    @change="
                      update(
                        'show_dock_icon',
                        ($event.target as HTMLInputElement).checked,
                      )
                    "
                /></label>
                <label class="preference-row"
                  ><span>Choose task to be counted in Dock</span
                  ><span class="select-wrap"
                    ><select
                      aria-label="Choose task to be counted in Dock"
                      :value="value<string>('dock_count')"
                      @change="update('dock_count', selected($event))"
                    >
                      <option value="none">None</option>
                      <option value="today">Today</option>
                      <option value="inbox">Inbox</option>
                      <option value="all">All</option>
                      <option value="overdue">Overdue</option></select
                    ><PhCaretUpDown :size="10" /></span
                ></label>
              </div>
            </template>
            <template v-else
              ><section class="display-section">
                <h3>Font Size</h3>
                <div class="display-choices">
                  <button
                    v-for="font in fontChoices"
                    :key="font.id"
                    class="display-choice"
                    :aria-pressed="value<string>('font_size') === font.id"
                    @click="update('font_size', font.id)"
                  >
                    <span
                      class="preview-card font-preview"
                      :style="{ fontSize: `${font.size}px` }"
                      >Font Size<span
                        v-if="value<string>('font_size') === font.id"
                        class="choice-check"
                        ><PhCheck :size="13" weight="bold" /></span></span
                    ><span>{{ font.label }}</span>
                  </button>
                </div>
              </section>
              <section class="display-section">
                <h3>Sidebar Count</h3>
                <div class="display-choices">
                  <button
                    v-for="count in countChoices"
                    :key="count.id"
                    class="display-choice"
                    :aria-pressed="value<string>('sidebar_count') === count.id"
                    @click="update('sidebar_count', count.id)"
                  >
                    <span class="preview-card count-preview"
                      ><span
                        ><TaskNavigationGlyph
                          kind="today"
                          :size="14"
                        />Today<small>{{ count.today }}</small></span
                      ><span
                        ><TaskNavigationGlyph
                          kind="inbox"
                          :size="14"
                        />Inbox<small>{{ count.inbox }}</small></span
                      ><span
                        v-if="value<string>('sidebar_count') === count.id"
                        class="choice-check"
                        ><PhCheck :size="13" weight="bold" /></span></span
                    ><span>{{ count.label }}</span>
                  </button>
                </div>
              </section>
              <section class="display-section">
                <h3>
                  List Color
                  <span
                    class="list-color-help"
                    tabindex="0"
                    aria-label="About List Color"
                    aria-describedby="list-color-description"
                    ><PhQuestion :size="15" /><span
                      id="list-color-description"
                      role="tooltip"
                      >Colors task rows using their list color.</span
                    ></span
                  >
                </h3>
                <div class="display-choices">
                  <button
                    v-for="show in [true, false]"
                    :key="String(show)"
                    class="display-choice"
                    :aria-pressed="value<boolean>('show_list_color') === show"
                    @click="update('show_list_color', show)"
                  >
                    <span
                      class="preview-card task-preview"
                      :class="{ 'with-list-color': show }"
                      ><span><PhSquare :size="16" />Task Title</span
                      ><span><PhSquare :size="16" />Task Title</span
                      ><span
                        v-if="value<boolean>('show_list_color') === show"
                        class="choice-check"
                        ><PhCheck :size="13" weight="bold" /></span></span
                    ><span>{{ show ? "Show" : "Hide" }}</span>
                  </button>
                </div>
              </section>
              <section class="display-section">
                <h3>Completed Task Style</h3>
                <div class="display-choices">
                  <button
                    v-for="style in ['default', 'strikethrough']"
                    :key="style"
                    class="display-choice"
                    :aria-pressed="value<string>('completed_style') === style"
                    @click="update('completed_style', style)"
                  >
                    <span
                      class="preview-card task-preview completed-preview"
                      :class="{ 'strike-preview': style === 'strikethrough' }"
                      ><span
                        ><PhCheckSquare :size="16" weight="fill" />Task
                        Title</span
                      ><span
                        ><PhCheckSquare :size="16" weight="fill" />Task
                        Title</span
                      ><span
                        v-if="value<string>('completed_style') === style"
                        class="choice-check"
                        ><PhCheck :size="13" weight="bold" /></span></span
                    ><span>{{
                      style === "default" ? "Default" : "Strikethrough"
                    }}</span>
                  </button>
                </div>
              </section></template
            >
          </template>
          <template v-else-if="section === 'more'"
            ><h2>More</h2>
            <div class="settings-group">
              <label class="preference-row">
                <span>Launch at Login</span>
                <input
                  class="native-switch"
                  type="checkbox"
                  role="switch"
                  aria-label="Launch at Login"
                  :checked="value<boolean>('launch_at_login')"
                  @change="updateConfirmedBoolean('launch_at_login', $event)"
                />
              </label>
              <label class="preference-row">
                <span>Minimize app when auto launching</span>
                <input
                  class="native-switch"
                  type="checkbox"
                  role="switch"
                  aria-label="Minimize app when auto launching"
                  :checked="value<boolean>('launch_minimized')"
                  @change="updateConfirmedBoolean('launch_minimized', $event)"
                />
              </label>
            </div>
            <h3 class="group-heading">Smart Recognition</h3>
            <div class="settings-group recognition-group">
              <section class="recognition-setting">
                <label class="preference-row"
                  ><span>Date Recognition</span
                  ><input
                    class="native-switch"
                    type="checkbox"
                    role="switch"
                    aria-label="Date recognition"
                    :checked="dateRecognitionEnabled"
                    @change="toggleDateRecognition"
                /></label>
                <p class="group-description">
                  When adding tasks, recognize date and time information and
                  automatically set reminders.
                </p>
                <label
                  class="recognition-option"
                  :class="{ disabled: !dateRecognitionEnabled }"
                  ><input
                    type="checkbox"
                    aria-label="Remove text in tasks"
                    :checked="removeRecognizedDate"
                    :disabled="!dateRecognitionEnabled"
                    @change="toggleDateRemoval"
                  /><span>Remove text in tasks</span></label
                >
              </section>
              <section class="recognition-setting">
                <div class="preference-row"><span>Tag Recognition</span></div>
                <p class="group-description">
                  You can select to keep or remove tags from task name.
                </p>
                <label class="recognition-option"
                  ><input
                    type="checkbox"
                    aria-label="Remove tags from task name"
                    :checked="value<string>('tag_recognition') === 'remove'"
                    @change="
                      update(
                        'tag_recognition',
                        ($event.target as HTMLInputElement).checked
                          ? 'remove'
                          : 'keep',
                      )
                    "
                  /><span>Remove tags from task name</span></label
                >
              </section>
            </div>
            <h3 class="group-heading">Task Default</h3>
            <div class="settings-group">
              <label class="preference-row"
                ><span>Default Date</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Default date"
                    :value="value<string>('default_date')"
                    @change="update('default_date', selected($event))"
                  >
                    <option value="none">None</option>
                    <option value="today">Today</option>
                    <option value="tomorrow">Tomorrow</option></select
                  ><PhCaretUpDown :size="10" /></span></label
              ><label class="preference-row"
                ><span>Default Reminders (Due time task)</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Default timed reminder"
                    :value="value<string>('default_reminder_timed')"
                    @change="update('default_reminder_timed', selected($event))"
                  >
                    <option value="none">None</option>
                    <option value="on-time">On time</option>
                    <option value="5m">5 minutes before</option>
                    <option value="15m">15 minutes before</option>
                    <option value="30m">30 minutes before</option>
                    <option value="1h">1 hour before</option></select
                  ><PhCaretUpDown :size="10" /></span></label
              ><label class="preference-row"
                ><span>Default Reminders (All day task)</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Default all-day reminder"
                    :value="value<string>('default_reminder_allday')"
                    @change="
                      update('default_reminder_allday', selected($event))
                    "
                  >
                    <option value="none">None</option>
                    <option value="09:00">On the day, 9:00 AM</option>
                    <option value="18:00">On the day, 6:00 PM</option></select
                  ><PhCaretUpDown :size="10" /></span
              ></label>
            </div>
            <div class="settings-group">
              <label class="preference-row"
                ><span>Default Priority</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Default priority"
                    :value="value<number>('default_priority')"
                    @change="
                      update('default_priority', Number(selected($event)))
                    "
                  >
                    <option :value="0">No Priority</option>
                    <option :value="1">Low Priority</option>
                    <option :value="2">Medium Priority</option>
                    <option :value="3">High Priority</option></select
                  ><PhCaretUpDown :size="10" /></span></label
              ><label class="preference-row"
                ><span>Default Tag</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Default tag"
                    :value="value<string | null>('default_tag_id') || ''"
                    @change="update('default_tag_id', selected($event) || null)"
                  >
                    <option value="">None</option>
                    <option v-for="tag in tags" :key="tag.id" :value="tag.id">
                      {{ tag.name }}
                    </option></select
                  ><PhCaretUpDown :size="10" /></span></label
              ><label class="preference-row"
                ><span>Default List</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Default list"
                    :value="value<string>('default_list_id')"
                    @change="update('default_list_id', selected($event))"
                  >
                    <option
                      v-for="list in lists"
                      :key="list.id"
                      :value="list.id"
                    >
                      {{ list.name }}
                    </option></select
                  ><PhCaretUpDown :size="10" /></span
              ></label>
            </div>
            <div class="settings-group">
              <label class="preference-row"
                ><span>Default Add to</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Default task position"
                    :value="value<string>('default_add_to')"
                    @change="update('default_add_to', selected($event))"
                  >
                    <option value="top">Top of List</option>
                    <option value="bottom">Bottom of List</option></select
                  ><PhCaretUpDown :size="10" /></span></label
              ><label class="preference-row"
                ><span>Overdue Section shows at</span
                ><span class="select-wrap"
                  ><select
                    aria-label="Overdue section position"
                    :value="value<string>('overdue_position')"
                    @change="update('overdue_position', selected($event))"
                  >
                    <option value="top">Top of List</option>
                    <option value="bottom">Bottom of List</option></select
                  ><PhCaretUpDown :size="10" /></span
              ></label>
            </div>
            <button class="settings-link reset-defaults" @click="resetDefaults">
              Reset Default
            </button>
            <h3 class="group-heading template-heading">Task Template</h3>
            <div v-if="templates.length" class="template-grid">
              <article
                v-for="template in templates"
                :key="template.id"
                class="template-card"
              >
                <form
                  v-if="editingTemplate?.id === template.id"
                  class="template-rename"
                  @submit.prevent="renameTemplate"
                >
                  <input
                    v-model="editingTemplate.name"
                    :aria-label="`Name for ${template.name}`"
                    autofocus
                    maxlength="200"
                  />
                  <div>
                    <button
                      type="submit"
                      :disabled="!editingTemplate.name.trim()"
                    >
                      Save</button
                    ><button type="button" @click="editingTemplate = null">
                      Cancel
                    </button>
                  </div>
                </form>
                <template v-else>
                  <button
                    class="template-preview"
                    :aria-label="`Use template ${template.name}`"
                    @click="emit('template-action', 'apply', template.id)"
                  >
                    <strong>{{ template.name }}</strong>
                    <span
                      v-if="templateItems(template).length"
                      class="template-items"
                      ><span
                        v-for="(item, index) in templateItems(template)"
                        :key="index"
                        ><PhSquare :size="12" />{{ item }}</span
                      ></span
                    >
                    <span v-else class="template-notes">{{
                      templateNotes(template) ||
                      String(template.task.title || "")
                    }}</span>
                  </button>
                  <div class="template-actions">
                    <button
                      :aria-label="`Rename template ${template.name}`"
                      @click="
                        editingTemplate = {
                          id: template.id,
                          name: template.name,
                        }
                      "
                    >
                      Rename</button
                    ><button
                      :aria-label="`Delete template ${template.name}`"
                      @click="deletingTemplate = template.id"
                    >
                      <PhTrash :size="12" />
                    </button>
                  </div>
                </template>
              </article>
            </div>
            <p v-else class="preference-help">
              Save a task as a template from its task menu. Your templates will
              appear here.
            </p>
            <div
              v-if="deletingTemplate"
              class="template-delete-confirm"
              role="alert"
            >
              <p>
                Delete this template? Tasks already created from it will be
                kept.
              </p>
              <button
                @click="
                  emit('template-action', 'delete', deletingTemplate);
                  deletingTemplate = null;
                "
              >
                Delete Template</button
              ><button @click="deletingTemplate = null">Cancel</button>
            </div></template
          >
          <template v-else-if="section === 'hotkeys'"
            ><div
              class="settings-segments compact-segments"
              role="tablist"
              aria-label="Keyboard settings"
            >
              <button
                :class="{ selected: hotkeyTab === 'hotkeys' }"
                role="tab"
                :aria-selected="hotkeyTab === 'hotkeys'"
                @click="hotkeyTab = 'hotkeys'"
              >
                Hotkeys</button
              ><button
                :class="{ selected: hotkeyTab === 'shortcuts' }"
                role="tab"
                :aria-selected="hotkeyTab === 'shortcuts'"
                @click="hotkeyTab = 'shortcuts'"
              >
                Shortcuts
              </button>
            </div>
            <template v-if="hotkeyTab === 'hotkeys'"
              ><div
                v-for="group in hotkeyGroups"
                :key="group.name"
                class="hotkey-group"
              >
                <h2>{{ group.name }}</h2>
                <p v-if="group.description" class="preference-help">
                  {{ group.description }}
                </p>
                <div class="settings-group">
                  <div
                    v-for="item in group.items"
                    :key="item.id || item.name"
                    class="preference-row"
                  >
                    <span>{{ item.name }}</span
                    ><span v-if="item.chord" class="static-shortcut">{{
                      displayHotkey(item.chord)
                    }}</span
                    ><span
                      v-else-if="item.readonly && item.id"
                      class="static-shortcut"
                      >{{ displayHotkey(hotkeys[item.id]) }}</span
                    ><span
                      v-else-if="item.id"
                      class="key-recorder"
                      :class="{ recording: recording === item.id }"
                      ><button
                        :aria-label="`Change ${item.name} shortcut`"
                        @click="item.id && startRecording(item.id)"
                      >
                        {{
                          recording === item.id
                            ? firstKey
                              ? `${firstKey.toUpperCase()} then…`
                              : "Type shortcut"
                            : displayHotkey(hotkeys[item.id])
                        }}</button
                      ><button
                        v-if="hotkeys[item.id] && recording !== item.id"
                        class="clear-shortcut"
                        :aria-label="`Clear ${item.name} shortcut`"
                        @click="item.id && saveHotkey(item.id, '')"
                      >
                        <PhX :size="12" /></button
                    ></span>
                  </div>
                </div>
              </div>
              <p v-if="recordingError" class="recording-error" role="alert">
                {{ recordingError }}
              </p>
              <p v-if="recording" class="preference-help">
                {{
                  recording && globalSettingKey(recording)
                    ? "Press Command, Control or Option with a key. Escape cancels."
                    : "Press a shortcut, or two letters in sequence. Escape cancels."
                }}
              </p>
              <div class="hotkeys-footer">
                <button
                  @click="
                    update('hotkeys', {});
                    update('global_quick_add_shortcut', '');
                    update('global_show_hide_shortcut', '');
                    cancelRecording();
                  "
                >
                  Reset Default
                </button>
              </div></template
            ><template v-else
              ><h2 class="shortcut-heading">Quick Add</h2>
              <p class="preference-help">
                Type these shortcuts into the task field to quickly add details.
              </p>
              <div class="settings-group">
                <div
                  v-for="shortcut in [
                    { label: 'Set due date for the task', key: '*' },
                    { label: 'Set priority for the task', key: '!' },
                    { label: 'Set tag for the task', key: '#' },
                    { label: 'Add/Move task to certain list', key: '~ / ^' },
                  ]"
                  :key="shortcut.label"
                  class="preference-row"
                >
                  <span>{{ shortcut.label }}</span
                  ><kbd>{{ shortcut.key }}</kbd>
                </div>
              </div>
              <h2>Text Editing</h2>
              <div class="settings-group">
                <div class="preference-row">
                  <span>Open formatting menu</span><kbd>/</kbd>
                </div>
                <div class="preference-row"><span>Bold</span><kbd>⌘B</kbd></div>
                <div class="preference-row">
                  <span>Italic</span><kbd>⌘I</kbd>
                </div>
              </div></template
            ></template
          >
          <template v-else
            ><h2>About</h2>
            <div class="about-tasks">
              <PhCheckSquare :size="66" weight="fill" />
              <h3>Tasks</h3>
              <p>Version {{ displayedVersion }}</p>
            </div>
            <div class="settings-group">
              <div class="preference-row">
                <span>Storage</span
                ><span class="setting-value">On this Mac</span>
              </div>
              <div class="preference-row">
                <span>Account</span
                ><span class="setting-value">Not required</span>
              </div>
            </div>
            <p class="preference-help">
              Tasks works offline and shares one local database with its
              command-line and MCP tools. Cloud Sync can optionally synchronize
              your personal data through your Google Drive or Dropbox account.
            </p></template
          >
        </div>
        <div
          v-if="interfaceStyleOpen"
          class="interface-style-backdrop"
          @mousedown.self="closeInterfaceStyle"
        >
          <section
            ref="interfacePanel"
            class="interface-style-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="interface-style-title"
            aria-describedby="interface-style-description"
          >
            <button
              class="settings-close interface-style-close"
              aria-label="Close interface style"
              @click="closeInterfaceStyle"
            >
              <PhX :size="9" weight="bold" />
            </button>
            <h2 id="interface-style-title">Interface Style</h2>
            <p id="interface-style-description">
              Choose your preferred interface style.
            </p>
            <div
              class="interface-style-choices"
              role="radiogroup"
              aria-label="Interface style"
            >
              <button
                v-for="style in ['flat', 'card'] as const"
                :key="style"
                class="interface-style-choice"
                role="radio"
                :data-style="style"
                :aria-checked="value<string>('interface_style') === style"
                :tabindex="value<string>('interface_style') === style ? 0 : -1"
                @click="selectInterfaceStyle(style)"
              >
                <span class="interface-style-preview" :class="style"
                  ><img
                    src="/assets/interface-style-reference.png"
                    alt=""
                    draggable="false"
                /></span>
                <span class="interface-style-label"
                  ><span
                    class="interface-style-radio"
                    :class="{
                      checked: value<string>('interface_style') === style,
                    }"
                    ><PhCheck
                      v-if="value<string>('interface_style') === style"
                      :size="11"
                      weight="bold" /></span
                  >{{ style === "flat" ? "Flat" : "Card" }}</span
                >
              </button>
            </div>
          </section>
        </div>
      </section>
    </div></Teleport
  >
</template>

<style scoped>
.settings-backdrop {
  position: fixed;
  inset: 0;
  z-index: 120;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
  background: #171c2933;
  backdrop-filter: blur(2px);
}
.settings-backdrop.standalone {
  position: relative;
  inset: auto;
  display: block;
  width: 100vw;
  height: 100vh;
  padding: 0;
  background: transparent;
  backdrop-filter: none;
}
.settings-backdrop.standalone .settings-window {
  width: 100vw;
  height: 100vh;
  max-width: none;
  max-height: none;
  border-radius: 0;
  box-shadow: none;
}
.native-settings-drag {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 30px;
  z-index: 1;
}
.template-heading {
  margin-top: 28px;
}
.template-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 14px;
  margin: 0 10px;
}
.template-card {
  background: var(--settings-group);
  border-radius: 9px;
  min-width: 0;
  overflow: hidden;
}
.template-preview {
  display: block;
  width: 100%;
  height: 154px;
  overflow: hidden;
  padding: 10px 14px;
  text-align: left;
}
.template-preview strong {
  display: block;
  font-size: 12px;
  line-height: 1.4;
  font-weight: 600;
  margin-bottom: 8px;
}
.template-notes {
  display: -webkit-box;
  -webkit-line-clamp: 6;
  -webkit-box-orient: vertical;
  overflow: hidden;
  font-size: 12px;
  line-height: 1.4;
  color: var(--settings-secondary);
  white-space: pre-line;
}
.template-items {
  display: flex;
  flex-direction: column;
  gap: 7px;
  color: var(--settings-secondary);
  font-size: 12px;
}
.template-items > span {
  display: flex;
  align-items: center;
  gap: 5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.template-actions {
  display: flex;
  justify-content: space-between;
  padding: 0 10px 7px;
  opacity: 0;
}
.template-card:hover .template-actions,
.template-card:focus-within .template-actions {
  opacity: 1;
}
.template-actions button {
  color: var(--settings-secondary);
  font-size: 10px;
}
.template-rename {
  padding: 10px;
}
.template-rename input {
  background: var(--settings-bg);
  border: 1px solid var(--settings-border);
  border-radius: 5px;
  width: 100%;
  padding: 5px;
  font-size: 12px;
}
.template-rename > div {
  display: flex;
  justify-content: space-between;
  margin-top: 8px;
  font-size: 11px;
  color: var(--settings-accent);
}
.template-delete-confirm {
  background: var(--settings-group);
  border-radius: 8px;
  padding: 12px 16px;
  font-size: 12px;
  margin-top: 12px;
}
.template-delete-confirm p {
  margin: 0 0 10px;
  color: var(--settings-secondary);
  line-height: 1.4;
}
.template-delete-confirm button {
  color: var(--settings-accent);
  padding: 3px 12px 3px 0;
}
.settings-window {
  --settings-bg: #fff;
  --settings-sidebar: #f8f8f8;
  --settings-group: #fafafa;
  --settings-text: #202020;
  --settings-secondary: #777;
  --settings-muted: #a1a1a1;
  --settings-border: #e3e3e3;
  --settings-accent: #354471;
  width: 800px;
  height: 680px;
  max-width: calc(100vw - 40px);
  max-height: calc(100vh - 40px);
  background: var(--settings-bg);
  color: var(--settings-text);
  border-radius: 16px;
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr);
  overflow: hidden;
  box-shadow: 0 22px 80px #0003;
  font-size: 14px;
  outline: none;
  font-family:
    -apple-system, BlinkMacSystemFont, "Helvetica Neue", Arial, sans-serif;
  font-weight: 400;
}
.settings-sidebar {
  background: var(--settings-sidebar);
  border-right: 1px solid var(--settings-border);
  padding: 36px 4px 20px;
  position: relative;
  overflow: auto;
}
.settings-close {
  position: absolute;
  top: 10px;
  left: 10px;
  width: 13px;
  height: 13px;
  padding: 0;
  background: #ff605a;
  border: 1px solid #e8514d;
  color: #8f1e1b;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
}
.settings-close svg {
  opacity: 0;
}
.settings-close:hover svg,
.settings-close:focus-visible svg {
  opacity: 1;
}
.settings-sidebar h1 {
  font-size: 16px;
  font-weight: 650;
  letter-spacing: -0.3px;
  margin: 0 12px 15px;
  line-height: 22px;
}
.settings-sidebar nav button {
  height: 38px;
  width: 100%;
  display: flex;
  align-items: center;
  gap: 10px;
  text-align: left;
  padding: 0 14px;
  border-radius: 11px;
  font-size: 14px;
  color: #5b5b5b;
}
.settings-sidebar nav button.active {
  background: #eeeeee;
}
.settings-sidebar nav button:hover {
  background: #eeeeee;
}
.settings-sidebar nav button.separate-group {
  margin-top: 16px;
}
.settings-sidebar nav button:last-child {
  margin-top: 24px;
}
.settings-pane {
  overflow: auto;
  min-height: 0;
  padding: 35px 28px 34px;

  scrollbar-width: thin;
}
.settings-pane h2 {
  font-size: 16px;
  font-weight: 650;
  line-height: 22px;
  margin: 0 18px 16px;
  letter-spacing: -0.2px;
}
.settings-group {
  background: var(--settings-group);
  border-radius: 9px;
  padding: 4px 18px;
  margin-bottom: 14px;
}
.preference-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 40px;
  font-size: 14px;
  line-height: 1.4;
}
.preference-row > span:first-of-type {
  flex: 1;
  min-width: 0;
}
.smart-row > span:first-of-type {
  flex: 1;
}
.select-wrap {
  display: flex;
  align-items: center;
  position: relative;
  color: var(--settings-secondary);
  max-width: 58%;
  margin-left: auto;
  flex-shrink: 0;
}
.select-wrap select {
  appearance: none;
  border: 0;
  background: transparent;
  color: var(--settings-secondary);
  font: inherit;
  font-size: 13px;
  text-align: right;
  padding: 5px 16px 5px 4px;
  cursor: pointer;
  max-width: 100%;
  min-width: 32px;
}
.select-wrap > svg {
  pointer-events: none;
  position: absolute;
  right: 3px;
  color: var(--settings-muted);
}
.select-wrap select:focus-visible {
  outline: 2px solid var(--settings-accent);
  border-radius: 4px;
}
.settings-link {
  color: var(--settings-accent);
  font-size: 13px;
  padding: 0;
  text-align: right;
  white-space: nowrap;
}
.settings-link:hover {
  text-decoration: underline;
}
.interface-style-trigger {
  width: 100%;
  padding: 0;
  text-align: left;
}
.interface-style-value {
  display: flex;
  align-items: center;
  gap: 5px;
  color: var(--settings-secondary);
}
.interface-style-value svg {
  color: var(--settings-muted);
}
.interface-style-backdrop {
  position: fixed;
  inset: 0;
  z-index: 5;
  display: flex;
  align-items: center;
  justify-content: center;
}
.interface-style-dialog {
  position: relative;
  width: 440px;
  height: 310px;
  max-width: calc(100vw - 32px);
  border: 1px solid #bfc0c4;
  border-radius: 11px;
  background: var(--settings-bg);
  color: var(--settings-text);
  box-shadow: 0 8px 24px #0004;
  padding-top: 18px;
}
.interface-style-close {
  left: 8px;
  top: 8px;
  width: 12px;
  height: 12px;
}
.interface-style-dialog h2 {
  margin: 0;
  text-align: center;
  font-size: 16px;
  line-height: 22px;
  font-weight: 600;
}
.interface-style-dialog p {
  margin: 15px 0 0;
  text-align: center;
  font-size: 13px;
  line-height: 20px;
  color: var(--settings-muted);
}
.interface-style-choices {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 160px));
  justify-content: space-between;
  margin: 32px 35px 0;
  gap: 20px;
}
.interface-style-choice {
  min-width: 0;
  padding: 0;
}
.interface-style-preview {
  display: block;
  aspect-ratio: 40 / 29;
  width: 160px;
  max-width: 100%;
  overflow: hidden;
  position: relative;
  border-radius: 6px;
  box-shadow: 0 0 24px #0000000b;
}
.interface-style-preview img {
  position: absolute;
  max-width: none;
  width: 300%;
  height: auto;
  left: -35%;
  top: -106.8965517%;
  pointer-events: none;
}
.interface-style-preview.card img {
  left: -165%;
}
.interface-style-label {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  margin-top: 25px;
  font-size: 13px;
  line-height: 20px;
}
.interface-style-radio {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 1px solid #cecece;
  display: flex;
  align-items: center;
  justify-content: center;
}
.interface-style-radio.checked {
  border-color: var(--settings-accent);
  background: var(--settings-accent);
  color: #fff;
}
.preference-help {
  font-size: 11px;
  line-height: 1.5;
  color: var(--settings-muted);
  margin: 10px 18px 20px;
}
.group-description {
  font-size: 11px;
  line-height: 1.4;
  color: var(--settings-muted);
  margin: -2px 0 10px;
}
.recognition-group {
  padding-bottom: 18px;
}
.recognition-setting + .recognition-setting {
  margin-top: 9px;
}
.recognition-setting .group-description {
  font-size: 12px;
  line-height: 15px;
}
.recognition-group .native-switch:checked {
  background: var(--settings-accent);
}
.recognition-option {
  display: flex;
  align-items: center;
  gap: 9px;
  min-height: 20px;
  padding-left: 2px;
  font-size: 14px;
  line-height: 20px;
}
.recognition-option input {
  width: 13px;
  height: 13px;
  margin: 0;
  flex-shrink: 0;
  accent-color: var(--settings-accent);
}
.recognition-option.disabled {
  color: var(--settings-muted);
}
.recognition-option input:focus-visible {
  outline: 2px solid var(--settings-accent);
  outline-offset: 3px;
}
.native-switch {
  appearance: none;
  width: 26px;
  height: 16px;
  border-radius: 10px;
  background: #d9d9d9;
  position: relative;
  cursor: pointer;
  flex-shrink: 0;
  margin: 0;
}
.native-switch:before {
  content: "";
  width: 14px;
  height: 14px;
  border-radius: 50%;
  position: absolute;
  left: 1px;
  top: 1px;
  background: #fff;
  box-shadow: 0 1px 2px #0001;
  transition: transform 0.12s;
}
.native-switch:checked {
  background: #6477b7;
}
.native-switch:checked:before {
  transform: translateX(10px);
}
.native-switch:focus-visible {
  outline: 2px solid var(--settings-accent);
  outline-offset: 2px;
}
.settings-segments {
  display: flex;
  justify-content: center;
  align-items: center;
  border-radius: 22px;
  background: #f2f2f2;
  padding: 2px;
  width: max-content;
  margin: 0 auto 24px;
}
.settings-segments button {
  font-size: 14px;
  color: var(--settings-secondary);
  padding: 7px 13px;
  border-radius: 20px;
  line-height: 18px;
}
.settings-segments button.selected {
  background: var(--settings-bg);
  color: var(--settings-text);
  box-shadow: 0 0 2px #ddd5;
}
.appearance-pane {
  padding-top: 36px;
}
.display-pane .settings-segments {
  margin-bottom: 23px;
}
.choice-heading,
.display-section h3 {
  font-size: 14px;
  font-weight: 600;
  line-height: 20px;
  margin: 19px 0 12px;
}
.color-series {
  display: grid;
  grid-template-columns: repeat(8, minmax(0, 1fr));
  column-gap: 13px;
  row-gap: 20px;
}
.theme-choice {
  padding: 0;
  min-width: 0;
  text-align: center;
  font-size: 12px;
  color: var(--settings-muted);
  line-height: 18px;
}
.color-swatch {
  width: 100%;
  height: 55px;
  border-radius: 10px;
  display: block;
  margin-bottom: 7px;
  position: relative;
}
.default-swatch {
  box-shadow: inset 0 0 0 1px #d0d0d0;
}
.dark-swatch-dots {
  position: absolute;
  inset: 0;
  margin: auto;
  color: #405eb2;
}
.app-icon-choices {
  display: grid;
  grid-template-columns: repeat(8, minmax(0, 1fr));
  gap: 16px 13px;
  margin-bottom: 24px;
}
.app-icon-choice {
  position: relative;
  padding: 0;
  width: 100%;
  aspect-ratio: 1;
}
.app-icon-choice img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.dock-settings {
  margin-top: 4px;
}
.choice-check {
  position: absolute;
  right: -4px;
  bottom: -4px;
  background: var(--settings-accent);
  border: 1.5px solid var(--settings-bg);
  color: #fff;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1;
}
.list-color-help {
  display: inline-flex;
  position: relative;
  vertical-align: middle;
  color: var(--muted);
  cursor: help;
  margin-left: 3px;
}
.list-color-help [role="tooltip"] {
  display: none;
  position: absolute;
  z-index: 5;
  left: -50px;
  top: 24px;
  width: 220px;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--popover);
  color: var(--text);
  box-shadow: 0 2px 12px #0003;
  font-size: 12px;
  font-weight: 400;
  line-height: 1.4;
}
.list-color-help:hover [role="tooltip"],
.list-color-help:focus [role="tooltip"] {
  display: block;
}
.display-section {
  margin-bottom: 22px;
}
.display-section h3 {
  margin: 0 0 16px;
}
.display-choices {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 16px;
}
.display-choice {
  display: flex;
  flex-direction: column;
  gap: 9px;
  text-align: center;
  align-items: stretch;
  min-width: 0;
  padding: 0;
  font-size: 12px;
  color: var(--settings-muted);
  line-height: 18px;
}
.preview-card {
  position: relative;
  min-height: 50px;
  width: 100%;
  border: 1px solid var(--settings-border);
  border-radius: 10px;
  color: var(--settings-text);
  box-shadow: 0 0 1px #0001;
}
.font-preview {
  display: flex;
  align-items: center;
  justify-content: center;
}
.count-preview {
  height: 76px;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 16px;
  padding: 12px 11px;
}
.count-preview > span:not(.choice-check) {
  display: flex;
  align-items: center;
  gap: 9px;
  font-size: 12px;
  color: #555;
  text-align: left;
  line-height: 18px;
}
.count-preview small {
  margin-left: auto;
  font-size: 10px;
  color: var(--settings-muted);
}
.count-preview svg {
  color: #8a8a8a;
}
.count-preview .choice-check svg {
  color: #fff;
}
.task-preview {
  height: 88px;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 16px;
  padding: 12px 13px;
}
.task-preview > span:not(.choice-check) {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 14px;
  line-height: 18px;
  position: relative;
  white-space: nowrap;
}
.task-preview > span:not(.choice-check) > svg {
  color: #aaa;
}
.task-preview.with-list-color:before,
.task-preview.with-list-color:after {
  content: "";
  position: absolute;
  left: -1px;
  width: 3px;
  height: 44px;
}
.task-preview.with-list-color:before {
  top: 0;
  background: #5076ee;
  border-radius: 8px 0 0;
}
.task-preview.with-list-color:after {
  bottom: 0;
  background: #e9ac22;
  border-radius: 0 0 0 8px;
}
.completed-preview > span:not(.choice-check) {
  color: #a5a5a5;
}
.completed-preview > span:not(.choice-check) > svg {
  color: #d4d4d4;
}
.strike-preview > span:not(.choice-check) {
  text-decoration: line-through;
}
.group-heading {
  font-size: 16px;
  line-height: 22px;
  font-weight: 600;
  margin: 26px 18px 12px;
}
.group-heading:first-of-type {
  margin-top: 22px;
}
.reset-defaults {
  margin: 0 18px;
}
.hotkeys-pane {
  position: relative;
  padding-bottom: 62px;
}
.compact-segments {
  margin-bottom: 24px;
}
.compact-segments button {
  font-size: 12px;
  line-height: 17px;
  padding: 3px 12px;
}
.hotkey-group h2 {
  margin-top: 24px;
}
.hotkey-group:first-of-type h2 {
  margin-top: 0;
}
.key-recorder {
  margin-left: auto;
  width: 128px;
  display: flex;
  align-items: center;
  height: 24px;
  border: 1px solid transparent;
  border-radius: 6px;
  flex-shrink: 0;
  color: var(--settings-muted);
  font-size: 12px;
}
.key-recorder > button:first-child {
  flex: 1;
  height: 100%;
  text-align: center;
  padding: 0 5px;
  font-size: 12px;
  white-space: nowrap;
}
.static-shortcut {
  min-width: 112px;
  padding: 0 24px 0 5px;
  color: var(--settings-muted);
  font-size: 12px;
  text-align: right;
}
.key-recorder:hover,
.key-recorder:focus-within {
  border-color: var(--settings-border);
}
.key-recorder.recording {
  border-color: var(--settings-accent);
  color: var(--settings-accent);
}
.clear-shortcut {
  width: 20px;
  height: 22px;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  visibility: hidden;
}
.key-recorder:hover .clear-shortcut,
.key-recorder:focus-within .clear-shortcut {
  visibility: visible;
}
.hotkeys-footer {
  display: flex;
  justify-content: flex-end;
  padding-top: 4px;
}
.hotkeys-footer button {
  font-size: 13px;
  border: 1px solid var(--settings-border);
  padding: 5px 17px;
  border-radius: 8px;
  color: var(--settings-secondary);
  background: var(--settings-bg);
}
.recording-error {
  font-size: 11px;
  color: #bf555c;
  line-height: 1.5;
  margin: 12px 18px;
}
.shortcut-heading {
  margin-top: 24px !important;
}
.settings-pane kbd {
  font-size: 13px;
  font-family: inherit;
  color: var(--settings-secondary);
  margin-left: auto;
  min-width: 35px;
  text-align: center;
}
.about-tasks {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 25px 0 30px;
  color: var(--settings-accent);
}
.about-tasks h3 {
  font-size: 22px;
  font-weight: 600;
  margin: 12px 0 5px;
  color: var(--settings-text);
}
.about-tasks p {
  font-size: 12px;
  color: var(--settings-muted);
  margin: 0;
}
.setting-value {
  margin-left: auto;
  font-size: 13px;
  color: var(--settings-secondary);
}
.settings-window:focus-visible {
  outline: none;
}
.settings-window button:focus-visible {
  outline: 2px solid var(--settings-accent);
  outline-offset: 2px;
}
.settings-window button:disabled {
  opacity: 0.5;
}
:global(:root[data-theme="dark"]) .settings-window {
  --settings-bg: #25262d;
  --settings-sidebar: #2c2d34;
  --settings-group: #2e2f37;
  --settings-text: #e4e5e9;
  --settings-secondary: #b3b4be;
  --settings-muted: #898c98;
  --settings-border: #42444e;
  --settings-accent: #a1b1ed;
}
.settings-window:global([data-theme="dark"]) {
  color: var(--settings-text);
}
:global(:root[data-theme="dark"]) .settings-sidebar nav button {
  color: #bfc1cb;
}
:global(:root[data-theme="dark"]) .settings-sidebar nav button.active,
:global(:root[data-theme="dark"]) .settings-sidebar nav button:hover,
:global(:root[data-theme="dark"]) .settings-segments {
  background: #373942;
}
:global(:root[data-theme="dark"]) .count-preview > span:not(.choice-check) {
  color: #bfc1cb;
}
:global(:root[data-theme="dark"]) .choice-check {
  background: #536895;
}
:global(:root[data-theme="dark"]) .native-switch {
  background: #5d606c;
}
:global(:root[data-theme="dark"]) .native-switch:checked {
  background: #8195da;
}
@media (max-width: 650px) {
  .settings-backdrop {
    padding: 12px;
  }
  .settings-window {
    max-width: 100%;
    max-height: calc(100vh - 24px);
    grid-template-columns: 150px minmax(0, 1fr);
    border-radius: 12px;
  }
  .settings-sidebar {
    padding-top: 35px;
  }
  .settings-sidebar h1 {
    margin-left: 10px;
  }
  .settings-sidebar nav button {
    padding: 0 9px;
    font-size: 12px;
    gap: 7px;
  }
  .settings-sidebar nav button svg {
    width: 16px;
    height: 16px;
  }
  .settings-pane {
    padding: 35px 16px 24px;
  }
  .settings-pane h2 {
    margin-left: 0;
  }
  .settings-group {
    padding: 4px 10px;
  }
  .preference-row {
    font-size: 12px;
    gap: 7px;
    flex-wrap: wrap;
    padding: 8px 0;
  }
  .select-wrap {
    max-width: 100%;
  }
  .select-wrap select {
    font-size: 11px;
  }
  .preference-row > span:first-of-type {
    min-width: 80px;
  }
  .color-series {
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 14px 10px;
  }
  .app-icon-choices {
    grid-template-columns: repeat(2, 53px);
    gap: 14px;
  }
  .color-swatch {
    height: 45px;
  }
  .display-choices {
    gap: 8px;
  }
  .display-choice {
    font-size: 10px;
    line-height: 14px;
  }
  .display-choices {
    grid-template-columns: 1fr;
  }
  .font-preview {
    height: 42px;
  }
  .count-preview {
    height: 64px;
  }
  .task-preview {
    height: 74px;
  }
  .preview-card {
    max-width: 180px;
    align-self: center;
  }
  .display-section h3 {
    font-size: 13px;
  }
  .preference-help {
    margin-left: 0;
    margin-right: 0;
  }
  .group-heading {
    margin-left: 0;
  }
  .settings-link {
    font-size: 11px;
    white-space: normal;
  }
  .settings-segments button {
    padding: 6px 8px;
    font-size: 12px;
  }
  .key-recorder {
    width: 115px;
  }
  .hotkey-group .preference-row > span:first-child {
    flex-basis: 100%;
  }
  .settings-sidebar nav button:last-child {
    margin-top: 16px;
  }
  .native-switch {
    margin-left: auto;
  }
}
@media (prefers-reduced-motion: reduce) {
  .native-switch:before {
    transition: none;
  }
}
</style>
