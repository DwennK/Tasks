<script setup lang="ts">
import {
  computed,
  defineAsyncComponent,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import {
  PhArrowClockwise,
  PhCalendarDots,
  PhCalendarStar,
  PhCirclesFour,
  PhClock,
  PhQuestion,
  PhArchiveBox,
  PhArrowUp,
  PhArrowsDownUp,
  PhBell,
  PhCalendarBlank,
  PhCheck,
  PhCheckSquare,
  PhCloudSlash,
  PhDotsThree,
  PhFlag,
  PhFolder,
  PhGearSix,
  PhInfo,
  PhList,
  PhMagnifyingGlass,
  PhMoon,
  PhPlus,
  PhSquaresFour,
  PhSun,
  PhTag,
  PhTrash,
  PhX,
  PhXSquare,
  PhCaretDown,
  PhCaretRight,
  PhPushPin,
} from "@phosphor-icons/vue";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import {
  canDropTask,
  clearTaskDrag,
  taskDropPosition,
} from "./composables/taskDrag";
import SidebarTree from "./components/SidebarTree.vue";
import TaskNavigationGlyph from "./components/TaskNavigationGlyph.vue";
import { IconSquareRoundedCheck, IconTrash } from "@tabler/icons-vue";
const TaskEditor = defineAsyncComponent(
  () => import("./components/TaskEditor.vue"),
);
import TaskTreeRow from "./components/TaskTreeRow.vue";
import PaneResizer from "./components/PaneResizer.vue";
import TaskContextMenu from "./components/TaskContextMenu.vue";
const UpdateNotice = defineAsyncComponent(
  () => import("./components/UpdateNotice.vue"),
);
const SettingsPanel = defineAsyncComponent(
  () => import("./components/SettingsPanel.vue"),
);
const TaskTimeline = defineAsyncComponent(
  () => import("./components/TaskTimeline.vue"),
);
const TaskSummary = defineAsyncComponent(
  () => import("./components/TaskSummary.vue"),
);
const BatchAddDialog = defineAsyncComponent(
  () => import("./components/BatchAddDialog.vue"),
);
const ReminderPanel = defineAsyncComponent(
  () => import("./components/ReminderPanel.vue"),
);
import AppDialog from "./components/AppDialog.vue";
import SearchDialog, { type SearchTarget } from "./components/SearchDialog.vue";
const QuickAddDialog = defineAsyncComponent(
  () => import("./components/QuickAddDialog.vue"),
);
const ListFolderDialog = defineAsyncComponent(
  () => import("./components/ListFolderDialog.vue"),
);
const BulkTaskDialog = defineAsyncComponent(
  () => import("./components/BulkTaskDialog.vue"),
);
import { isNative } from "./adapter";
import {
  beginEditorWrite,
  editorWriteCheckpoint,
  waitForEditorWrites,
} from "./components/editorWork";
import type { SavedFilter, Task, ViewPreferences } from "./types";
import type { SummaryPreset } from "./lib/task-summary";
import {
  localDate,
  dateLabel,
  useTasks,
  type View,
} from "./composables/useTasks";
import { parseTaskInput, defaultHotkeys, keyChord } from "./lib/task-input";
import { captureTaskTitleFields } from "./composables/useTaskTitleField";
const {
  state,
  tasks,
  active,
  selected,
  visibleTasks,
  viewPreferences,
  setViewPreferences,
  refresh,
  checkRevision,
  mutate,
  navigate,
  retryDrafts,
  flushAll,
  writeCheckpoint,
  editTask,
} = useTasks();
const editorRef = ref<{
  reconcileEditorDrafts: () => void;
  openDate: () => void;
  openTags: () => void;
  focusSubtask: () => void;
  printTask?: () => Promise<void>;
}>();
const standaloneReminders = new URLSearchParams(window.location.search).has(
  "reminders",
);
const batchPaste = ref<{ text: string; fromQuick: boolean } | null>(null);
const standaloneSettings = new URLSearchParams(window.location.search).has(
  "settings",
);
const addInput = ref<HTMLInputElement>(),
  searchInput = ref<HTMLInputElement>();
const newTitle = ref(""),
  searchOpen = ref(false),
  creating = ref(false),
  quickOpen = ref(false),
  settingsOpen = ref(false),
  shortcutsOpen = ref(false),
  commandOpen = ref(false);
const quickTitle = ref(""),
  quickDatePatch = ref<Record<string, unknown>>({}),
  quickPriority = ref(0),
  quickList = ref("inbox");
const quickDateEdited = ref(false),
  quickPriorityEdited = ref(false),
  quickListEdited = ref(false);
const parsedQuickInput = computed(
  () =>
    parseTaskInput(
      quickTitle.value,
      settings.value,
      activeLists.value,
      state.snapshot.tags,
    ) as Record<string, unknown>,
);
const quickPreviewDate = computed(() =>
  !quickDateEdited.value && parsedQuickInput.value.due_date
    ? {
        ...quickDatePatch.value,
        due_date: parsedQuickInput.value.due_date,
        due_time: parsedQuickInput.value.due_time ?? null,
      }
    : quickDatePatch.value,
);
const quickPreviewPriority = computed(() =>
  !quickPriorityEdited.value && parsedQuickInput.value.priority !== undefined
    ? Number(parsedQuickInput.value.priority)
    : quickPriority.value,
);
const quickPreviewList = computed(() =>
  !quickListEdited.value && parsedQuickInput.value.list_id
    ? String(parsedQuickInput.value.list_id)
    : quickList.value,
);
function setQuickDate(patch: Record<string, unknown>) {
  const next = { ...quickPreviewDate.value, ...patch };
  quickDateEdited.value = true;
  quickDatePatch.value = next;
}
function setQuickPriority(value: number) {
  quickPriorityEdited.value = true;
  quickPriority.value = value;
}
function setQuickList(value: string) {
  quickListEdited.value = true;
  quickList.value = value;
}
const commandQuery = ref("");
const menu = ref<"sort" | "list" | "create" | null>(null);
const settingsSection = ref<
  | "sync"
  | "updates"
  | "smart"
  | "notifications"
  | "date"
  | "appearance"
  | "more"
  | "hotkeys"
  | "about"
>("smart");
const management = ref<{
  kind: "folder" | "list";
  id: string | null;
  name: string;
  parentId: string | null;
  color: string;
  deleting: boolean;
} | null>(null);
const managementBusy = ref(false),
  managementError = ref("");
const managementItem = computed(() =>
  !management.value?.id
    ? null
    : management.value.kind === "folder"
      ? state.snapshot.folders.find((item) => item.id === management.value?.id)
      : state.snapshot.lists.find((item) => item.id === management.value?.id),
);
const tasksById = computed(
  () => new Map(tasks.value.map((task) => [task.id, task])),
);
const tasksByParent = computed(() => {
  const map = new Map<string, Task[]>();
  for (const task of tasks.value)
    if (task.parent_id) {
      const group = map.get(task.parent_id) || [];
      group.push(task);
      map.set(task.parent_id, group);
    }
  for (const group of map.values())
    group.sort(
      (a, b) =>
        a.position - b.position || a.created_at.localeCompare(b.created_at),
    );
  return map;
});
const tagDialog = ref<{
  id: string | null;
  name: string;
  color: string;
  deleting: boolean;
} | null>(null);
const filterDialog = ref<SavedFilter | null>(null);
const sectionDialog = ref<{ id: string | null; name: string } | null>(null);
const templateDialog = ref(false),
  templateName = ref("");
const templateTaskId = ref<string | null>(null);
const selectedIds = ref(new Set<string>());
const editingTitleId = ref<string | null>(null);
const editingTitleOrder = ref<string[]>([]);
const finishingTitleId = ref<string | null>(null);
let titlePointerActive = false;
let titlePointerTimer: ReturnType<typeof setTimeout> | undefined;
let deferredTitleBlur: (() => void) | undefined;
function beginTitlePointer() {
  if (titlePointerTimer) clearTimeout(titlePointerTimer);
  titlePointerActive = true;
}
function endTitlePointer() {
  if (titlePointerTimer) clearTimeout(titlePointerTimer);
  titlePointerActive = false;
  const finish = deferredTitleBlur;
  deferredTitleBlur = undefined;
  finish?.();
}
function releaseTitlePointer() {
  // The click belongs to this pointerup. Its target must be resolved before
  // releasing the frozen sort/search position of the field that just blurred.
  titlePointerTimer = setTimeout(endTitlePointer, 0);
}
const taskListElement = ref<HTMLElement>();
const context = ref<{ task: Task; x: number; y: number } | null>(null);
const bulkDialog = ref<{
  mode: "date" | "tags";
  task: Task;
  ids: string[];
} | null>(null);
const bulkBusy = ref(false),
  bulkError = ref("");
const bulkTasks = computed(() =>
  (bulkDialog.value?.ids || [])
    .map((id) => tasksById.value.get(id))
    .filter((task): task is Task => !!task),
);
const collapsedGroups = ref(new Set<string>());
const subtaskExpansion = ref<{ revision: number; expanded: boolean } | null>(
  null,
);
const addBelow = ref<{ anchorId: string; title: string } | null>(null);
const addingBelow = ref(false);
const postponing = ref(false);
const tagsCollapsed = ref(false),
  filtersCollapsed = ref(false),
  desktopSidebarHidden = ref(false);
const media = window.matchMedia("(prefers-color-scheme: dark)");
const systemDark = ref(media.matches);
const settings = computed(() => state.snapshot.settings);
const interfaceStyle = computed(() =>
  settings.value.interface_style === "flat" ? "flat" : "card",
);
const viewportWidth = ref(window.innerWidth);
const resizingSidebar = ref<number | null>(null),
  resizingList = ref<number | null>(null);
const sidebarMax = computed(() =>
  Math.max(150, Math.min(500, viewportWidth.value - 62 - 250 - 280)),
);
const sidebarWidth = computed(() =>
  Math.min(
    sidebarMax.value,
    Math.max(
      150,
      resizingSidebar.value ??
        Number(
          settings.value.sidebar_width ||
            (viewportWidth.value >= 1700
              ? 230
              : viewportWidth.value <= 850
                ? 185
                : viewportWidth.value <= 1100
                  ? 195
                  : 218),
        ),
    ),
  ),
);
const listMax = computed(() =>
  Math.max(
    250,
    Math.min(
      1000,
      viewportWidth.value -
        70 -
        (desktopSidebarHidden.value ? 0 : sidebarWidth.value) -
        280,
    ),
  ),
);
const listWidth = computed(() =>
  Math.min(
    listMax.value,
    Math.max(
      250,
      resizingList.value ??
        Number(
          settings.value.list_width ||
            (desktopSidebarHidden.value
              ? 460
              : viewportWidth.value >= 1700
                ? 440
                : viewportWidth.value <= 850
                  ? selected.value
                    ? 260
                    : 300
                  : viewportWidth.value <= 1100
                    ? 340
                    : 388),
        ),
    ),
  ),
);
const paneStyle = computed(() => ({
  "--sidebar-width": `${sidebarWidth.value}px`,
  "--list-width": `${listWidth.value}px`,
}));
function viewportChanged() {
  viewportWidth.value = window.innerWidth;
}
async function savePaneWidth(
  key: "sidebar_width" | "list_width",
  value: number,
) {
  const local = key === "sidebar_width" ? resizingSidebar : resizingList;
  local.value = value;
  try {
    await mutate("settings.set", { key, value });
  } catch {
  } finally {
    if (local.value === value) local.value = null;
  }
}
const theme = computed(() => String(settings.value.theme || "light"));
const dark = computed(
  () =>
    theme.value === "dark" || (theme.value === "system" && systemDark.value),
);
const palettes: Record<string, [string, string, string]> = {
  default: ["#293450", "#4b5c8c", "#587bff"],
  navy: ["#293450", "#4b5c8c", "#587bff"],
  sky: ["#6485ff", "#4369e6", "#5278ed"],
  turquoise: ["#70d5c4", "#13b79c", "#18a98f"],
  teal: ["#80c9ce", "#398e9b", "#3799a6"],
  matcha: ["#bdcbb2", "#829d78", "#6e8d60"],
  sunshine: ["#ffd380", "#f3a464", "#da9b39"],
  peach: ["#ffb5ac", "#ed75a0", "#df7693"],
  lilac: ["#ceb6d8", "#919bd3", "#9680bc"],
  ebony: ["#bda493", "#8b6b58", "#9a7762"],
  gray: ["#555a65", "#303541", "#707785"],
  dark: ["#202124", "#202124", "#839aff"],
};
watch(
  [dark, settings],
  () => {
    const root = document.documentElement;
    root.dataset.theme = dark.value ? "dark" : "light";
    root.dataset.fontSize = String(settings.value.font_size || "normal");
    root.dataset.completedStyle = String(
      settings.value.completed_style || "strikethrough",
    );
    const palette =
      palettes[String(settings.value.theme_color || "navy")] || palettes.navy!;
    root.style.setProperty(
      "--shell-rail",
      `linear-gradient(${palette[0]},${palette[1]})`,
    );
    root.style.setProperty("--accent", dark.value ? "#839aff" : palette[2]);
  },
  { immediate: true, deep: true },
);
const viewNames: Record<View, string> = {
  summary: "Summary",
  today: "Today",
  tomorrow: "Tomorrow",
  upcoming: "Next 7 Days",
  inbox: "Inbox",
  all: "All",
  completed: "Completed",
  wont_do: "Won’t Do",
  trash: "Trash",
  list: "",
  search: "Search",
  tag: "",
  filter: "",
  archived: "Archived Lists",
};
const currentList = computed(() =>
  state.snapshot.lists.find((l) => l.id === state.listId),
);
const savedFilters = computed(
  () => (settings.value.saved_filters || []) as SavedFilter[],
);
const title = computed(() =>
  state.view === "list"
    ? currentList.value?.name || "Tasks"
    : state.view === "tag"
      ? state.snapshot.tags.find((t) => t.id === state.listId)?.name || "Tag"
      : state.view === "filter"
        ? savedFilters.value.find((f) => f.id === state.listId)?.name ||
          "Filter"
        : viewNames[state.view],
);
const activeLists = computed(() =>
  state.snapshot.lists.filter((l) => !l.archived_at),
);
const listOptions = computed(() =>
  activeLists.value.filter((l) => l.id !== "inbox"),
);
const archivedLists = computed(() =>
  state.snapshot.lists.filter((l) => l.archived_at),
);
const showCounts = computed(() => settings.value.sidebar_count !== "hidden");
const hideNoteCounts = computed(() => settings.value.sidebar_count === "tasks");
function countedTasks(items: Task[]) {
  return hideNoteCounts.value ? items.filter((task) => !task.is_note) : items;
}
const scheduledTasks = computed(() =>
  tasks.value.filter(
    (t) =>
      !t.completed_at &&
      !t.deleted_at &&
      !t.wont_do_at &&
      !state.snapshot.lists.find((l) => l.id === t.list_id)?.archived_at &&
      state.snapshot.lists.find((l) => l.id === t.list_id)
        ?.show_in_smart_lists !== false,
  ),
);
const tomorrowDate = () => {
  const d = new Date();
  d.setDate(d.getDate() + 1);
  return localDate(d);
};
const weekDate = () => {
  const d = new Date();
  d.setDate(d.getDate() + 6);
  return localDate(d);
};
function smartTaskCounts(includeNotes: boolean): Record<string, number> {
  const countable = (items: Task[]) =>
    includeNotes ? items : items.filter((task) => !task.is_note);
  const activeTasks = countable(active.value);
  const scheduled = countable(scheduledTasks.value);
  const allTasks = countable(tasks.value);
  return {
    all: activeTasks.length,
    today: scheduled.filter((t) => t.due_date && t.due_date <= localDate())
      .length,
    tomorrow: scheduled.filter((t) => t.due_date === tomorrowDate()).length,
    upcoming: scheduled.filter(
      (t) =>
        t.due_date && t.due_date >= localDate() && t.due_date <= weekDate(),
    ).length,
    inbox: activeTasks.filter((t) => t.list_id === "inbox").length,
    completed: allTasks.filter((t) => t.completed_at && !t.deleted_at).length,
    wont_do: allTasks.filter((t) => t.wont_do_at && !t.deleted_at).length,
    trash: allTasks.filter((t) => t.deleted_at).length,
    tags: state.snapshot.tags.length,
    filters: savedFilters.value.length,
    summary: allTasks.filter((t) => !t.deleted_at && !t.is_note).length,
  };
}
const smartCounts = computed(() => smartTaskCounts(!hideNoteCounts.value));
// Count preferences never hide a view that contains only notes.
const smartContentCounts = computed(() => smartTaskCounts(true));
function smartVisible(id: string) {
  const value =
    (settings.value.smart_lists as Record<string, string> | undefined)?.[id] ||
    (["tomorrow", "wont_do"].includes(id) ? "auto" : "show");
  return (
    value === "show" ||
    (value === "auto" && (smartContentCounts.value[id] || 0) > 0)
  );
}
const smartViews = computed(() =>
  [
    { id: "all" as const, name: "All" },
    { id: "today" as const, name: "Today" },
    { id: "tomorrow" as const, name: "Tomorrow" },
    { id: "upcoming" as const, name: "Next 7 Days" },
    { id: "inbox" as const, name: "Inbox" },
  ].filter((v) => smartVisible(v.id)),
);
const bottomViews = computed(() =>
  [
    { id: "completed" as View, name: "Completed", icon: PhCheckSquare },
    { id: "wont_do" as View, name: "Won’t Do", icon: PhXSquare },
    { id: "trash" as View, name: "Trash", icon: PhTrash },
  ].filter((v) => smartVisible(v.id)),
);
const canCreate = computed(
  () =>
    !["trash", "completed", "wont_do", "archived", "summary"].includes(
      state.view,
    ) && !currentList.value?.archived_at,
);
const canReorder = computed(
  () =>
    ["list", "inbox"].includes(state.view) &&
    viewPreferences.value.sort_by === "manual",
);
const hasFailedDraft = computed(() =>
  Object.values(state.drafts).some((d) => d.failed),
);
const emptyLabel = computed(() =>
  state.view === "search"
    ? "No matching tasks"
    : state.view === "completed"
      ? "No completed tasks"
      : state.view === "trash"
        ? "Trash is empty"
        : state.view === "today"
          ? "You’re all set for today"
          : "No tasks yet",
);
function groupRootTasks(items: Task[]) {
  const ids = new Set(items.map((task) => task.id));
  const byId = tasksById.value;
  return items.filter((task) => {
    let parent = task.parent_id;
    const seen = new Set<string>();
    while (parent && !seen.has(parent)) {
      if (ids.has(parent)) return false;
      seen.add(parent);
      parent = byId.get(parent)?.parent_id || null;
    }
    return true;
  });
}
const displayedTasks = computed(() => {
  if (!editingTitleId.value) return visibleTasks.value;
  const items = [...visibleTasks.value];
  const edited = tasksById.value.get(editingTitleId.value);
  // A live rename must not remove its own field from search or move it while
  // the user is typing in a title-sorted list.
  if (
    edited &&
    !edited.deleted_at &&
    !items.some((task) => task.id === edited.id)
  )
    items.push(edited);
  const order = new Map(
    editingTitleOrder.value.map((id, index) => [id, index]),
  );
  return items.sort(
    (a, b) =>
      (order.get(a.id) ?? Number.MAX_SAFE_INTEGER) -
      (order.get(b.id) ?? Number.MAX_SAFE_INTEGER),
  );
});
const taskGroups = computed(() => {
  const result = new Map<
    string,
    { key: string; label: string; tasks: Task[]; sectionId?: string }
  >();
  if (viewPreferences.value.group_by === "section" && state.view === "list")
    for (const section of state.snapshot.sections
      .filter((s) => s.list_id === state.listId)
      .sort((a, b) => a.position - b.position))
      result.set(section.id, {
        key: section.id,
        label: section.name,
        tasks: [],
        sectionId: section.id,
      });
  for (const task of groupRootTasks(displayedTasks.value)) {
    let key = "",
      label = "";
    if (task.completed_at && !["completed", "trash"].includes(state.view)) {
      key = "completed";
      label = "Completed";
    } else if (task.pinned) {
      key = "pinned";
      label = "Pinned";
    } else if (viewPreferences.value.group_by === "date") {
      key = task.due_date
        ? task.due_date < localDate()
          ? "overdue"
          : task.due_date
        : "undated";
      label =
        key === "overdue"
          ? "Overdue"
          : key === "undated"
            ? "No Date"
            : dateLabel(key);
    } else if (viewPreferences.value.group_by === "priority") {
      key = `priority:${task.priority}`;
      label = [
        "No Priority",
        "Low Priority",
        "Medium Priority",
        "High Priority",
      ][task.priority]!;
    } else if (viewPreferences.value.group_by === "list") {
      key = task.list_id;
      label = state.snapshot.lists.find((l) => l.id === key)?.name || "Inbox";
    } else if (viewPreferences.value.group_by === "tag") {
      key = task.tag_ids?.[0] || "untagged";
      label = state.snapshot.tags.find((t) => t.id === key)?.name || "No Tag";
    } else if (viewPreferences.value.group_by === "section") {
      key = task.section_id || "unsectioned";
      label =
        state.snapshot.sections.find((s) => s.id === key)?.name || "Tasks";
    }
    if (!result.has(key)) result.set(key, { key, label, tasks: [] });
    result.get(key)!.tasks.push(task);
  }
  const groups = [...result.values()];
  groups.sort((a, b) =>
    a.key === "pinned"
      ? -1
      : b.key === "pinned"
        ? 1
        : a.key === "completed"
          ? 1
          : b.key === "completed"
            ? -1
            : viewPreferences.value.group_by === "date"
              ? a.key === "overdue"
                ? settings.value.overdue_position === "bottom"
                  ? 1
                  : -1
                : b.key === "overdue"
                  ? settings.value.overdue_position === "bottom"
                    ? -1
                    : 1
                  : a.key.localeCompare(b.key)
              : viewPreferences.value.group_by === "priority"
                ? b.key.localeCompare(a.key)
                : 0,
  );
  return groups;
});
const currentCommands = computed(() =>
  [
    { id: "quick_add", label: "Add Task", icon: PhPlus },
    { id: "search", label: "Search Tasks", icon: PhMagnifyingGlass },
    { id: "settings", label: "Settings", icon: PhGearSix },
    { id: "toggle_all_groups", label: "Toggle all groups", icon: PhList },
    {
      id: "toggle_all_subtasks",
      label: "Expand/Collapse all subtasks",
      icon: PhList,
    },
    ...smartViews.value.map((v) => ({
      id: v.id,
      label: `Go to ${v.name}`,
      icon: TaskNavigationGlyph,
      glyph: v.id,
    })),
    ...bottomViews.value.map((v) => ({
      id: v.id,
      label: `Go to ${v.name}`,
      icon: v.icon,
    })),
    ...(selected.value
      ? [
          { id: "complete_task", label: "Complete Task", icon: PhCheck },
          { id: "set_date", label: "Set Date", icon: PhCalendarBlank },
          { id: "pin_task", label: "Pin Task", icon: PhPushPin },
          { id: "duplicate_task", label: "Duplicate Task", icon: PhList },
          ...(canCreate.value &&
          !selected.value.completed_at &&
          !selected.value.wont_do_at &&
          !selected.value.deleted_at
            ? [{ id: "add_task_below", label: "Add task below", icon: PhPlus }]
            : []),
        ]
      : []),
  ].filter((c) =>
    c.label.toLowerCase().includes(commandQuery.value.toLowerCase()),
  ),
);
let poll: ReturnType<typeof setInterval>;
let unlistenDeepLink: UnlistenFn | undefined;
let unlistenOpenTask: UnlistenFn | undefined;
let unlistenSettingsSection: UnlistenFn | undefined;
let unlistenQuitCancelled: UnlistenFn | undefined;
let unlistenGlobalQuickAdd: UnlistenFn | undefined;
let unlistenIntegrationError: UnlistenFn | undefined;
let desktopIntegrationWrite: Promise<void> = Promise.resolve();
let quitGeneration = 0;
let unlistenQuit: UnlistenFn | undefined,
  unlistenCommands: UnlistenFn | undefined;
let disposed = false;
const quitting = ref(false);
const installingUpdate = ref(false);
let unlistenInstalling: UnlistenFn | undefined;
const activeQuitRound = ref<number | null>(null);
let quitInputCleanup: (() => void) | undefined;
let quitInputLocked = false;
function captureNativeEditorInput() {
  captureTaskTitleFields(quitInputLocked);
  editorRef.value?.reconcileEditorDrafts?.();
  const inlineDraft = addBelow.value;
  if (inlineDraft) {
    const input = document.querySelector<HTMLInputElement>(
      ".task-add-below input",
    );
    if (input) inlineDraft.title = input.value;
    // Blur can still deliver native composition input. Only commit once the
    // quit coordinator has drained it, including a draft in a hidden group.
    if (quitInputLocked && inlineDraft.title.trim() && !addingBelow.value)
      void createTaskBelow();
  }
}
async function drainNativeInput() {
  await nextTick();
  // WebKit's native text/composition events can follow the native quit menu
  // event. Let those events and Vue's resulting render run before flushing.
  // The fallback also works when a hidden window does not produce frames.
  await new Promise<void>((resolve) => {
    const fallback = window.setTimeout(resolve, 100);
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        window.clearTimeout(fallback);
        window.setTimeout(resolve, 0);
      }),
    );
  });
  await nextTick();
}
function resetQuit(reason = "") {
  quitGeneration++;
  quitting.value = false;
  installingUpdate.value = false;
  quitInputLocked = false;
  activeQuitRound.value = null;
  quitInputCleanup?.();
  quitInputCleanup = undefined;
  if (reason && reason !== "Quit cancelled") state.error = reason;
}
async function cancelNativeQuit(reason = "Quit cancelled") {
  const roundId = activeQuitRound.value;
  if (roundId === null) return;
  try {
    await invoke("cancel_quit", { reason, roundId });
  } catch (error) {
    state.error = String(error);
  }
}
function lockQuitKeyboard(event: KeyboardEvent) {
  if (!quitting.value) return;
  if (event.key === "Escape") void cancelNativeQuit();
  if (event.key === "Tab") {
    event.preventDefault();
    event.stopImmediatePropagation();
    document.querySelector<HTMLButtonElement>(".quit-overlay button")?.focus();
    return;
  }
  if (
    ["Enter", " "].includes(event.key) &&
    (event.target as HTMLElement).closest(".quit-overlay button")
  )
    return;
  event.preventDefault();
  event.stopImmediatePropagation();
}
function lockQuitBeforeInput(event: Event) {
  if (quitInputLocked) {
    event.preventDefault();
    event.stopImmediatePropagation();
  }
}
async function finishNativeQuit(roundId: number) {
  if (typeof roundId !== "number" || !Number.isSafeInteger(roundId)) return;
  if (quitting.value && activeQuitRound.value === roundId) return;
  quitInputCleanup?.();
  quitting.value = true;
  quitInputLocked = false;
  activeQuitRound.value = roundId;
  const generation = ++quitGeneration;
  const editorCheckpoint = editorWriteCheckpoint();
  const serviceCheckpoint = writeCheckpoint();
  let inputChanged = false,
    acknowledged = false;
  const trackInput = () => {
    inputChanged = true;
    if (acknowledged && generation === quitGeneration)
      void cancelNativeQuit(
        "A new edit arrived while quitting. Please try again.",
      );
  };
  document.addEventListener("input", trackInput, true);
  quitInputCleanup = () =>
    document.removeEventListener("input", trackInput, true);
  try {
    captureNativeEditorInput();
    if (document.activeElement instanceof HTMLElement)
      document.activeElement.blur();
    await drainNativeInput();
    if (generation !== quitGeneration) return;
    // Let the focused editor commit any native composition during blur first.
    // Once drained, no new input may enter while another window is saving.
    quitInputLocked = true;
    await nextTick();
    document.querySelector<HTMLButtonElement>(".quit-overlay button")?.focus();
    for (;;) {
      if (generation !== quitGeneration) return;
      inputChanged = false;
      captureNativeEditorInput();
      await waitForEditorWrites(editorCheckpoint);
      if (!(await flushAll(serviceCheckpoint))) {
        await cancelNativeQuit("Could not save pending changes.");
        return;
      }
      await drainNativeInput();
      await waitForEditorWrites(editorCheckpoint);
      captureNativeEditorInput();
      if (generation !== quitGeneration) return;
      if (inputChanged || Object.keys(state.drafts).length || state.busy)
        continue;
      acknowledged = true;
      await invoke("finish_quit", { roundId });
      // Keep the window locked until the native coordinator has received every
      // window's acknowledgement and exits, or broadcasts cancellation.
      return;
    }
  } catch (error) {
    if (generation === quitGeneration) await cancelNativeQuit(String(error));
  }
}
function mediaChange(event: MediaQueryListEvent) {
  systemDark.value = event.matches;
}
async function syncNativeAppearance() {
  if (!isNative) return;
  try {
    await invoke("sync_desktop_appearance");
  } catch (error) {
    state.error = `Could not update macOS appearance: ${String(error)}`;
  }
}
async function syncNativeIntegrations() {
  if (!isNative || quitting.value) return;
  try {
    await invoke("sync_desktop_integrations");
  } catch (error) {
    state.error = `Could not update macOS integrations: ${String(error)}`;
  }
}
async function consumeGlobalQuickAdd() {
  if (quitting.value || disposed) return;
  if (
    (state.loading ||
      !state.snapshot.lists.some((list) => list.id === "inbox")) &&
    !(await refresh())
  )
    return;
  if (quitting.value || disposed) return;
  try {
    const requested = await invoke<boolean>("take_global_quick_add");
    if (requested && (await openQuick())) {
      await nextTick();
      if (quitting.value || disposed || !quickOpen.value) return;
      document
        .querySelector<HTMLInputElement>('[aria-label="Quick task title"]')
        ?.focus();
    }
  } catch (error) {
    state.error = String(error);
  }
}
watch(
  () => [state.loading, state.snapshot.revision] as const,
  () => {
    if (
      isNative &&
      !standaloneSettings &&
      !standaloneReminders &&
      !state.loading
    ) {
      void syncNativeAppearance();
      void syncNativeIntegrations();
    }
  },
);
function focusRefresh() {
  void checkRevision();
}
function visibilityRefresh() {
  if (document.visibilityState === "visible") focusRefresh();
}
onMounted(() => {
  document.addEventListener("dragend", clearTaskDrag);
  document.addEventListener("drop", clearTaskDrag);
  document.addEventListener("pointerdown", beginTitlePointer, true);
  document.addEventListener("pointerup", releaseTitlePointer, true);
  document.addEventListener("pointercancel", endTitlePointer, true);
  document.addEventListener("click", endTitlePointer);
  void refresh();
  poll = setInterval(checkRevision, 2500);
  window.addEventListener("focus", focusRefresh);
  window.addEventListener("pageshow", focusRefresh);
  document.addEventListener("visibilitychange", visibilityRefresh);
  window.addEventListener("resize", viewportChanged);
  media.addEventListener("change", mediaChange);
  document.addEventListener("keydown", lockQuitKeyboard, true);
  document.addEventListener("beforeinput", lockQuitBeforeInput, true);
  if (isNative) {
    void listen("tasks-update-installing", () => {
      installingUpdate.value = true;
    }).then((u) => (disposed ? u() : (unlistenInstalling = u)));
    void listen<number>(
      "tasks-before-quit",
      (event) => void finishNativeQuit(event.payload),
    ).then((u) => (disposed ? u() : (unlistenQuit = u)));
    void listen<string>("tasks-quit-cancelled", (event) => {
      resetQuit(event.payload || "Could not save pending changes.");
    }).then((u) => (disposed ? u() : (unlistenQuitCancelled = u)));
    void listen<string>("desktop-integration-error", (event) => {
      state.error = event.payload;
    }).then((u) => (disposed ? u() : (unlistenIntegrationError = u)));
  }
  if (standaloneReminders) return;
  if (standaloneSettings) {
    const initial = new URLSearchParams(window.location.search).get("section");
    if (
      initial &&
      [
        "smart",
        "sync",
        "updates",
        "notifications",
        "date",
        "appearance",
        "more",
        "hotkeys",
        "about",
      ].includes(initial)
    )
      settingsSection.value = initial as typeof settingsSection.value;
    if (isNative)
      void listen<string>("settings-section", (event) => {
        if (
          [
            "smart",
            "sync",
            "updates",
            "notifications",
            "date",
            "appearance",
            "more",
            "hotkeys",
            "about",
          ].includes(event.payload)
        )
          settingsSection.value = event.payload as typeof settingsSection.value;
      }).then((u) => (disposed ? u() : (unlistenSettingsSection = u)));
    return;
  }
  window.addEventListener("keydown", keyboard);
  document.addEventListener("pointerdown", closeMenus);
  if (isNative) {
    void listen("global-quick-add", () => void consumeGlobalQuickAdd()).then(
      async (u) => {
        if (disposed) u();
        else {
          unlistenGlobalQuickAdd = u;
          await consumeGlobalQuickAdd();
        }
      },
    );
    void listen<string>(
      "tasks-open-task",
      (event) => void handleDeepLinks([`tasks://task/${event.payload}`]),
    ).then((u) => (disposed ? u() : (unlistenOpenTask = u)));
    void onOpenUrl((urls) => void handleDeepLinks(urls)).then((u) =>
      disposed ? u() : (unlistenDeepLink = u),
    );
    void getCurrent()
      .then((urls) => {
        if (urls) void handleDeepLinks(urls);
      })
      .catch((e) => (state.error = String(e)));

    void listen<string>("tasks-command", (event) => {
      void runCommand(event.payload);
    }).then((u) => (disposed ? u() : (unlistenCommands = u)));
  }
});
onBeforeUnmount(() => {
  document.removeEventListener("dragend", clearTaskDrag);
  document.removeEventListener("drop", clearTaskDrag);
  clearTaskDrag();
  document.removeEventListener("pointerdown", beginTitlePointer, true);
  document.removeEventListener("pointerup", releaseTitlePointer, true);
  document.removeEventListener("pointercancel", endTitlePointer, true);
  document.removeEventListener("click", endTitlePointer);
  if (titlePointerTimer) clearTimeout(titlePointerTimer);
  deferredTitleBlur = undefined;
  disposed = true;
  unlistenInstalling?.();
  unlistenQuit?.();
  unlistenDeepLink?.();
  unlistenOpenTask?.();
  unlistenSettingsSection?.();
  unlistenQuitCancelled?.();
  unlistenGlobalQuickAdd?.();
  unlistenIntegrationError?.();
  unlistenCommands?.();
  clearInterval(poll);
  window.removeEventListener("focus", focusRefresh);
  window.removeEventListener("pageshow", focusRefresh);
  document.removeEventListener("visibilitychange", visibilityRefresh);
  window.removeEventListener("resize", viewportChanged);
  window.removeEventListener("keydown", keyboard);
  document.removeEventListener("pointerdown", closeMenus);
  media.removeEventListener("change", mediaChange);
  document.removeEventListener("keydown", lockQuitKeyboard, true);
  document.removeEventListener("beforeinput", lockQuitBeforeInput, true);
  quitInputCleanup?.();
});
async function handleDeepLinks(urls: string[]) {
  if (quitting.value) return;
  await refresh();
  if (quitting.value) return;
  for (const value of urls) {
    let url: URL;
    try {
      url = new URL(value);
    } catch {
      continue;
    }
    if (
      url.protocol !== "tasks:" ||
      url.hostname !== "task" ||
      url.search ||
      url.hash
    )
      continue;
    const id = url.pathname.replace(/^\//, "");
    if (
      !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
        id,
      )
    )
      continue;
    const task = tasks.value.find((t) => t.id === id);
    if (!task) {
      state.error = "This task could not be found on this Mac.";
      continue;
    }
    navigate(
      task.deleted_at
        ? "trash"
        : task.wont_do_at
          ? "wont_do"
          : task.completed_at
            ? "completed"
            : "list",
      task.list_id,
    );
    state.selectedId = task.id;
    selectedIds.value = new Set([task.id]);
  }
}
function closeMenus(event: PointerEvent) {
  if (!(event.target as HTMLElement).closest(".popover-anchor"))
    menu.value = null;
}
watch(
  () => [state.view, state.listId],
  () => {
    captureTaskTitleFields(true);
    editingTitleId.value = null;
    selectedIds.value = new Set();
    collapsedGroups.value = new Set();
    subtaskExpansion.value = null;
    addBelow.value = null;
    context.value = null;
  },
);
watch(
  () => state.selectedId,
  (id) => {
    if (editingTitleId.value && id !== editingTitleId.value) {
      captureTaskTitleFields(true);
      editingTitleId.value = null;
    }
  },
);
async function search() {
  captureTaskTitleFields(true);
  menu.value = null;
  context.value = null;
  searchOpen.value = true;
}
function openSearchResult(target: SearchTarget) {
  searchOpen.value = false;
  if (target.kind === "task") {
    const task = tasksById.value.get(target.id);
    if (!task) return;
    navigate("list", task.list_id);
    state.selectedId = task.id;
  } else navigate(target.kind, target.id);
}
function handleBatchPaste(event: ClipboardEvent) {
  const text = event.clipboardData?.getData("text/plain") || "";
  if (text.split(/\r?\n/).filter((line) => line.trim()).length < 2) return;
  event.preventDefault();
  batchPaste.value = { text, fromQuick: false };
}
const batchDefaults = computed(() =>
  batchPaste.value?.fromQuick
    ? {
        list_id: quickPreviewList.value,
        ...quickDatePatch.value,
      }
    : {
        list_id:
          state.view === "list"
            ? state.listId
            : state.view === "inbox"
              ? "inbox"
              : settings.value.default_list_id || "inbox",
        ...(["today", "tomorrow"].includes(state.view)
          ? { due_date: state.view === "today" ? localDate() : tomorrowDate() }
          : {}),
        ...(state.view === "tag" && state.listId
          ? { tag_ids: [state.listId] }
          : {}),
      },
);
const batchOverrides = computed(() =>
  batchPaste.value?.fromQuick
    ? {
        ...(quickDateEdited.value ? quickPreviewDate.value : {}),
        ...(quickDateEdited.value && quickPreviewDate.value.due_date === null
          ? {
              due_time: null,
              start_date: null,
              start_time: null,
              duration_minutes: null,
              recurrence: null,
              reminders: [],
            }
          : {}),
        ...(quickPriorityEdited.value
          ? { priority: quickPreviewPriority.value }
          : {}),
      }
    : {},
);
function batchCreated(ids: string[]) {
  if (batchPaste.value?.fromQuick) quickOpen.value = false;
  else newTitle.value = "";
  batchPaste.value = null;
  const task = tasks.value.find((t) => t.id === ids[0]);
  if (task) {
    navigate("list", task.list_id);
    state.selectedId = task.id;
  }
}
const summaryPresets = computed(
  () => (settings.value.summary_templates || []) as SummaryPreset[],
);
async function saveSummaryPresets(value: SummaryPreset[]) {
  try {
    await mutate("settings.set", { key: "summary_templates", value });
  } catch {}
}
async function createSummaryNote(input: {
  title: string;
  notes: string;
  notes_format: "markdown";
  is_note: true;
}) {
  try {
    const task = await mutate<Task>("tasks.create", {
      ...input,
      list_id: settings.value.default_list_id || "inbox",
    });
    navigate("list", task.list_id);
    state.selectedId = task.id;
  } catch {}
}
async function updateTimelineTask(id: string, patch: Record<string, unknown>) {
  try {
    await mutate("tasks.update", { id, ...patch });
  } catch {}
}
async function createTimelineTask(input: { title: string; due_date: string }) {
  try {
    await mutate("tasks.create", {
      ...input,
      list_id:
        state.view === "list"
          ? state.listId || "inbox"
          : settings.value.default_list_id || "inbox",
    });
  } catch {}
}
let openingQuick: Promise<boolean> | undefined;
async function openQuick(): Promise<boolean> {
  if (quitting.value || disposed) return false;
  if (quickOpen.value) return true;
  if (openingQuick) return openingQuick;
  const opening = (async () => {
    // Join the initial snapshot before capturing defaults. An earlier failed
    // load also needs a retry; Inbox exists in every successful snapshot.
    if (
      (state.loading ||
        !state.snapshot.lists.some((list) => list.id === "inbox")) &&
      !(await refresh())
    )
      return false;
    if (quitting.value || disposed) return false;
    if (quickOpen.value) return true;
    quickTitle.value = "";
    const defaultDate =
      state.view === "today" || state.view === "tomorrow"
        ? state.view
        : settings.value.default_date;
    quickDatePatch.value = {
      due_date:
        defaultDate === "today"
          ? localDate()
          : defaultDate === "tomorrow"
            ? tomorrowDate()
            : null,
    };
    const targetList =
      state.view === "list" && state.listId
        ? state.listId
        : String(settings.value.default_list_id || "inbox");
    quickList.value = activeLists.value.some((list) => list.id === targetList)
      ? targetList
      : "inbox";
    quickPriority.value = Number(settings.value.default_priority || 0);
    quickDateEdited.value = false;
    quickPriorityEdited.value = false;
    quickListEdited.value = false;
    quickOpen.value = true;
    return true;
  })();
  openingQuick = opening;
  try {
    return await opening;
  } finally {
    if (openingQuick === opening) openingQuick = undefined;
  }
}
async function createTask(quick = false) {
  const value = (quick ? quickTitle.value : newTitle.value).trim();
  if (!value || creating.value) return;
  creating.value = true;
  try {
    const input: Record<string, unknown> = {
      ...parseTaskInput(
        value,
        settings.value,
        activeLists.value,
        state.snapshot.tags,
      ),
    };
    if (quick) {
      input.list_id = quickPreviewList.value;
      input.priority = quickPreviewPriority.value;
      Object.assign(input, quickPreviewDate.value);
    } else {
      if (!input.list_id)
        input.list_id =
          state.view === "list"
            ? state.listId
            : state.view === "inbox"
              ? "inbox"
              : String(settings.value.default_list_id || "inbox");
      if (!input.due_date && ["today", "tomorrow"].includes(state.view))
        input.due_date = state.view === "today" ? localDate() : tomorrowDate();
      if (state.view === "tag" && state.listId)
        input.tag_ids = [
          ...new Set([
            ...(Array.isArray(input.tag_ids) ? input.tag_ids : []),
            state.listId,
          ]),
        ];
    }
    const task = await mutate<Task>("tasks.create", input);
    if (quick) {
      quickOpen.value = false;
      navigate("list", task.list_id);
    } else newTitle.value = "";
    state.selectedId = task.id;
    selectedIds.value = new Set([task.id]);
    await nextTick();
    if (!quick) addInput.value?.focus();
  } catch {
  } finally {
    creating.value = false;
  }
}
async function openAddBelow() {
  const task = selected.value;
  if (
    quitting.value ||
    addingBelow.value ||
    !canCreate.value ||
    !task ||
    task.deleted_at ||
    task.completed_at ||
    task.wont_do_at ||
    !activeLists.value.some((list) => list.id === task.list_id)
  )
    return;
  const ancestors = new Set([task.id]);
  let parent = task.parent_id;
  while (parent && !ancestors.has(parent)) {
    ancestors.add(parent);
    parent = tasksById.value.get(parent)?.parent_id || null;
  }
  const group = taskGroups.value.find((group) =>
    group.tasks.some((item) => ancestors.has(item.id)),
  );
  if (!group) return;
  const closed = new Set(collapsedGroups.value);
  closed.delete(group.key);
  collapsedGroups.value = closed;
  addBelow.value = { anchorId: task.id, title: "" };
  await nextTick();
}
function cancelAddBelow() {
  if (!addingBelow.value) addBelow.value = null;
}
async function createTaskBelow() {
  const draft = addBelow.value;
  const title = draft?.title.trim();
  const anchor = draft && tasksById.value.get(draft.anchorId);
  if (!draft || !title || addingBelow.value) return;
  if (!anchor) {
    const error = new Error(
      "The selected task is no longer available. Your new task has not been added.",
    );
    state.error = error.message;
    beginEditorWrite()(error);
    return;
  }
  const target = {
    id: anchor.id,
    listId: anchor.list_id,
    parentId: anchor.parent_id,
    sectionId: anchor.section_id,
  };
  addingBelow.value = true;
  const finishWork = beginEditorWrite();
  let failure: unknown;
  let created: Task | undefined;
  try {
    // A blur can submit this draft while the same click changes the view.
    // Capture recognition and context before waiting for pending writes.
    const input: Record<string, unknown> = {
      ...parseTaskInput(
        title,
        settings.value,
        activeLists.value,
        state.snapshot.tags,
      ),
      list_id: target.listId,
      parent_id: target.parentId,
      section_id: target.sectionId,
    };
    if (!input.due_date && ["today", "tomorrow"].includes(state.view))
      input.due_date = state.view === "today" ? localDate() : tomorrowDate();
    if (state.view === "tag" && state.listId)
      input.tag_ids = [
        ...new Set([
          ...(Array.isArray(input.tag_ids) ? input.tag_ids : []),
          state.listId,
        ]),
      ];
    editorRef.value?.reconcileEditorDrafts();
    if (!(await flushAll()))
      throw new Error(
        "Could not save pending changes. Your new task has not been added.",
      );
    if (!(await refresh(true)))
      throw new Error(
        "Could not verify the selected task. Your new task has not been added.",
      );
    const current = tasksById.value.get(target.id);
    if (
      !current ||
      current.deleted_at ||
      current.completed_at ||
      current.wont_do_at ||
      current.list_id !== target.listId ||
      current.parent_id !== target.parentId ||
      current.section_id !== target.sectionId ||
      !activeLists.value.some((list) => list.id === target.listId)
    )
      throw new Error(
        "The selected task moved or is no longer open. Choose its new position before adding a task.",
      );
    created = await mutate<Task>("tasks.create", input);
    const latestAnchor = tasksById.value.get(target.id);
    if (
      !latestAnchor ||
      latestAnchor.deleted_at ||
      latestAnchor.list_id !== created.list_id ||
      latestAnchor.parent_id !== created.parent_id ||
      latestAnchor.section_id !== target.sectionId
    )
      throw new Error(
        "The task was added, but the original task moved before its position could be set.",
      );
    const siblings = tasks.value
      .filter(
        (task) =>
          task.list_id === created!.list_id &&
          task.parent_id === created!.parent_id &&
          task.id !== created!.id,
      )
      .sort(
        (a, b) =>
          a.position - b.position ||
          a.created_at.localeCompare(b.created_at) ||
          a.id.localeCompare(b.id),
      )
      .map((task) => task.id);
    const index = siblings.indexOf(target.id);
    if (index < 0)
      throw new Error(
        "The task was added, but its original position is no longer available.",
      );
    siblings.splice(index + 1, 0, created.id);
    await mutate("tasks.reorder", { ids: siblings });
  } catch (error) {
    failure = error;
    state.error = created
      ? `Task added, but its position could not be updated: ${String(error)}`
      : String(error);
  } finally {
    addingBelow.value = false;
    if (created) {
      if (addBelow.value === draft) addBelow.value = null;
      if (state.selectedId === target.id) {
        state.selectedId = created.id;
        selectedIds.value = new Set([created.id]);
        await nextTick();
        document
          .querySelector<HTMLButtonElement>(
            `[data-task-id="${created.id}"] .task-row-title`,
          )
          ?.focus();
      }
    }
    finishWork(failure);
  }
}
function toggleAllGroups() {
  const groups = taskGroups.value.filter((group) => group.label);
  const collapse = groups.some(
    (group) => !collapsedGroups.value.has(group.key),
  );
  collapsedGroups.value = collapse
    ? new Set(groups.map((group) => group.key))
    : new Set();
}
function toggleAllSubtasks() {
  const hasExpanded = !!document.querySelector(
    '.task-list-scroll .subtask-disclosure[aria-expanded="true"]',
  );
  subtaskExpansion.value = {
    revision: (subtaskExpansion.value?.revision || 0) + 1,
    expanded: !hasExpanded,
  };
}
async function postponeGroup(group: { tasks: Task[] }) {
  if (postponing.value || quitting.value) return;
  captureTaskTitleFields(true);
  const today = localDate();
  const targets = group.tasks
    .filter(
      (task) =>
        task.due_date &&
        task.due_date < today &&
        !task.completed_at &&
        !task.deleted_at &&
        !task.wont_do_at,
    )
    .map((task) => ({ id: task.id, expected_version: task.version }));
  if (!targets.length) return;
  postponing.value = true;
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    editorRef.value?.reconcileEditorDrafts();
    for (const target of targets)
      await mutate("tasks.update", { ...target, due_date: today });
  } catch (error) {
    failure = error;
    state.error = String(error);
  } finally {
    postponing.value = false;
    finishWork(failure);
  }
}
function playCompletion() {
  if (settings.value.completion_sound === false) return;
  try {
    const audio = new AudioContext();
    const gain = audio.createGain();
    gain.connect(audio.destination);
    gain.gain.setValueAtTime(0.06, audio.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.001, audio.currentTime + 0.28);
    [880, 1174.66].forEach((f, i) => {
      const o = audio.createOscillator();
      o.frequency.value = f;
      o.connect(gain);
      o.start(audio.currentTime + i * 0.08);
      o.stop(audio.currentTime + 0.28);
    });
    setTimeout(() => void audio.close(), 350);
  } catch {}
}
async function complete(task: Task) {
  if (task.is_note) return;
  captureTaskTitleFields(true);
  try {
    if (task.wont_do_at)
      await mutate("tasks.wont_do", { id: task.id, wont_do: false });
    else {
      await mutate("tasks.complete", {
        id: task.id,
        completed: !task.completed_at,
      });
      if (!task.completed_at) playCompletion();
    }
  } catch {}
}
async function restore(id: string) {
  try {
    await mutate("tasks.restore", { id });
  } catch {}
}
function handleMutate(action: string, input: Record<string, unknown>) {
  void mutate<{ id?: string }>(action, input)
    .then((result) => {
      if (action === "lists.duplicate" && result.id)
        navigate("list", result.id);
    })
    .catch(() => {});
}
async function reorderContainer(input: {
  kind: "folder" | "list";
  id: string;
  parentId: string | null;
  targetId: string;
  zone: "before" | "after";
}) {
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    const { kind, id, parentId, targetId, zone } = input;
    const container =
      kind === "folder"
        ? state.snapshot.folders.find((folder) => folder.id === id)
        : state.snapshot.lists.find((list) => list.id === id);
    if (!container) return;
    const oldParent =
      "parent_id" in container ? container.parent_id : container.folder_id;
    if (oldParent !== parentId)
      await mutate(`${kind}s.update`, {
        id,
        [kind === "folder" ? "parent_id" : "folder_id"]: parentId,
      });
    const siblings =
      kind === "folder"
        ? state.snapshot.folders.filter(
            (folder) => folder.parent_id === parentId,
          )
        : state.snapshot.lists.filter(
            (list) => list.folder_id === parentId && list.id !== "inbox",
          );
    const ids = siblings
      .sort((a, b) => a.position - b.position || a.name.localeCompare(b.name))
      .map((item) => item.id)
      .filter((item) => item !== id);
    const index = ids.indexOf(targetId);
    if (index < 0) return;
    ids.splice(index + (zone === "after" ? 1 : 0), 0, id);
    await mutate(`${kind}s.reorder`, { ids });
  } catch (error) {
    failure = error;
  } finally {
    finishWork(failure);
  }
}
function renderedTasks(): Task[] {
  // The tree owns its disclosure state. Read the rendered order so keyboard
  // selection also includes expanded descendants and excludes closed groups.
  return [
    ...(taskListElement.value?.querySelectorAll<HTMLElement>(
      ".task-tree-node[data-task-id]",
    ) || []),
  ]
    .map((element) => tasksById.value.get(element.dataset.taskId!))
    .filter((task): task is Task => !!task);
}
function selectTask(id: string, event?: MouseEvent) {
  captureTaskTitleFields(true);
  if (event?.shiftKey && state.selectedId) {
    const rows = renderedTasks();
    const start = rows.findIndex((t) => t.id === state.selectedId),
      end = rows.findIndex((t) => t.id === id);
    selectedIds.value = new Set(
      start < 0 || end < 0
        ? [id]
        : rows
            .slice(Math.min(start, end), Math.max(start, end) + 1)
            .map((t) => t.id),
    );
  } else if (event?.metaKey || event?.ctrlKey) {
    const ids = new Set(selectedIds.value);
    ids.has(id) ? ids.delete(id) : ids.add(id);
    selectedIds.value = ids;
  } else selectedIds.value = new Set([id]);
  state.selectedId = id;
}
function editTitleInRow(id: string, event: MouseEvent) {
  selectTask(id, event);
  if (event.metaKey || event.ctrlKey || event.shiftKey) {
    editingTitleId.value = null;
    return;
  }
  const task = tasksById.value.get(id);
  // On compact screens the existing detail pane covers the list. Keep its
  // visible title editor instead of focusing a field behind that pane.
  if (quitting.value || !task || task.deleted_at || window.innerWidth < 760)
    return;
  editingTitleOrder.value = visibleTasks.value.map((task) => task.id);
  editingTitleId.value = id;
}
async function finishRowTitle(
  id: string,
  action: "blur" | "escape" | "below" | "subtask",
) {
  if (quitting.value || finishingTitleId.value || editingTitleId.value !== id)
    return;
  captureTaskTitleFields();
  if (action === "blur" && tasksById.value.get(id)?.title.trim()) {
    const finish = () => {
      if (editingTitleId.value === id) editingTitleId.value = null;
    };
    if (titlePointerActive) deferredTitleBlur = finish;
    else finish();
    return;
  }
  finishingTitleId.value = id;
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    if (!(await flushAll())) {
      failure = new Error("Could not save pending title changes.");
      return;
    }
    if (editingTitleId.value !== id || state.selectedId !== id) return;
    editingTitleId.value = null;
    await nextTick();
    if (action === "below") await openAddBelow();
    else if (action === "subtask") await editorRef.value?.focusSubtask();
    else if (action === "escape")
      document
        .querySelector<HTMLButtonElement>(
          `[data-task-id="${id}"] > .task-tree-line .task-row-title`,
        )
        ?.focus();
  } catch (error) {
    failure = error;
    state.error = String(error);
  } finally {
    finishingTitleId.value = null;
    finishWork(failure);
  }
}
function taskContext(event: MouseEvent, task: Task) {
  if (!selectedIds.value.has(task.id)) selectTask(task.id);
  context.value = { task, x: event.clientX, y: event.clientY };
}
async function taskAction(action: string, value?: unknown) {
  captureTaskTitleFields(true);
  const task = context.value?.task || selected.value;
  if (!task) return;
  const requestedIds = selectedIds.value.has(task.id)
    ? [...selectedIds.value]
    : [task.id];
  const actionTasks = requestedIds
    .map((id) => tasksById.value.get(id))
    .filter(
      (item): item is Task =>
        !!item && (action !== "complete" || !item.is_note),
    );
  const roots = [
    "complete",
    "duplicate",
    "move",
    "delete",
    "restore",
    "wont_do",
  ].includes(action)
    ? groupRootTasks(actionTasks)
    : actionTasks;
  const ids = roots.map((item) => item.id);
  context.value = null;
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    if (["date", "tags", "subtask"].includes(action)) {
      if ((action === "date" || action === "tags") && requestedIds.length > 1) {
        bulkError.value = "";
        bulkDialog.value = { mode: action, task, ids: requestedIds };
        return;
      }
      state.selectedId = task.id;
      await nextTick();
      if (action === "date") editorRef.value?.openDate?.();
      if (action === "tags") editorRef.value?.openTags?.();
      if (action === "subtask") editorRef.value?.focusSubtask?.();
      return;
    }
    if (action === "copy_link") {
      await navigator.clipboard.writeText(`tasks://task/${task.id}`);
      return;
    }
    if (action === "template") {
      templateName.value = task.title;
      templateTaskId.value = task.id;
      templateDialog.value = true;
      return;
    }
    for (const id of ids) {
      if (action === "complete") {
        if (task.wont_do_at)
          await mutate("tasks.wont_do", { id, wont_do: false });
        else
          await mutate("tasks.complete", { id, completed: !task.completed_at });
      }
      if (action === "restore") await restore(id);
      if (action === "delete") await mutate("tasks.delete", { id });
      if (action === "wont_do")
        await mutate("tasks.wont_do", { id, wont_do: !task.wont_do_at });
      if (action === "convert")
        await mutate("tasks.update", { id, is_note: !task.is_note });
      if (action === "priority")
        await mutate("tasks.update", { id, priority: Number(value) });
      if (action === "pin")
        await mutate("tasks.update", { id, pinned: !task.pinned });
      if (action === "move")
        await mutate("tasks.update", {
          id,
          list_id: String(value),
          parent_id: null,
        });
      if (action === "duplicate") {
        const clone = await mutate<Task>("tasks.duplicate", { id });
        state.selectedId = clone.id;
      }
    }
    if (
      action === "complete" &&
      ids.length &&
      !task.completed_at &&
      !task.wont_do_at
    )
      playCompletion();
    if (action === "delete") state.selectedId = null;
  } catch (error) {
    failure = error;
  } finally {
    finishWork(failure);
  }
}
async function applyBulkDate(patch: Record<string, unknown>) {
  if (!bulkDialog.value || bulkBusy.value) return;
  bulkBusy.value = true;
  bulkError.value = "";
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    for (const id of bulkDialog.value.ids)
      await mutate("tasks.update", { id, ...patch });
    bulkDialog.value = null;
  } catch (error) {
    failure = error;
    bulkError.value = String(error);
  } finally {
    bulkBusy.value = false;
    finishWork(failure);
  }
}
async function applyBulkTags(changes: Record<string, boolean>) {
  if (!bulkDialog.value || bulkBusy.value) return;
  bulkBusy.value = true;
  bulkError.value = "";
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    for (const id of bulkDialog.value.ids) {
      const task = tasksById.value.get(id);
      if (!task) continue;
      const ids = new Set(task.tag_ids);
      for (const [tagId, include] of Object.entries(changes))
        include ? ids.add(tagId) : ids.delete(tagId);
      await mutate("tasks.update", { id, tag_ids: [...ids] });
    }
    bulkDialog.value = null;
  } catch (error) {
    failure = error;
    bulkError.value = String(error);
  } finally {
    bulkBusy.value = false;
    finishWork(failure);
  }
}
async function dropTask(event: DragEvent, target: Task) {
  if (!canReorder.value) return;
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    const payload = JSON.parse(
      event.dataTransfer?.getData("application/x-tasks") || "{}",
    );
    const source = tasks.value.find((t) => t.id === payload.id);
    if (payload.kind !== "task" || !source || !canDropTask(source, target))
      return;
    const ids = tasks.value
      .filter(
        (t) =>
          t.list_id === target.list_id &&
          t.parent_id === target.parent_id &&
          !t.deleted_at &&
          !t.wont_do_at,
      )
      .sort((a, b) => a.position - b.position)
      .map((t) => t.id)
      .filter((id) => id !== source.id);
    const after = taskDropPosition(event) === "after";
    ids.splice(ids.indexOf(target.id) + (after ? 1 : 0), 0, source.id);
    await mutate("tasks.reorder", { ids });
    if (source.section_id !== target.section_id)
      await mutate("tasks.update", {
        id: source.id,
        section_id: target.section_id,
      });
  } catch (error) {
    failure = error;
  } finally {
    finishWork(failure);
  }
}
async function dropSection(event: DragEvent, sectionId: string | null) {
  try {
    const data = JSON.parse(
      event.dataTransfer?.getData("application/x-tasks") || "{}",
    );
    if (data.kind === "task" && state.listId)
      await mutate("tasks.update", {
        id: data.id,
        list_id: state.listId,
        section_id: sectionId,
      });
  } catch {}
}
function createContainer(input: {
  kind: "folder" | "list";
  parentId: string | null;
}) {
  menu.value = null;
  managementError.value = "";
  management.value = {
    ...input,
    id: null,
    name: "",
    color: "#6476b4",
    deleting: false,
  };
}
function editContainer(input: { kind: "folder" | "list"; id: string }) {
  const item =
    input.kind === "folder"
      ? state.snapshot.folders.find((f) => f.id === input.id)
      : state.snapshot.lists.find((l) => l.id === input.id);
  if (!item) return;
  managementError.value = "";
  management.value = {
    ...input,
    name: item.name,
    parentId: "parent_id" in item ? item.parent_id : item.folder_id,
    color: "color" in item ? item.color : "#6476b4",
    deleting: false,
  };
  menu.value = null;
}
function removeContainer(input: { kind: "folder" | "list"; id: string }) {
  editContainer(input);
  if (management.value) management.value.deleting = true;
}
async function saveContainer(value: Record<string, unknown>) {
  const v = management.value;
  if (!v || !String(value.name || "").trim() || managementBusy.value) return;
  managementBusy.value = true;
  managementError.value = "";
  try {
    const item = await mutate<{ id: string }>(
      `${v.kind === "list" ? "lists" : "folders"}.${v.id ? "update" : "create"}`,
      { ...value, ...(v.id ? { id: v.id } : {}) },
    );
    management.value = null;
    if (v.kind === "list") navigate("list", item.id);
  } catch (error) {
    managementError.value = String(error);
  } finally {
    managementBusy.value = false;
  }
}
async function deleteContainer() {
  const v = management.value;
  if (!v?.id) return;
  managementBusy.value = true;
  try {
    await mutate(`${v.kind === "list" ? "lists" : "folders"}.delete`, {
      id: v.id,
    });
    management.value = null;
    if (state.listId === v.id) navigate("inbox");
  } catch {
  } finally {
    managementBusy.value = false;
  }
}
async function updateSetting(key: string, value: unknown) {
  if (quitting.value) return;
  if (
    isNative &&
    [
      "launch_at_login",
      "launch_minimized",
      "global_quick_add_shortcut",
      "global_show_hide_shortcut",
    ].includes(key)
  ) {
    const finishWork = beginEditorWrite();
    let failure: unknown;
    const work = desktopIntegrationWrite.then(async () => {
      await invoke("set_desktop_integration_setting", { key, value });
      if (!(await refresh(true)))
        throw new Error(
          "The setting was saved, but the updated settings could not be loaded.",
        );
    });
    desktopIntegrationWrite = work.catch(() => {});
    try {
      await work;
    } catch (error) {
      failure = error;
      state.error = String(error);
    } finally {
      finishWork(failure);
    }
    return;
  }
  try {
    if (key === "notifications" && value === true && isNative) {
      let permission = await isPermissionGranted();
      if (!permission) permission = (await requestPermission()) === "granted";
      if (!permission) {
        state.error =
          "Allow notifications for Tasks in macOS System Settings, then try again.";
        return;
      }
    }
    await mutate("settings.set", { key, value });
    if (isNative && ["app_icon", "show_dock_icon", "dock_count"].includes(key))
      await syncNativeAppearance();
  } catch {}
}
async function notificationSettings() {
  if (isNative)
    try {
      await invoke("open_notification_settings");
    } catch (e) {
      state.error = String(e);
    }
  else state.error = "Notification settings are available in the macOS app.";
}
async function saveTag() {
  const v = tagDialog.value;
  if (!v?.name.trim()) return;
  try {
    if (v.deleting && v.id) await mutate("tags.delete", { id: v.id });
    else
      await mutate(`tags.${v.id ? "update" : "create"}`, {
        ...(v.id ? { id: v.id } : {}),
        name: v.name.trim(),
        color: v.color,
      });
    tagDialog.value = null;
  } catch {}
}
function openFilter(filter?: SavedFilter) {
  filterDialog.value = filter
    ? JSON.parse(JSON.stringify(filter))
    : {
        id: crypto.randomUUID(),
        name: "",
        list_ids: [],
        tag_ids: [],
        priorities: [],
        date: "all",
        match: "all",
      };
}
async function saveFilter(remove = false) {
  const v = filterDialog.value;
  if (!v || !v.name.trim()) return;
  const next = savedFilters.value.filter((f) => f.id !== v.id);
  if (!remove) next.push({ ...v, name: v.name.trim() });
  await updateSetting("saved_filters", next);
  filterDialog.value = null;
  if (remove && state.listId === v.id) navigate("inbox");
}
async function saveSection() {
  const v = sectionDialog.value;
  if (!v?.name.trim() || !state.listId) return;
  try {
    await mutate(`sections.${v.id ? "update" : "create"}`, {
      ...(v.id ? { id: v.id } : { list_id: state.listId }),
      name: v.name.trim(),
    });
    await setViewPreferences({ group_by: "section" });
    sectionDialog.value = null;
  } catch {}
}
async function saveTemplate() {
  if (!templateTaskId.value || !templateName.value.trim()) return;
  try {
    await mutate("templates.create", {
      name: templateName.value.trim(),
      task_id: templateTaskId.value,
    });
    templateDialog.value = false;
  } catch {}
}
async function applyTemplate(id: string) {
  try {
    const task = await mutate<Task>("templates.apply", {
      id,
      list_id: state.view === "list" ? state.listId : "inbox",
    });
    if (standaloneSettings && isNative) {
      await emitTo("main", "tasks-open-task", task.id);
      await closeSettings();
    } else {
      navigate("list", task.list_id);
      state.selectedId = task.id;
      settingsOpen.value = false;
    }
    menu.value = null;
  } catch {}
}
async function templateAction(
  action: "apply" | "rename" | "delete",
  id: string,
  name?: string,
) {
  if (quitting.value) return;
  if (action === "apply") return applyTemplate(id);
  try {
    await mutate(
      action === "rename" ? "templates.update" : "templates.delete",
      action === "rename" ? { id, name } : { id },
    );
  } catch {}
}
function toggleGroup(key: string) {
  const next = new Set(collapsedGroups.value);
  next.has(key) ? next.delete(key) : next.add(key);
  collapsedGroups.value = next;
}
async function preference(patch: Partial<ViewPreferences>) {
  if (patch.view_mode === "board" && viewPreferences.value.group_by === "none")
    patch.group_by = state.view === "list" ? "section" : "priority";
  try {
    await setViewPreferences(patch);
  } catch {}
}
// Rail shortcuts reuse existing views; navigation never changes task data.
const railRefreshing = ref(false);
async function refreshFromRail() {
  if (railRefreshing.value) return;
  railRefreshing.value = true;
  try {
    await refresh();
  } finally {
    railRefreshing.value = false;
  }
}
async function openTaskList() {
  navigate("inbox");
  await preference({ view_mode: "list" });
}
async function openBoardView() {
  state.selectedId = null;
  await preference({ view_mode: "board" });
}
function openSettings(section: typeof settingsSection.value = "smart") {
  settingsSection.value = section;
  if (isNative) {
    void invoke("open_settings_window", { section }).catch(
      (e) => (state.error = String(e)),
    );
    return;
  }
  settingsOpen.value = true;
}
function toggleSidebar() {
  if (window.innerWidth < 760) state.sidebarOpen = !state.sidebarOpen;
  else desktopSidebarHidden.value = !desktopSidebarHidden.value;
}
async function closeSettings() {
  if (!(await flushAll())) return;
  if (standaloneSettings && isNative)
    void invoke("close_settings_window").catch(
      (e) => (state.error = String(e)),
    );
  else settingsOpen.value = false;
}
async function runCommand(id: string) {
  if (quitting.value) return;
  commandOpen.value = false;
  if (id === "quick_add") await openQuick();
  else if (id === "search") await search();
  else if (id === "settings") openSettings();
  else if (id === "check_updates") {
    openSettings("updates");
    void invoke("check_for_updates").catch((e) => (state.error = String(e)));
  } else if (id === "shortcuts") openSettings("hotkeys");
  else if (
    [
      "today",
      "tomorrow",
      "upcoming",
      "inbox",
      "all",
      "completed",
      "wont_do",
      "trash",
    ].includes(id)
  )
    navigate(id as View);
  else if (id === "complete_task" && selected.value)
    await taskAction("complete");
  else if (id === "delete_task") await taskAction("delete");
  else if (id === "pin_task") await taskAction("pin");
  else if (id === "duplicate_task") await taskAction("duplicate");
  else if (id === "add_subtask") editorRef.value?.focusSubtask?.();
  else if (id === "add_task_below") await openAddBelow();
  else if (id === "toggle_all_groups") toggleAllGroups();
  else if (id === "toggle_all_subtasks") toggleAllSubtasks();
  else if (id === "set_date") await taskAction("date");
  else if (id === "list_view") await preference({ view_mode: "list" });
  else if (id === "kanban_view") await preference({ view_mode: "board" });
  else if (id === "print_task" && selected.value)
    await editorRef.value?.printTask?.();
  else if (selected.value) {
    const patch: Record<string, unknown> = {};
    if (id === "clear_date")
      Object.assign(patch, {
        due_date: null,
        due_time: null,
        start_date: null,
        start_time: null,
        duration_minutes: null,
        reminders: [],
        recurrence: null,
      });
    else if (["set_today", "set_tomorrow", "set_next_week"].includes(id)) {
      const d = new Date();
      d.setDate(
        d.getDate() +
          (id === "set_tomorrow" ? 1 : id === "set_next_week" ? 7 : 0),
      );
      patch.due_date = localDate(d);
    } else if (id.startsWith("priority_"))
      patch.priority = {
        priority_none: 0,
        priority_low: 1,
        priority_medium: 2,
        priority_high: 3,
      }[id];
    if (Object.keys(patch).length) {
      const ids = selectedIds.value.has(selected.value.id)
        ? [...selectedIds.value]
        : [selected.value.id];
      const finishWork = beginEditorWrite();
      let failure: unknown;
      try {
        for (const taskId of ids)
          await mutate("tasks.update", { id: taskId, ...patch });
      } catch (error) {
        failure = error;
      } finally {
        finishWork(failure);
      }
    }
  }
}
let keySequence = "",
  keyTime = 0;
function keyboard(event: KeyboardEvent) {
  if (event.defaultPrevented) return;
  const target = event.target as HTMLElement;
  const input =
    target?.matches("input,textarea,select") || target?.isContentEditable;
  const modal =
    searchOpen.value ||
    !!batchPaste.value ||
    quickOpen.value ||
    settingsOpen.value ||
    management.value ||
    tagDialog.value ||
    filterDialog.value ||
    sectionDialog.value ||
    templateDialog.value ||
    bulkDialog.value ||
    commandOpen.value;
  if (event.key === "Escape") {
    menu.value = null;
    context.value = null;
    if (!modal) {
      if (window.innerWidth < 760) {
        state.selectedId = null;
        selectedIds.value = new Set();
      }
      state.sidebarOpen = false;
    }
    return;
  }
  if (modal) return;
  if (
    event.key === "Enter" &&
    !event.isComposing &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.shiftKey &&
    !input &&
    selected.value
  ) {
    const button = target?.closest("button");
    const selectedTitle =
      button?.matches(".task-row-title") &&
      button.closest<HTMLElement>("[data-task-id]")?.dataset.taskId ===
        selected.value.id;
    if (
      (!button || selectedTitle) &&
      !document.querySelector(
        '[role="menu"],.task-date-picker,[role="dialog"][aria-modal="true"]',
      )
    ) {
      event.preventDefault();
      void openAddBelow();
      return;
    }
  }
  const chord = keyChord(event);
  if (!chord) return;
  const hotkeys = {
    ...defaultHotkeys,
    ...((settings.value.hotkeys || {}) as Record<string, string>),
  };
  const direct = Object.entries(hotkeys).find(
    ([, value]) =>
      value === chord &&
      (!input ||
        chord.includes("mod+") ||
        (chord === "shift+enter" &&
          !!selected.value &&
          target.matches(".editor-title"))),
  );
  if (direct) {
    event.preventDefault();
    if (direct[0] === "search") {
      commandQuery.value = "";
      commandOpen.value = true;
    } else void runCommand(direct[0]);
    return;
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
    event.preventDefault();
    void search();
    return;
  }
  if (input) return;
  if (target?.closest("button") && [" ", "Enter"].includes(event.key)) return;
  if (Date.now() - keyTime > 800) keySequence = "";
  keySequence = keySequence ? `${keySequence} ${chord}` : chord;
  keyTime = Date.now();
  const seq = Object.entries(hotkeys).find(
    ([, value]) => value === keySequence,
  );
  if (seq) {
    event.preventDefault();
    keySequence = "";
    void runCommand(seq[0]);
    return;
  }
  if (keySequence.includes(" ")) keySequence = chord;
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a") {
    event.preventDefault();
    selectedIds.value = new Set(renderedTasks().map((t) => t.id));
    return;
  }
  if (
    (event.metaKey || event.ctrlKey) &&
    ["1", "2", "3", "4"].includes(event.key)
  ) {
    event.preventDefault();
    navigate(
      (["today", "upcoming", "inbox", "all"] as View[])[Number(event.key) - 1]!,
    );
    return;
  }
  if (event.key === "n") {
    event.preventDefault();
    addInput.value?.focus();
  }
  if (event.key === "/") {
    event.preventDefault();
    void search();
  }
  if (event.key === " " && selected.value) {
    event.preventDefault();
    void taskAction("complete");
  }
  if (["ArrowUp", "ArrowDown"].includes(event.key)) {
    event.preventDefault();
    const rows = renderedTasks();
    const index = rows.findIndex((t) => t.id === state.selectedId);
    const next =
      rows[
        Math.max(
          0,
          Math.min(
            rows.length - 1,
            index + (event.key === "ArrowDown" ? 1 : -1),
          ),
        )
      ];
    if (next) selectTask(next.id);
  }
}
</script>
<template>
  <ReminderPanel v-if="standaloneReminders" />
  <template v-else-if="standaloneSettings">
    <SettingsPanel
      standalone
      :settings="settings"
      :lists="activeLists"
      :tags="state.snapshot.tags"
      :templates="state.snapshot.templates"
      :initial-section="settingsSection"
      @close="closeSettings"
      @update-setting="updateSetting"
      @notification-settings="notificationSettings"
      @template-action="templateAction"
    />
    <div v-if="state.error" class="error-toast" role="alert">
      <span>{{ state.error }}</span
      ><button
        class="icon-button"
        aria-label="Dismiss error"
        @click="state.error = ''"
      >
        <PhX :size="16" />
      </button>
    </div>
  </template>
  <template v-else>
    <main
      class="app-frame"
      :aria-hidden="searchOpen || undefined"
      data-tauri-drag-region
      :style="paneStyle"
      :class="{
        'is-native': isNative,
        'interface-flat': interfaceStyle === 'flat',
        'has-selection': selected,
        'sidebar-open': state.sidebarOpen,
        'sidebar-collapsed': desktopSidebarHidden,
        'board-layout':
          (['board', 'timeline'].includes(viewPreferences.view_mode) ||
            state.view === 'summary') &&
          !selected,
        'summary-layout': state.view === 'summary',
      }"
    >
      <aside class="app-rail" aria-label="App navigation">
        <div class="window-drag" data-tauri-drag-region></div>
        <button
          class="app-avatar"
          aria-label="Settings"
          title="Settings · ⌘,"
          @click="openSettings()"
        >
          <img src="/assets/profile-texture.png" alt="" />
        </button>
        <div class="rail-navigation" aria-label="Task shortcuts">
          <button
            class="rail-button"
            :class="{
              active:
                viewPreferences.view_mode === 'list' &&
                !['today', 'upcoming', 'filter'].includes(state.view),
            }"
            aria-label="Tasks"
            title="Tasks"
            @click="openTaskList"
          >
            <PhCheckSquare :size="27" weight="fill" />
          </button>
          <button
            class="rail-button"
            :class="{ active: state.view === 'upcoming' }"
            aria-label="Upcoming tasks"
            title="Next 7 Days"
            @click="navigate('upcoming')"
          >
            <PhCalendarDots :size="26" weight="fill" />
          </button>
          <button
            class="rail-button"
            :class="{ active: viewPreferences.view_mode === 'board' }"
            aria-label="Board view"
            title="Board view"
            @click="openBoardView"
          >
            <PhCirclesFour :size="27" weight="fill" />
          </button>
          <button
            class="rail-button"
            :class="{ active: state.view === 'today' }"
            aria-label="Today's tasks"
            title="Today"
            @click="navigate('today')"
          >
            <PhClock :size="26" weight="fill" />
          </button>
          <button
            class="rail-button"
            :class="{ active: state.view === 'filter' || !!filterDialog }"
            aria-label="Create task filter"
            title="Filters"
            @click="openFilter()"
          >
            <PhCalendarStar :size="26" weight="fill" />
          </button>
          <button
            class="rail-button"
            aria-label="Search tasks"
            title="Search · ⌘F"
            @click="search"
          >
            <PhMagnifyingGlass :size="26" weight="bold" />
          </button>
        </div>
        <div class="rail-spacer" data-tauri-drag-region></div>
        <div class="rail-utilities">
          <button
            class="rail-button"
            aria-label="Refresh tasks"
            title="Refresh local tasks"
            :disabled="railRefreshing"
            @click="refreshFromRail"
          >
            <PhArrowClockwise
              :size="26"
              :class="{ 'is-refreshing': railRefreshing }"
            />
          </button>
          <button
            class="rail-button"
            aria-label="Notifications"
            title="Notifications"
            @click="openSettings('notifications')"
          >
            <PhBell :size="26" weight="fill" />
          </button>
          <button
            class="rail-button"
            aria-label="Keyboard shortcuts"
            title="Help and keyboard shortcuts"
            @click="openSettings('hotkeys')"
          >
            <PhQuestion :size="26" weight="fill" />
          </button>
        </div>
      </aside>
      <div class="workspace">
        <aside class="sidebar" aria-label="Task lists">
          <div class="sidebar-top" data-tauri-drag-region>
            <span>Tasks</span
            ><button
              class="icon-button mobile-close"
              aria-label="Close sidebar"
              @click="state.sidebarOpen = false"
            >
              <PhX :size="18" />
            </button>
          </div>
          <nav class="smart-navigation">
            <button
              v-for="view in smartViews"
              :key="view.id"
              class="nav-row"
              :class="{ selected: state.view === view.id }"
              @click="navigate(view.id)"
            >
              <TaskNavigationGlyph :kind="view.id" :size="19" /><span>{{
                view.name
              }}</span
              ><small v-if="showCounts && smartCounts[view.id]">{{
                smartCounts[view.id]
              }}</small>
            </button>
            <button
              v-if="smartVisible('summary')"
              class="nav-row"
              :class="{ selected: state.view === 'summary' }"
              @click="navigate('summary')"
            >
              <PhList :size="19" /><span>Summary</span>
            </button>
          </nav>
          <div class="sidebar-divider"></div>
          <div class="sidebar-tree-scroll">
            <SidebarTree
              :folders="state.snapshot.folders"
              :lists="listOptions"
              :tasks="tasks"
              :active-list="state.view === 'list' ? state.listId : null"
              :show-counts="showCounts"
              :hide-note-counts="hideNoteCounts"
              :show-color="true"
              @select="navigate('list', $event)"
              @create="createContainer"
              @edit="editContainer"
              @remove="removeContainer"
              @mutate="handleMutate"
              @reorder-container="reorderContainer"
            />
            <button
              v-if="archivedLists.length"
              class="nav-row archived-link"
              :class="{ selected: state.view === 'archived' }"
              @click="navigate('archived')"
            >
              <PhArchiveBox :size="17" /><span>Archived Lists</span>
            </button>
            <section
              v-if="smartVisible('filters')"
              class="sidebar-section sidebar-filters"
            >
              <div class="sidebar-section-heading">
                <button
                  :aria-expanded="!filtersCollapsed"
                  @click="filtersCollapsed = !filtersCollapsed"
                >
                  <PhCaretRight
                    v-if="filtersCollapsed"
                    :size="11"
                  /><PhCaretDown v-else :size="11" />Filters</button
                ><button
                  class="icon-button"
                  aria-label="Add filter"
                  @click="openFilter()"
                >
                  <PhPlus :size="16" />
                </button>
              </div>
              <template v-if="!filtersCollapsed">
                <button
                  v-if="!savedFilters.length"
                  class="sidebar-filter-hint"
                  aria-label="Create a filter"
                  @click="openFilter()"
                >
                  Display tasks filtered by list, date, priority, tag, and more
                </button>
                <div
                  v-for="filter in savedFilters"
                  :key="filter.id"
                  class="nav-row"
                  :class="{
                    selected:
                      state.view === 'filter' && state.listId === filter.id,
                  }"
                  role="group"
                  @contextmenu.prevent="openFilter(filter)"
                >
                  <TaskNavigationGlyph kind="filters" :size="18" /><button
                    class="sidebar-label-button"
                    @click="navigate('filter', filter.id)"
                  >
                    {{ filter.name }}</button
                  ><button
                    class="sidebar-item-edit icon-button"
                    :aria-label="`Edit filter ${filter.name}`"
                    @click.stop="openFilter(filter)"
                  >
                    <PhDotsThree :size="16" />
                  </button></div
              ></template>
            </section>
            <section
              v-if="smartVisible('tags')"
              class="sidebar-section sidebar-tags"
            >
              <div class="sidebar-section-heading">
                <button
                  :aria-expanded="!tagsCollapsed"
                  @click="tagsCollapsed = !tagsCollapsed"
                >
                  <PhCaretRight v-if="tagsCollapsed" :size="11" /><PhCaretDown
                    v-else
                    :size="11"
                  />Tags</button
                ><button
                  class="icon-button"
                  aria-label="Add tag"
                  @click="
                    tagDialog = {
                      id: null,
                      name: '',
                      color: '#6476b4',
                      deleting: false,
                    }
                  "
                >
                  <PhPlus :size="16" />
                </button>
              </div>
              <template v-if="!tagsCollapsed"
                ><div
                  v-for="tag in state.snapshot.tags"
                  :key="tag.id"
                  class="nav-row"
                  :class="{
                    selected: state.view === 'tag' && state.listId === tag.id,
                  }"
                  role="group"
                  @contextmenu.prevent="tagDialog = { ...tag, deleting: false }"
                >
                  <PhTag :size="18" :style="{ color: tag.color }" /><button
                    class="sidebar-label-button"
                    @click="navigate('tag', tag.id)"
                  >
                    {{ tag.name }}</button
                  ><small v-if="showCounts">{{
                    countedTasks(scheduledTasks).filter((t) =>
                      t.tag_ids?.includes(tag.id),
                    ).length || ""
                  }}</small
                  ><button
                    class="sidebar-item-edit icon-button"
                    :aria-label="`Edit tag ${tag.name}`"
                    @click.stop="tagDialog = { ...tag, deleting: false }"
                  >
                    <PhDotsThree :size="16" />
                  </button></div
              ></template>
            </section>
          </div>
          <div class="sidebar-bottom">
            <div class="sidebar-divider"></div>
            <button
              v-for="view in bottomViews"
              :key="view.id"
              class="nav-row"
              :class="{ selected: state.view === view.id }"
              @click="navigate(view.id)"
            >
              <span class="sidebar-bottom-glyph"
                ><component
                  :is="
                    view.id === 'completed'
                      ? IconSquareRoundedCheck
                      : view.id === 'trash'
                        ? IconTrash
                        : view.icon
                  "
                  :size="view.id === 'trash' ? 21 : 19"
                  :stroke-width="1.9" /></span
              ><span>{{ view.name }}</span>
            </button>
          </div>
        </aside>
        <PaneResizer
          v-if="!desktopSidebarHidden"
          label="Resize sidebar"
          :value="sidebarWidth"
          :min="150"
          :max="sidebarMax"
          :default-value="218"
          @resize="resizingSidebar = $event"
          @commit="savePaneWidth('sidebar_width', $event)"
          @cancel="resizingSidebar = null"
        />
        <section class="task-list-panel" aria-label="Tasks">
          <header class="list-header" data-tauri-drag-region="deep">
            <button
              class="icon-button sidebar-toggle"
              aria-label="Toggle sidebar"
              @click="toggleSidebar"
            >
              <TaskNavigationGlyph kind="sidebar" :size="20" /></button
            ><PhList
              v-if="state.view === 'list'"
              class="list-title-icon"
              :size="19"
            />
            <h1>{{ title }}</h1>
            <div class="popover-anchor" data-tauri-drag-region="false">
              <button
                class="icon-button"
                aria-label="Sort tasks"
                @click="menu = menu === 'sort' ? null : 'sort'"
              >
                <PhArrowsDownUp :size="19" />
              </button>
              <div
                v-if="menu === 'sort'"
                class="popover sorting-popover align-right"
                role="menu"
                aria-label="Sort and group tasks"
              >
                <div class="popover-section-caption">Group By</div>
                <button
                  v-for="item in [
                    { id: 'none', label: 'None' },
                    { id: 'date', label: 'Date' },
                    { id: 'priority', label: 'Priority' },
                    { id: 'list', label: 'List' },
                    { id: 'tag', label: 'Tag' },
                    ...(state.view === 'list'
                      ? [{ id: 'section', label: 'Section' }]
                      : []),
                  ]"
                  :key="item.id"
                  @click="
                    preference({
                      group_by: item.id as ViewPreferences['group_by'],
                    })
                  "
                >
                  {{ item.label
                  }}<PhCheck
                    v-if="viewPreferences.group_by === item.id"
                    :size="13"
                  />
                </button>
                <div class="menu-separator"></div>
                <div class="popover-section-caption">Sort By</div>
                <button
                  v-for="item in [
                    { id: 'manual', label: 'Custom' },
                    { id: 'due_date', label: 'Date' },
                    { id: 'priority', label: 'Priority' },
                    { id: 'title', label: 'Title' },
                    { id: 'created_at', label: 'Creation Time' },
                  ]"
                  :key="item.id"
                  @click="
                    preference({
                      sort_by: item.id as ViewPreferences['sort_by'],
                    })
                  "
                >
                  {{ item.label
                  }}<PhCheck
                    v-if="viewPreferences.sort_by === item.id"
                    :size="13"
                  />
                </button>
                <div class="menu-separator"></div>
                <button
                  @click="
                    preference({
                      sort_order:
                        viewPreferences.sort_order === 'asc' ? 'desc' : 'asc',
                    })
                  "
                >
                  Reverse Order<PhCheck
                    v-if="viewPreferences.sort_order === 'desc'"
                    :size="13"
                  />
                </button>
              </div>
            </div>
            <div class="popover-anchor" data-tauri-drag-region="false">
              <button
                class="icon-button"
                aria-label="List actions"
                @click="menu = menu === 'list' ? null : 'list'"
              >
                <PhDotsThree :size="23" weight="bold" />
              </button>
              <div
                v-if="menu === 'list'"
                class="popover menu-popover align-right shell-list-menu"
                role="menu"
                aria-label="List actions"
              >
                <button
                  @click="
                    preference({ view_mode: 'list' });
                    menu = null;
                  "
                >
                  <PhList :size="17" />List View<PhCheck
                    v-if="viewPreferences.view_mode === 'list'"
                    class="menu-check"
                    :size="13"
                  /></button
                ><button
                  @click="
                    preference({ view_mode: 'board' });
                    menu = null;
                  "
                >
                  <PhSquaresFour :size="17" />Kanban View<PhCheck
                    v-if="viewPreferences.view_mode === 'board'"
                    class="menu-check"
                    :size="13"
                  />
                </button>
                <button
                  v-if="['list', 'filter'].includes(state.view)"
                  @click="
                    preference({ view_mode: 'timeline' });
                    menu = null;
                  "
                >
                  <PhCalendarBlank :size="17" />Timeline View<PhCheck
                    v-if="viewPreferences.view_mode === 'timeline'"
                    class="menu-check"
                    :size="13"
                  />
                </button>
                <div class="menu-separator"></div>
                <button
                  v-if="canCreate"
                  @click="
                    openQuick();
                    menu = null;
                  "
                >
                  <PhPlus :size="17" />Add Task</button
                ><button
                  v-if="state.view === 'list'"
                  @click="
                    sectionDialog = { id: null, name: '' };
                    menu = null;
                  "
                >
                  <PhPlus :size="17" />Add Section</button
                ><button
                  @click="
                    state.showCompleted = !state.showCompleted;
                    menu = null;
                  "
                >
                  <PhCheckSquare :size="17" />{{
                    state.showCompleted ? "Hide Completed" : "Show Completed"
                  }}</button
                ><template v-if="state.view === 'list' && state.listId"
                  ><div class="menu-separator"></div>
                  <button
                    @click="editContainer({ kind: 'list', id: state.listId })"
                  >
                    <PhList :size="17" />Edit List</button
                  ><button
                    @click="
                      handleMutate('lists.archive', {
                        id: state.listId,
                        archived: !currentList?.archived_at,
                      });
                      menu = null;
                    "
                  >
                    <PhArchiveBox :size="17" />{{
                      currentList?.archived_at
                        ? "Unarchive List"
                        : "Archive List"
                    }}</button
                  ><button
                    class="danger-text"
                    @click="removeContainer({ kind: 'list', id: state.listId })"
                  >
                    <PhTrash :size="17" />Delete List
                  </button></template
                ><template v-if="state.snapshot.templates.length && canCreate"
                  ><div class="menu-separator"></div>
                  <div class="popover-section-caption">Templates</div>
                  <button
                    v-for="template in state.snapshot.templates"
                    :key="template.id"
                    @click="applyTemplate(template.id)"
                  >
                    <PhList :size="16" />{{ template.name }}
                  </button></template
                >
              </div>
            </div>
          </header>
          <form
            v-if="state.view === 'search'"
            class="quick-add search-field"
            @submit.prevent
          >
            <PhMagnifyingGlass :size="17" /><input
              ref="searchInput"
              v-model="state.query"
              placeholder="Search tasks and notes"
              aria-label="Search tasks and notes"
            /><button
              v-if="state.query"
              class="icon-button"
              aria-label="Clear search"
              @click="state.query = ''"
            >
              <PhX :size="14" />
            </button>
          </form>
          <form
            v-else-if="canCreate"
            class="quick-add"
            @submit.prevent="createTask()"
          >
            <PhPlus :size="18" /><input
              ref="addInput"
              @paste="handleBatchPaste"
              v-model="newTitle"
              :placeholder="`Add ${currentList?.list_type === 'note' ? 'note' : 'task'}${state.view === 'list' ? '' : ` to &quot;${activeLists.find((l) => l.id === (settings.default_list_id || 'inbox'))?.name || 'Inbox'}&quot;`}`"
              aria-label="Add task"
              :disabled="creating"
            /><button
              v-if="newTitle"
              class="icon-button quick-submit"
              type="submit"
              aria-label="Create task"
              :disabled="creating"
            >
              <PhArrowUp :size="16" />
            </button>
          </form>
          <div v-if="selectedIds.size > 1" class="multi-actions">
            <span>{{ selectedIds.size }} selected</span
            ><button
              aria-label="Complete selected tasks"
              @click="taskAction('complete')"
            >
              <PhCheck :size="16" /></button
            ><button
              aria-label="Delete selected tasks"
              @click="taskAction('delete')"
            >
              <PhTrash :size="16" /></button
            ><button
              @click="selectedIds = new Set()"
              aria-label="Clear selection"
            >
              <PhX :size="16" />
            </button>
          </div>
          <TaskSummary
            v-if="state.view === 'summary'"
            embedded
            :tasks="tasks"
            :lists="state.snapshot.lists"
            :tags="state.snapshot.tags"
            :presets="summaryPresets"
            :week-start="Number(settings.week_start ?? 1)"
            @open-task="openSearchResult({ kind: 'task', id: $event })"
            @save-presets="saveSummaryPresets"
            @create-note="createSummaryNote"
          />
          <TaskTimeline
            v-else-if="
              ['list', 'filter'].includes(state.view) &&
              viewPreferences.view_mode === 'timeline'
            "
            :tasks="displayedTasks"
            :lists="activeLists"
            :selected-id="state.selectedId"
            @open-task="selectTask"
            @update="updateTimelineTask"
            @create="createTimelineTask"
          />
          <div
            v-else
            ref="taskListElement"
            class="task-list-scroll"
            :class="{ 'task-board': viewPreferences.view_mode === 'board' }"
          >
            <div v-if="state.loading" class="empty-list loading-state">
              Loading tasks…
            </div>
            <template v-else-if="state.view === 'archived'"
              ><div
                v-for="list in archivedLists"
                :key="list.id"
                class="archived-list-row"
                role="group"
              >
                <PhArchiveBox :size="20" /><button
                  class="sidebar-label-button"
                  @click="navigate('list', list.id)"
                >
                  {{ list.name }}</button
                ><button
                  @click.stop="
                    handleMutate('lists.archive', {
                      id: list.id,
                      archived: false,
                    })
                  "
                >
                  Unarchive
                </button>
              </div>
              <div v-if="!archivedLists.length" class="empty-list">
                No archived lists
              </div></template
            ><template v-else
              ><div
                v-if="state.view === 'search' && state.query"
                class="search-count"
              >
                {{ displayedTasks.length }}
                {{ displayedTasks.length === 1 ? "result" : "results" }}
              </div>
              <section
                v-for="group in taskGroups"
                :key="group.key"
                class="task-group"
                @dragover.prevent
                @drop.self.prevent="
                  viewPreferences.group_by === 'section' &&
                  dropSection($event, group.sectionId || null)
                "
              >
                <header v-if="group.label" class="task-group-header">
                  <button
                    class="task-group-toggle"
                    @click="toggleGroup(group.key)"
                  >
                    <PhCaretRight
                      v-if="collapsedGroups.has(group.key)"
                      :size="11"
                    /><PhCaretDown v-else :size="11" /><span>{{
                      group.label
                    }}</span
                    ><small>{{ group.tasks.length }}</small>
                  </button>
                  <button
                    v-if="group.key === 'overdue' && group.tasks.length"
                    class="postpone-group"
                    :disabled="postponing"
                    @click="postponeGroup(group)"
                  >
                    Postpone
                  </button>
                  <div v-if="group.sectionId" class="section-tools">
                    <button
                      :aria-label="`Rename section ${group.label}`"
                      @click="
                        sectionDialog = {
                          id: group.sectionId,
                          name: group.label,
                        }
                      "
                    >
                      <PhDotsThree :size="17" /></button
                    ><button
                      :aria-label="`Delete section ${group.label}`"
                      @click="
                        handleMutate('sections.delete', { id: group.sectionId })
                      "
                    >
                      <PhX :size="13" />
                    </button>
                  </div>
                </header>
                <template v-if="!collapsedGroups.has(group.key)"
                  ><TaskTreeRow
                    v-for="task in group.tasks"
                    :key="task.id"
                    :task="task"
                    :tasks="tasks"
                    :tasks-by-id="tasksById"
                    :tasks-by-parent="tasksByParent"
                    :selected-id="state.selectedId"
                    :selected-ids="selectedIds"
                    :expansion-command="subtaskExpansion"
                    :add-below-id="addBelow?.anchorId"
                    :add-below-title="addBelow?.title"
                    :add-below-busy="addingBelow"
                    :editing-title-id="editingTitleId"
                    :quitting="quitting"
                    @edit-title="editTitleInRow"
                    @finish-title="finishRowTitle"
                    @update-add-below="addBelow && (addBelow.title = $event)"
                    @create-below="!quitting && createTaskBelow()"
                    @cancel-add-below="!quitting && cancelAddBelow()"
                    :show-completed="
                      state.showCompleted ||
                      ['completed', 'trash', 'wont_do'].includes(state.view)
                    "
                    :draggable="canReorder"
                    :list-name="
                      [
                        'all',
                        'today',
                        'tomorrow',
                        'upcoming',
                        'search',
                        'tag',
                        'filter',
                      ].includes(state.view)
                        ? state.snapshot.lists.find(
                            (l) => l.id === task.list_id,
                          )?.name
                        : undefined
                    "
                    :list-color="
                      settings.show_list_color === true
                        ? state.snapshot.lists.find(
                            (l) => l.id === task.list_id,
                          )?.color
                        : undefined
                    "
                    @select="selectTask"
                    @complete="complete"
                    @restore="restore"
                    @drop="dropTask"
                    @context="taskContext"
                  />
                  <div
                    v-if="!group.tasks.length && group.sectionId"
                    class="empty-section"
                    @dragover.prevent
                    @drop.prevent="dropSection($event, group.sectionId)"
                  >
                    Drop tasks here
                  </div></template
                >
              </section>
              <div
                v-if="!displayedTasks.length && !taskGroups.length"
                class="empty-list"
              >
                <PhMagnifyingGlass
                  v-if="state.view === 'search'"
                  :size="34"
                  weight="thin"
                /><PhTrash
                  v-else-if="state.view === 'trash'"
                  :size="34"
                  weight="thin"
                /><PhCheckSquare v-else :size="34" weight="thin" />
                <p>{{ emptyLabel }}</p>
                <span v-if="canCreate">Add a task to get started.</span>
              </div>
            </template>
          </div>
          <footer v-if="!isNative" class="preview-status">
            <PhCloudSlash :size="12" />Local preview
          </footer>
        </section>
        <PaneResizer
          v-if="
            (!['board', 'timeline'].includes(viewPreferences.view_mode) &&
              state.view !== 'summary') ||
            selected
          "
          label="Resize task list"
          :value="listWidth"
          :min="250"
          :max="listMax"
          :default-value="388"
          @resize="resizingList = $event"
          @commit="savePaneWidth('list_width', $event)"
          @cancel="resizingList = null"
        />
        <TaskEditor
          v-if="selected"
          ref="editorRef"
          :task="selected"
          @close="state.selectedId = null"
        />
        <section
          v-else
          class="empty-detail"
          aria-label="Select a task to see its details"
        >
          <img src="/assets/empty-detail.png" alt="" draggable="false" />
          <div
            class="detail-window-drag"
            data-tauri-drag-region
            aria-hidden="true"
          ></div>
        </section>
      </div>
      <div v-if="state.error" class="error-toast" role="alert">
        <span>{{ state.error }}</span
        ><button v-if="hasFailedDraft" @click="retryDrafts">Retry</button
        ><button
          class="icon-button"
          aria-label="Dismiss error"
          @click="state.error = ''"
        >
          <PhX :size="16" />
        </button>
      </div>
    </main>
    <TaskContextMenu
      v-if="context"
      :task="context.task"
      :x="context.x"
      :y="context.y"
      :count="selectedIds.size"
      :lists="activeLists"
      :tags="state.snapshot.tags"
      @close="context = null"
      @action="taskAction"
    />
    <SettingsPanel
      v-if="settingsOpen"
      :settings="settings"
      :lists="activeLists"
      :tags="state.snapshot.tags"
      :templates="state.snapshot.templates"
      :initial-section="settingsSection"
      @close="settingsOpen = false"
      @update-setting="updateSetting"
      @notification-settings="notificationSettings"
      @template-action="templateAction"
    />
    <BulkTaskDialog
      v-if="bulkDialog"
      :mode="bulkDialog.mode"
      :task="bulkDialog.task"
      :tasks="bulkTasks"
      :tags="state.snapshot.tags"
      :settings="settings"
      :busy="bulkBusy"
      :error="bulkError"
      @close="bulkDialog = null"
      @apply-date="applyBulkDate"
      @apply-tags="applyBulkTags"
    />
    <BatchAddDialog
      v-if="batchPaste"
      :text="batchPaste.text"
      :lists="activeLists"
      :tags="state.snapshot.tags"
      :settings="settings"
      :defaults="batchDefaults"
      :overrides="batchOverrides"
      @close="batchPaste = null"
      @created="batchCreated"
    />
    <QuickAddDialog
      v-if="quickOpen"
      :title="quickTitle"
      :date-patch="quickPreviewDate"
      :priority="quickPreviewPriority"
      :list-id="quickPreviewList"
      :lists="activeLists"
      :settings="settings"
      :busy="creating"
      @update:title="quickTitle = $event"
      @update:date-patch="setQuickDate"
      @update:priority="setQuickPriority"
      @update:list-id="setQuickList"
      @batch-paste="batchPaste = { text: $event, fromQuick: true }"
      @submit="createTask(true)"
      @close="quickOpen = false"
    />
    <ListFolderDialog
      v-if="management"
      :key="`${management.kind}:${management.id || 'new'}`"
      :kind="management.kind"
      :item="managementItem"
      :folders="state.snapshot.folders"
      :initial-parent-id="management.parentId"
      :busy="managementBusy"
      :deleting="management.deleting"
      :error="managementError"
      @save="saveContainer"
      @close="management = null"
      @delete="deleteContainer"
    />
    <AppDialog
      v-if="tagDialog"
      :title="tagDialog.id ? 'Edit Tag' : 'Add Tag'"
      @close="tagDialog = null"
      ><form class="dialog-form" @submit.prevent="saveTag">
        <label
          >Name<input
            v-model="tagDialog.name"
            aria-label="Tag name"
            autofocus
            maxlength="100" /></label
        ><label class="color-field"
          >Color<input
            type="color"
            v-model="tagDialog.color"
            aria-label="Tag color"
        /></label>
        <p v-if="tagDialog.deleting">Delete this tag? Tasks will be kept.</p>
        <footer>
          <button
            v-if="tagDialog.id && !tagDialog.deleting"
            class="text-button danger-text"
            type="button"
            @click="tagDialog.deleting = true"
          >
            Delete Tag</button
          ><span v-else></span
          ><button class="primary-button" :disabled="!tagDialog.name.trim()">
            {{ tagDialog.deleting ? "Delete" : "Save" }}
          </button>
        </footer>
      </form></AppDialog
    >
    <AppDialog
      v-if="filterDialog"
      :title="
        savedFilters.some((f) => f.id === filterDialog?.id)
          ? 'Edit Filter'
          : 'Add Filter'
      "
      @close="filterDialog = null"
      ><form class="dialog-form" @submit.prevent="saveFilter()">
        <label
          >Name<input
            v-model="filterDialog.name"
            aria-label="Filter name"
            autofocus /></label
        ><label
          >Match<select v-model="filterDialog.match" aria-label="Filter match">
            <option value="all">All conditions</option>
            <option value="any">Any condition</option>
          </select></label
        >
        <fieldset class="filter-options">
          <legend>Lists</legend>
          <label v-for="list in activeLists" :key="list.id"
            ><input
              type="checkbox"
              v-model="filterDialog.list_ids"
              :value="list.id"
            />{{ list.name }}</label
          >
        </fieldset>
        <fieldset v-if="state.snapshot.tags.length" class="filter-options">
          <legend>Tags</legend>
          <label v-for="tag in state.snapshot.tags" :key="tag.id"
            ><input
              type="checkbox"
              v-model="filterDialog.tag_ids"
              :value="tag.id"
            />{{ tag.name }}</label
          >
        </fieldset>
        <fieldset class="filter-options">
          <legend>Priority</legend>
          <label v-for="p in [3, 2, 1, 0]" :key="p"
            ><input
              type="checkbox"
              v-model="filterDialog.priorities"
              :value="p"
            />{{ ["None", "Low", "Medium", "High"][p] }}</label
          >
        </fieldset>
        <label
          >Date<select v-model="filterDialog.date" aria-label="Filter date">
            <option value="all">Any date</option>
            <option value="today">Today</option>
            <option value="tomorrow">Tomorrow</option>
            <option value="week">Next 7 Days</option>
            <option value="overdue">Overdue</option>
            <option value="undated">No Date</option>
          </select></label
        >
        <footer>
          <button
            v-if="savedFilters.some((f) => f.id === filterDialog?.id)"
            type="button"
            class="text-button danger-text"
            @click="saveFilter(true)"
          >
            Delete Filter</button
          ><span v-else></span
          ><button class="primary-button" :disabled="!filterDialog.name.trim()">
            Save
          </button>
        </footer>
      </form></AppDialog
    >
    <AppDialog
      v-if="sectionDialog"
      :title="sectionDialog.id ? 'Rename Section' : 'Add Section'"
      @close="sectionDialog = null"
      ><form class="dialog-form" @submit.prevent="saveSection">
        <label
          >Name<input
            v-model="sectionDialog.name"
            aria-label="Section name"
            autofocus
        /></label>
        <footer>
          <span></span
          ><button
            class="primary-button"
            :disabled="!sectionDialog.name.trim()"
          >
            Save
          </button>
        </footer>
      </form></AppDialog
    >
    <AppDialog
      v-if="templateDialog"
      title="Save as Template"
      @close="templateDialog = false"
      ><form class="dialog-form" @submit.prevent="saveTemplate">
        <label
          >Name<input
            v-model="templateName"
            aria-label="Template name"
            autofocus
        /></label>
        <footer>
          <span></span
          ><button class="primary-button" :disabled="!templateName.trim()">
            Save
          </button>
        </footer>
      </form></AppDialog
    >
    <AppDialog
      v-if="commandOpen"
      title="Command Menu"
      @close="commandOpen = false"
      ><div class="command-menu">
        <input
          v-model="commandQuery"
          placeholder="Search commands…"
          aria-label="Search commands"
          autofocus
          @keydown.enter="
            currentCommands[0] && runCommand(currentCommands[0].id)
          "
        /><button
          v-for="command in currentCommands"
          :key="command.id"
          @click="runCommand(command.id)"
        >
          <component
            :is="command.icon"
            :size="17"
            :kind="'glyph' in command ? command.glyph : undefined"
          /><span>{{ command.label }}</span>
        </button>
      </div></AppDialog
    >
    <SearchDialog
      v-if="searchOpen"
      :tasks="tasks"
      :lists="state.snapshot.lists"
      :tags="state.snapshot.tags"
      :filters="savedFilters"
      @close="searchOpen = false"
      @select="openSearchResult"
    />
  </template>
  <UpdateNotice
    v-if="!standaloneSettings && !standaloneReminders && !quitting"
    @open="openSettings('updates')"
  />
  <Teleport to="body"
    ><div
      v-if="quitting"
      class="quit-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="Saving before quit"
    >
      <div>
        <p>
          {{
            installingUpdate
              ? "Installing update. Tasks will restart…"
              : "Saving changes…"
          }}
        </p>
        <button v-if="!installingUpdate" @click="cancelNativeQuit()">
          Cancel
        </button>
      </div>
    </div></Teleport
  >
</template>

<style scoped>
.postpone-group {
  margin-left: auto;
  padding: 3px 8px;
  color: var(--accent);
  font-size: 12px;
  font-weight: 500;
}
.postpone-group:hover {
  text-decoration: underline;
}
.postpone-group:disabled {
  opacity: 0.5;
}
</style>
