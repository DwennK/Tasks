import { computed, reactive, ref } from "vue";
import { dispatch } from "../adapter";
import { noteText } from "../lib/note-text";
import type { SavedFilter, Snapshot, Task, ViewPreferences } from "../types";

export type View =
  | "summary"
  | "today"
  | "tomorrow"
  | "wont_do"
  | "tag"
  | "filter"
  | "archived"
  | "upcoming"
  | "inbox"
  | "all"
  | "completed"
  | "trash"
  | "list"
  | "search";
const formatLocalDate = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
const currentLocalDay = ref(formatLocalDate(new Date()));
export function refreshLocalDay() {
  currentLocalDay.value = formatLocalDate(new Date());
}
export function localDate(date?: Date) {
  if (date) return formatLocalDate(date);
  const today = formatLocalDate(new Date());
  // Subscribe computed views to the day tick, while imperative actions after
  // sleep still receive the actual date before the next polling callback.
  return currentLocalDay.value === today ? currentLocalDay.value : today;
}
export function dateLabel(value: string | null) {
  if (!value) return "";
  const today = localDate();
  const yesterday = new Date(`${today}T12:00:00`);
  yesterday.setDate(yesterday.getDate() - 1);
  const tomorrow = new Date(`${today}T12:00:00`);
  tomorrow.setDate(tomorrow.getDate() + 1);
  if (value === today) return "Today";
  if (value === localDate(yesterday)) return "Yesterday";
  if (value === localDate(tomorrow)) return "Tomorrow";
  return new Date(`${value}T12:00:00`).toLocaleDateString("en", {
    month: "short",
    day: "numeric",
  });
}

const empty: Snapshot = {
  tasks: [],
  lists: [],
  folders: [],
  tags: [],
  sections: [],
  templates: [],
  settings: {},
  revision: 0,
};
const state = reactive({
  snapshot: empty,
  loading: true,
  busy: 0,
  error: "",
  view: "inbox" as View,
  listId: null as string | null,
  selectedId: null as string | null,
  query: "",
  sort: "manual",
  showCompleted: false,
  sidebarOpen: false,
  drafts: {} as Record<
    string,
    { patch: Record<string, unknown>; version: number; failed: boolean }
  >,
});
let operation = Promise.resolve<unknown>(undefined);
let settingSequence = 0;
let writeFailures = 0;
const optimisticSettings = new Map<
  string,
  { value: unknown; sequence: number }
>();
let refreshPromise: Promise<boolean> | null = null;
const saving = new Map<string, Promise<void>>();
const timers = new Map<string, ReturnType<typeof setTimeout>>();
const restoringDrafts = new Map<string, number>();
const noteIndex = new Map<
  string,
  { notes: string; format: string; text: string }
>();
function searchableNote(task: Task) {
  const patch = state.drafts[task.id]?.patch;
  if (
    typeof task.notes_text === "string" &&
    (!patch || (!("notes" in patch) && !("notes_format" in patch)))
  )
    return task.notes_text.replace(/\s+/g, " ").trim();
  const cached = noteIndex.get(task.id);
  if (
    cached &&
    cached.notes === task.notes &&
    cached.format === task.notes_format
  )
    return cached.text;
  const text = noteText(task.notes, task.notes_format || "plain");
  noteIndex.set(task.id, {
    notes: task.notes,
    format: task.notes_format,
    text,
  });
  return text;
}

export function taskMatchesSearch(task: Task, query: string): boolean {
  const searchable = (
    task.title +
    "\n" +
    searchableNote(task) +
    "\n" +
    state.snapshot.tags
      .filter((tag) => task.tag_ids?.includes(tag.id))
      .map((tag) => tag.name)
      .join(" ")
  ).toLocaleLowerCase();
  return searchable.includes(query.toLocaleLowerCase().trim());
}

async function refresh(force = false): Promise<boolean> {
  refreshLocalDay();
  if (refreshPromise) {
    const result = await refreshPromise;
    if (!force) return result;
  }
  const run = (async () => {
    try {
      const snapshot = await dispatch<Snapshot>("snapshot", {});
      for (const [key, pending] of optimisticSettings)
        snapshot.settings[key] = pending.value;
      state.snapshot = {
        ...snapshot,
        tags: snapshot.tags || [],
        sections: snapshot.sections || [],
        templates: snapshot.templates || [],
      };
      const taskIds = new Set(snapshot.tasks.map((task) => task.id));
      for (const id of noteIndex.keys())
        if (!taskIds.has(id)) noteIndex.delete(id);
      state.loading = false;
      return true;
    } catch (e) {
      state.error = String(e);
      state.loading = false;
      return false;
    }
  })();
  refreshPromise = run;
  const result = await run;
  if (refreshPromise === run) refreshPromise = null;
  return result;
}
async function checkRevision() {
  // Reuse the existing light poll; only a changed local day invalidates views.
  refreshLocalDay();
  if (state.busy || refreshPromise) return;
  try {
    const result = await dispatch<{ revision: number }>("revision", {});
    if (result.revision !== state.snapshot.revision) await refresh();
  } catch (e) {
    state.error = String(e);
  }
}
function rebaseOwnWrite(
  before: Task | undefined,
  saved: Task | undefined,
  input: Record<string, unknown>,
) {
  if (
    !before ||
    !saved ||
    saved.id !== before.id ||
    saved.version <= before.version
  )
    return;
  const draft = state.drafts[saved.id];
  if (!draft || draft.version !== before.version || draft.failed) return;
  // Merge only across an authoritative write result/read when every drafted
  // field still equals its base. Disjoint changes can coexist; a same-field
  // external change retains its conflict and the draft stays untouched.
  for (const key of Object.keys(draft.patch)) {
    if (
      key in input ||
      JSON.stringify((saved as unknown as Record<string, unknown>)[key]) !==
        JSON.stringify((before as unknown as Record<string, unknown>)[key])
    )
      return;
  }
  draft.version = saved.version;
}
function enqueue<T = unknown>(
  action: string,
  input: Record<string, unknown> = {},
  onWritten?: () => void,
  preservedDraftIds?: ReadonlySet<string>,
): Promise<T> {
  state.busy++;
  const next = operation.then(async () => {
    // Relationship writes may address a comment/attachment rather than a task,
    // and cascading actions can touch several tasks. Retain raw base records.
    const beforeTasks = new Map(
      state.snapshot.tasks.map((task) => [task.id, task]),
    );
    const before =
      typeof input.id === "string"
        ? state.snapshot.tasks.find((t) => t.id === input.id)
        : undefined;
    // Metadata edits use the same optimistic concurrency check as text edits.
    const request =
      action === "tasks.update" &&
      before &&
      input.expected_version === undefined
        ? { ...input, expected_version: before.version }
        : input;
    try {
      const result = await dispatch<T>(action, request);
      onWritten?.();
      const saved =
        action.startsWith("tasks.") &&
        result &&
        typeof result === "object" &&
        "version" in result &&
        "id" in result
          ? (result as unknown as Task)
          : undefined;
      if (!saved || !preservedDraftIds?.has(saved.id))
        rebaseOwnWrite(before, saved, input);
      // The write result is authoritative even if the follow-up read fails.
      if (saved) {
        const index = state.snapshot.tasks.findIndex((t) => t.id === saved.id);
        if (index >= 0) state.snapshot.tasks[index] = saved;
        else state.snapshot.tasks.push(saved);
      }
      if (await refresh(true)) {
        for (const task of state.snapshot.tasks) {
          if (!preservedDraftIds?.has(task.id))
            rebaseOwnWrite(beforeTasks.get(task.id), task, input);
        }
      }
      return result;
    } catch (e) {
      writeFailures++;
      state.error = String(e);
      throw e;
    } finally {
      state.busy--;
    }
  });
  operation = next.catch(() => undefined);
  return next;
}
function restoringTaskIds(id: string) {
  const byId = new Map(state.snapshot.tasks.map((task) => [task.id, task]));
  const children = new Map<string, string[]>();
  for (const task of state.snapshot.tasks) {
    if (!task.parent_id) continue;
    const siblings = children.get(task.parent_id) || [];
    siblings.push(task.id);
    children.set(task.parent_id, siblings);
  }
  const related = new Set<string>();
  const pending = [id];
  while (pending.length) {
    const current = pending.pop()!;
    if (related.has(current)) continue;
    related.add(current);
    pending.push(...(children.get(current) || []));
  }
  let parentId = byId.get(id)?.parent_id;
  while (parentId && !related.has(parentId)) {
    related.add(parentId);
    parentId = byId.get(parentId)?.parent_id;
  }
  // Core Restore affects the subtree and necessary ancestors. Only deleted
  // members need an exception to the usual requirement to save drafts first.
  return new Set([...related].filter((taskId) => byId.get(taskId)?.deleted_at));
}
async function mutate<T = unknown>(
  action: string,
  input: Record<string, unknown> = {},
): Promise<T> {
  if (action === "settings.set" && typeof input.key === "string") {
    const key = input.key;
    const value = JSON.parse(JSON.stringify(input.value)) as unknown;
    const sequence = ++settingSequence;
    optimisticSettings.set(key, { value, sequence });
    // Mutate the same object passed to SettingsPanel so consecutive composite
    // controls immediately see each other's pending changes before Vue rerenders.
    state.snapshot.settings[key] = value;
    try {
      return await enqueue<T>(action, { ...input, value }, () => {
        // Release this acknowledged projection BEFORE the authoritative read.
        // A later pending edit to the same key retains its own projection.
        if (optimisticSettings.get(key)?.sequence === sequence)
          optimisticSettings.delete(key);
      });
    } catch (error) {
      if (optimisticSettings.get(key)?.sequence === sequence)
        optimisticSettings.delete(key);
      await refresh(true);
      throw error;
    } finally {
      if (optimisticSettings.get(key)?.sequence === sequence)
        optimisticSettings.delete(key);
    }
  }
  if (action === "tasks.restore" && typeof input.id === "string") {
    const preserved = restoringTaskIds(input.id);
    for (const id of preserved) {
      restoringDrafts.set(id, (restoringDrafts.get(id) || 0) + 1);
      const timer = timers.get(id);
      if (timer) clearTimeout(timer);
      timers.delete(id);
    }
    try {
      for (const id of Object.keys(state.drafts))
        if (!preserved.has(id)) await flushTask(id);
      if (
        Object.entries(state.drafts).some(
          ([id, draft]) => !preserved.has(id) && draft.failed,
        )
      )
        throw new Error("Save or retry your pending task changes first.");
      // Restore changes only lifecycle state. It must neither publish a held
      // title nor adopt a newer CLI version as an implicit conflict Retry.
      return await enqueue<T>(action, input, undefined, preserved);
    } finally {
      for (const id of preserved) {
        const remaining = (restoringDrafts.get(id) || 1) - 1;
        if (remaining) restoringDrafts.set(id, remaining);
        else {
          restoringDrafts.delete(id);
          const draft = state.drafts[id];
          if (draft && !draft.failed) {
            const timer = timers.get(id);
            if (timer) clearTimeout(timer);
            timers.set(
              id,
              setTimeout(() => void flushTask(id), 450),
            );
          }
        }
      }
    }
  }
  if (
    action.startsWith("tasks.") ||
    action.startsWith("attachments.") ||
    action.startsWith("comments.") ||
    action.startsWith("links.") ||
    action.startsWith("tags.") ||
    action.startsWith("templates.") ||
    action.startsWith("sections.") ||
    action === "lists.delete" ||
    action === "folders.delete"
  ) {
    if (Object.keys(state.drafts).some((id) => restoringDrafts.has(id)))
      throw new Error(
        "Wait for task restoration to finish before changing tasks.",
      );
    for (const id of Object.keys(state.drafts)) await flushTask(id);
    if (Object.values(state.drafts).some((d) => d.failed))
      throw new Error("Save or retry your pending task changes first.");
  }
  return enqueue<T>(action, input);
}
function editTask(
  id: string,
  patch: Record<string, unknown>,
  baseVersion?: number,
) {
  const task = state.snapshot.tasks.find((t) => t.id === id);
  if (!task) return;
  if (!state.drafts[id])
    state.drafts[id] = {
      patch: {},
      version: baseVersion ?? task.version,
      failed: false,
    };
  else if (baseVersion !== undefined)
    state.drafts[id].version = Math.min(state.drafts[id].version, baseVersion);
  Object.assign(state.drafts[id].patch, patch);
  if (
    state.drafts[id].failed &&
    typeof patch.title === "string" &&
    patch.title.trim()
  )
    state.drafts[id].failed = false;
  const timer = timers.get(id);
  if (timer) clearTimeout(timer);
  timers.set(
    id,
    setTimeout(() => void flushTask(id), 450),
  );
}
async function flushTask(id: string): Promise<void> {
  if (restoringDrafts.has(id)) return;
  const existing = saving.get(id);
  if (existing) {
    await existing;
    if (state.drafts[id] && !state.drafts[id].failed) await flushTask(id);
    return;
  }
  // Let earlier metadata operations acknowledge and safely advance the draft
  // before capturing the version that will be sent to SQLite.
  await operation;
  if (restoringDrafts.has(id)) return;
  const startedWhileWaiting = saving.get(id);
  if (startedWhileWaiting) {
    await startedWhileWaiting;
    if (state.drafts[id] && !state.drafts[id].failed) await flushTask(id);
    return;
  }
  const draft = state.drafts[id];
  if (!draft || draft.failed) return;
  const patch = { ...draft.patch };
  if (typeof patch.title === "string" && !patch.title.trim()) {
    state.error = "A task needs a title. Your changes are kept here.";
    draft.failed = true;
    return;
  }
  const work = (async () => {
    try {
      const saved = await enqueue<Task>("tasks.update", {
        id,
        ...patch,
        expected_version: draft.version,
      });
      const current = state.drafts[id];
      if (current) {
        for (const [key, value] of Object.entries(patch))
          if (current.patch[key] === value) delete current.patch[key];
        current.version = saved.version;
        if (!Object.keys(current.patch).length) delete state.drafts[id];
      }
    } catch {
      const current = state.drafts[id];
      if (current) current.failed = true;
      state.error =
        "Changes could not be saved. Your draft is kept. Retry to apply it to the latest task.";
    }
  })();
  saving.set(id, work);
  await work;
  if (saving.get(id) === work) saving.delete(id);
}
function writeCheckpoint() {
  return writeFailures;
}
async function flushAll(failuresBefore = writeFailures) {
  for (;;) {
    const queued = operation;
    await queued;
    if (writeFailures !== failuresBefore) return false;
    for (const id of Object.keys(state.drafts)) await flushTask(id);
    await Promise.all([...saving.values()]);
    if (Object.values(state.drafts).some((d) => d.failed)) return false;
    if (
      queued === operation &&
      !saving.size &&
      !Object.keys(state.drafts).length
    )
      return true;
  }
}
async function retryDrafts() {
  await refresh();
  for (const [id, draft] of Object.entries(state.drafts)) {
    const latest = state.snapshot.tasks.find((t) => t.id === id);
    if (!latest) continue;
    draft.version = latest.version;
    draft.failed = false;
    await flushTask(id);
  }
  if (!Object.values(state.drafts).some((d) => d.failed)) state.error = "";
}
const tasks = computed(() =>
  state.snapshot.tasks.map(
    (t) => ({ ...t, ...state.drafts[t.id]?.patch }) as Task,
  ),
);
const archivedIds = computed(
  () =>
    new Set(state.snapshot.lists.filter((l) => l.archived_at).map((l) => l.id)),
);
const hiddenSmartListIds = computed(
  () =>
    new Set(
      state.snapshot.lists
        .filter((list) => list.show_in_smart_lists === false)
        .map((list) => list.id),
    ),
);
const active = computed(() =>
  tasks.value.filter(
    (t) =>
      !t.completed_at &&
      !t.deleted_at &&
      !t.wont_do_at &&
      !t.parent_id &&
      !archivedIds.value.has(t.list_id) &&
      !hiddenSmartListIds.value.has(t.list_id),
  ),
);
export const defaultViewPreferences: ViewPreferences = {
  view_mode: "list",
  group_by: "none",
  sort_by: "manual",
  sort_order: "asc",
};
const viewPreferenceKey = computed(() =>
  ["tag", "filter"].includes(state.view)
    ? `${state.view}:${state.listId}`
    : state.view,
);
const viewPreferences = computed<ViewPreferences>(() => {
  const defaults = {
    ...defaultViewPreferences,
    group_by: ["today", "tomorrow", "upcoming"].includes(state.view)
      ? ("date" as const)
      : ("none" as const),
  };
  if (state.view === "list") {
    const list = state.snapshot.lists.find((l) => l.id === state.listId);
    return list
      ? {
          ...defaults,
          view_mode: list.view_mode || "list",
          group_by: list.group_by || "none",
          sort_by: list.sort_by || "manual",
          sort_order: list.sort_order || "asc",
        }
      : defaults;
  }
  return {
    ...defaults,
    ...((
      state.snapshot.settings.view_preferences as
        Record<string, Partial<ViewPreferences>> | undefined
    )?.[viewPreferenceKey.value] || {}),
  };
});
async function setViewPreferences(patch: Partial<ViewPreferences>) {
  if (state.view === "list" && state.listId)
    return mutate("lists.update", { id: state.listId, ...patch });
  const all = (state.snapshot.settings.view_preferences || {}) as Record<
    string,
    Partial<ViewPreferences>
  >;
  return mutate("settings.set", {
    key: "view_preferences",
    value: {
      ...all,
      [viewPreferenceKey.value]: { ...viewPreferences.value, ...patch },
    },
  });
}
export function matchesFilter(
  task: Task,
  filter: SavedFilter,
  today = localDate(),
) {
  const tomorrow = new Date(`${today}T12:00:00`);
  tomorrow.setDate(tomorrow.getDate() + 1);
  const end = new Date(`${today}T12:00:00`);
  end.setDate(end.getDate() + 6);
  const checks: boolean[] = [];
  if (filter.list_ids.length)
    checks.push(filter.list_ids.includes(task.list_id));
  if (filter.tag_ids.length)
    checks.push(filter.tag_ids.some((id) => (task.tag_ids || []).includes(id)));
  if (filter.priorities.length)
    checks.push(filter.priorities.includes(task.priority));
  if (filter.date !== "all")
    checks.push(
      filter.date === "undated"
        ? !task.due_date
        : !!task.due_date &&
            (filter.date === "today"
              ? task.due_date === today
              : filter.date === "tomorrow"
                ? task.due_date === localDate(tomorrow)
                : filter.date === "overdue"
                  ? task.due_date < today
                  : task.due_date >= today && task.due_date <= localDate(end)),
    );
  return (
    !checks.length ||
    (filter.match === "any" ? checks.some(Boolean) : checks.every(Boolean))
  );
}
const visibleTasks = computed(() => {
  const end = new Date();
  end.setDate(end.getDate() + 6);
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  const today = localDate(),
    until = localDate(end);
  const filter = (
    (state.snapshot.settings.saved_filters || []) as SavedFilter[]
  ).find((f) => f.id === state.listId);
  const data = tasks.value.filter((task) => {
    const parent = task.parent_id
      ? tasks.value.find((t) => t.id === task.parent_id)
      : undefined;
    if (state.view === "trash") return !!task.deleted_at && !parent?.deleted_at;
    if (task.deleted_at) return false;
    if (state.view === "completed")
      return !!task.completed_at && !parent?.completed_at;
    if (state.view === "wont_do")
      return !!task.wont_do_at && !parent?.wont_do_at;
    if (task.wont_do_at) return false;
    if (task.completed_at && !state.showCompleted) return false;
    if (state.view !== "list" && archivedIds.value.has(task.list_id))
      return false;
    if (
      !["search", "list"].includes(state.view) &&
      hiddenSmartListIds.value.has(task.list_id)
    )
      return false;
    if (state.view === "search") {
      return taskMatchesSearch(task, state.query);
    }
    if (state.view === "today")
      return !!task.due_date && task.due_date <= today;
    if (state.view === "tomorrow") return task.due_date === localDate(tomorrow);
    if (state.view === "upcoming")
      return (
        !!task.due_date && task.due_date >= today && task.due_date <= until
      );
    if (state.view === "tag")
      return !!state.listId && !!task.tag_ids?.includes(state.listId);
    if (state.view === "filter")
      return !!filter && matchesFilter(task, filter, today);
    if (task.parent_id) return false;
    if (state.view === "inbox") return task.list_id === "inbox";
    if (state.view === "list") return task.list_id === state.listId;
    if (state.view === "archived") return false;
    return true;
  });
  const preference = viewPreferences.value;
  return data.sort((a, b) => {
    const pin = Number(!!b.pinned) - Number(!!a.pinned);
    if (pin) return pin;
    const completion = Number(!!a.completed_at) - Number(!!b.completed_at);
    if (completion) return completion;
    let order = 0;
    if (preference.sort_by === "priority") order = b.priority - a.priority;
    else if (preference.sort_by === "due_date")
      order =
        (a.due_date || "9999").localeCompare(b.due_date || "9999") ||
        (a.due_time || "").localeCompare(b.due_time || "");
    else if (preference.sort_by === "title")
      order = a.title.localeCompare(b.title);
    else if (preference.sort_by === "created_at")
      order = a.created_at.localeCompare(b.created_at);
    else order = a.position - b.position;
    if (preference.sort_order === "desc") order *= -1;
    return order || a.created_at.localeCompare(b.created_at);
  });
});
const selected = computed(
  () => tasks.value.find((t) => t.id === state.selectedId) || null,
);
function navigate(view: View, id: string | null = null) {
  state.view = view;
  state.listId = id;
  state.selectedId = null;
  state.sidebarOpen = false;
  if (view !== "search") state.query = "";
}

export function useTasks() {
  return {
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
    editTask,
    flushTask,
    flushAll,
    writeCheckpoint,
    retryDrafts,
    navigate,
  };
}
