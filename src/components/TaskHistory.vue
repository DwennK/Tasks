<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from "vue";
import { dispatch } from "../adapter";
import { useTasks } from "../composables/useTasks";
import { noteText } from "../lib/note-text";
import AppDialog from "./AppDialog.vue";

type ContentVersion = { title: string; notes: string; notes_format: string };
type ActivityState = Record<string, unknown> & { content?: ContentVersion };
type Activity = {
  id: number;
  action: string;
  created_at: string;
  changed_fields: string[];
  before: ActivityState | null;
  after: ActivityState;
};
type ActivityPage = {
  items: Activity[];
  next_before_id: number | null;
  task_version: number;
  task_deleted: boolean;
};
const props = defineProps<{ taskId: string }>();
const emit = defineEmits<{ close: []; restored: [] }>();
const { state, mutate, flushTask } = useTasks();
const activities = ref<Activity[]>([]);
const nextBeforeId = ref<number | null>(null);
const version = ref(0);
const deleted = ref(false);
const loading = ref(false);
const restoring = ref(false);
const error = ref("");
const message = ref("");
let request = 0;

const labels: Record<string, string> = {
  content: "Title and description",
  list_id: "List",
  parent_id: "Parent task",
  due_date: "Date",
  due_time: "Time",
  reminder_at: "Reminder",
  reminders: "Reminders",
  priority: "Priority",
  position: "Order",
  completed_at: "Completed",
  deleted_at: "Trash",
  start_date: "Start date",
  start_time: "Start time",
  duration_minutes: "Duration",
  timezone: "Time zone",
  recurrence: "Repeat",
  recurrence_next_id: "Next occurrence",
  is_note: "Note",
  checklist: "Checklist",
  pinned: "Pinned",
  wont_do_at: "Won’t do",
  section_id: "Section",
  tag_ids: "Tags",
  attachments: "Attachments",
  linked_task_ids: "Linked tasks",
  comments: "Comments",
  comment_attachments: "Comment attachments",
  progress: "Progress",
};
function activityTitle(activity: Activity) {
  if (!activity.before) return "Created task";
  if (activity.action === "tasks.restore_content")
    return "Restored title and description";
  if (activity.changed_fields.includes("deleted_at"))
    return activity.after.deleted_at ? "Moved to Trash" : "Restored task";
  if (
    activity.changed_fields.includes("wont_do_at") &&
    activity.after.wont_do_at
  )
    return "Marked as Won’t Do";
  if (activity.changed_fields.includes("completed_at"))
    return activity.after.completed_at ? "Completed task" : "Reopened task";
  const actions: Record<string, string> = {
    "tasks.reorder": "Moved task",
    "tasks.tags": "Updated tags",
    "tags.delete": "Removed tag",
    "links.add": "Linked task",
    "links.remove": "Unlinked task",
    "attachments.add": "Added attachment",
    "attachments.import": "Added attachment",
    "attachments.remove": "Removed attachment",
    "comments.add": "Added comment",
    "comments.update": "Edited comment",
    "comments.delete": "Deleted comment",
    "sections.delete": "Removed section",
    "reminders.resolve": "Updated reminder",
  };
  return actions[activity.action] || "Updated task";
}
function timeLabel(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}
function displayValue(key: string, value: unknown): string {
  if (value === null || value === undefined || value === "") return "None";
  if (key === "list_id")
    return (
      state.snapshot.lists.find((list) => list.id === value)?.name ||
      String(value)
    );
  if (key === "parent_id" || key === "recurrence_next_id")
    return (
      state.snapshot.tasks.find((task) => task.id === value)?.title ||
      String(value)
    );
  if (key === "priority")
    return ["None", "Low", "Medium", "High"][Number(value)] || String(value);
  if (key === "progress") return `${value}%`;
  if (typeof value === "boolean") return value ? "Yes" : "No";
  if (Array.isArray(value)) {
    if (!value.length) return "None";
    if (key === "tag_ids")
      return value
        .map(
          (id) => state.snapshot.tags.find((tag) => tag.id === id)?.name || id,
        )
        .join(", ");
    if (key === "linked_task_ids")
      return value
        .map(
          (id) =>
            state.snapshot.tasks.find((task) => task.id === id)?.title || id,
        )
        .join(", ");
    return value
      .map((item) => {
        if (item && typeof item === "object")
          return (
            item.body ||
            item.name ||
            item.title ||
            item.text ||
            JSON.stringify(item)
          );
        return String(item);
      })
      .join("\n");
  }
  if (typeof value === "object") return JSON.stringify(value, null, 2);
  return String(value);
}
function matchesCurrent(content: ContentVersion) {
  const task = state.snapshot.tasks.find((item) => item.id === props.taskId);
  return (
    task?.title === content.title &&
    task.notes === content.notes &&
    task.notes_format === content.notes_format
  );
}
async function load(append = false) {
  const token = ++request;
  const taskId = props.taskId;
  loading.value = true;
  error.value = "";
  try {
    if (!append) await flushTask(taskId);
    const page = await dispatch<ActivityPage>("tasks.activities", {
      id: taskId,
      limit: 30,
      ...(append && nextBeforeId.value
        ? { before_id: nextBeforeId.value }
        : {}),
    });
    if (token !== request) return;
    activities.value = append
      ? [...activities.value, ...page.items]
      : page.items;
    nextBeforeId.value = page.next_before_id;
    if (!append) {
      version.value = page.task_version;
      deleted.value = page.task_deleted;
    }
  } catch (cause) {
    if (token === request) error.value = String(cause);
  } finally {
    if (token === request) loading.value = false;
  }
}
async function restore(activity: Activity, side: "before" | "after") {
  if (restoring.value || loading.value) return;
  restoring.value = true;
  error.value = "";
  message.value = "";
  try {
    await mutate("tasks.restore_content", {
      id: props.taskId,
      activity_id: activity.id,
      side,
      expected_version: version.value,
    });
    message.value = "Title and description restored.";
    await load();
    emit("restored");
  } catch (cause) {
    error.value = String(cause);
  } finally {
    restoring.value = false;
  }
}
watch(
  () => props.taskId,
  () => {
    activities.value = [];
    nextBeforeId.value = null;
    message.value = "";
    void load();
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  request++;
});
</script>

<template>
  <AppDialog
    title="Task Activities"
    wide
    panel-class="task-history-dialog"
    @close="emit('close')"
  >
    <div class="task-history">
      <div class="history-toolbar">
        <p>Activity is recorded from this version of Tasks.</p>
        <button
          class="history-button"
          :disabled="loading || restoring"
          @click="load()"
        >
          Refresh
        </button>
      </div>
      <p v-if="error" class="history-error" role="alert">{{ error }}</p>
      <p v-if="message" class="history-message" role="status">{{ message }}</p>
      <p v-if="deleted" class="history-notice">
        Restore this task from Trash to restore its title or description.
      </p>
      <p
        v-if="loading && !activities.length"
        class="history-empty"
        role="status"
      >
        Loading activities…
      </p>
      <p v-else-if="!activities.length && !error" class="history-empty">
        No activities yet. Future changes will appear here.
      </p>
      <ol v-else class="history-list" :aria-busy="loading">
        <li
          v-for="activity in activities"
          :key="activity.id"
          class="history-item"
        >
          <div class="history-heading">
            <strong>{{ activityTitle(activity) }}</strong>
            <time :datetime="activity.created_at">{{
              timeLabel(activity.created_at)
            }}</time>
          </div>
          <p v-if="activity.before" class="history-fields">
            {{
              activity.changed_fields
                .map((key) => labels[key] || key.replaceAll("_", " "))
                .join(" · ")
            }}
          </p>
          <details
            v-if="activity.after.content"
            class="history-details"
            :open="activity.id === activities[0]?.id"
          >
            <summary>View title and description</summary>
            <div class="history-versions">
              <section
                v-for="side in ['before', 'after'] as const"
                v-show="activity[side]?.content"
                :key="side"
                class="history-version"
              >
                <template v-if="activity[side]?.content">
                  <h3>{{ side === "before" ? "Before" : "After" }}</h3>
                  <h4>{{ activity[side]!.content!.title }}</h4>
                  <p class="history-notes">
                    {{
                      noteText(
                        activity[side]!.content!.notes,
                        activity[side]!.content!.notes_format,
                      ) || "No description"
                    }}
                  </p>
                  <button
                    class="history-button"
                    :disabled="
                      deleted ||
                      restoring ||
                      loading ||
                      matchesCurrent(activity[side]!.content!)
                    "
                    @click="restore(activity, side)"
                  >
                    {{
                      matchesCurrent(activity[side]!.content!)
                        ? "Current content"
                        : side === "before"
                          ? "Restore previous content"
                          : "Restore this content"
                    }}
                  </button>
                </template>
              </section>
            </div>
          </details>
          <details
            v-if="
              activity.before &&
              activity.changed_fields.some((key) => key !== 'content')
            "
            class="history-details"
          >
            <summary>View changes</summary>
            <dl class="history-changes">
              <template
                v-for="key in activity.changed_fields.filter(
                  (key) => key !== 'content',
                )"
                :key="key"
              >
                <dt>{{ labels[key] || key.replaceAll("_", " ") }}</dt>
                <dd>
                  <span>{{ displayValue(key, activity.before?.[key]) }}</span
                  ><span aria-hidden="true"> → </span
                  ><span class="sr-only"> changed to </span
                  ><span>{{ displayValue(key, activity.after[key]) }}</span>
                </dd>
              </template>
            </dl>
          </details>
        </li>
      </ol>
      <button
        v-if="nextBeforeId"
        class="history-button history-more"
        :disabled="loading || restoring"
        @click="load(true)"
      >
        {{ loading ? "Loading…" : "Load earlier activities" }}
      </button>
    </div>
  </AppDialog>
</template>

<style scoped>
:global(.task-history-dialog) {
  width: min(680px, calc(100vw - 32px));
}
.task-history {
  max-height: min(65vh, 620px);
  overflow-y: auto;
  padding: 0 2px 2px;
}
.history-toolbar,
.history-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.history-toolbar {
  margin-bottom: 16px;
}
.history-toolbar p,
.history-notice,
.history-fields,
time {
  color: var(--secondary);
  font-size: 12px;
}
.history-toolbar p {
  margin: 0;
}
.history-button {
  flex-shrink: 0;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 10px;
  background: var(--surface);
  color: var(--text);
  font-size: 12px;
}
.history-button:not(:disabled):hover {
  border-color: var(--accent);
  color: var(--accent);
}
.history-button:disabled {
  opacity: 0.5;
  cursor: default;
}
.history-empty {
  padding: 42px 12px;
  text-align: center;
  color: var(--secondary);
}
.history-list {
  margin: 0;
  padding: 0;
  list-style: none;
}
.history-item {
  border-top: 1px solid var(--border);
  padding: 17px 0;
}
.history-heading strong {
  font-size: 13px;
  font-weight: 550;
}
.history-heading time {
  text-align: right;
  flex-shrink: 0;
}
.history-fields {
  margin: 5px 0 0;
  line-height: 1.5;
}
.history-details {
  margin-top: 12px;
}
.history-details summary {
  cursor: pointer;
  font-size: 12px;
  color: var(--secondary);
}
.history-versions {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(210px, 100%), 1fr));
  gap: 12px;
  margin-top: 12px;
}
.history-version {
  min-width: 0;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px;
}
.history-version h3 {
  margin: 0 0 10px;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--secondary);
}
.history-version h4 {
  margin: 0 0 8px;
  font-size: 13px;
  overflow-wrap: anywhere;
}
.history-notes {
  margin: 0 0 14px;
  max-height: 210px;
  overflow-y: auto;
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.history-changes {
  margin: 12px 0 0;
  font-size: 12px;
  line-height: 1.5;
}
.history-changes dt {
  font-weight: 550;
  margin-top: 10px;
}
.history-changes dd {
  margin: 2px 0 0;
  color: var(--secondary);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 170px;
  overflow: auto;
}
.history-error {
  color: var(--danger, #dc4a4a);
  font-size: 12px;
}
.history-message {
  color: var(--accent);
  font-size: 12px;
}
.history-more {
  display: block;
  margin: 8px auto 0;
}
@media (max-width: 600px) {
  .task-history {
    padding-inline: 0;
  }
  .history-heading {
    align-items: flex-start;
    flex-direction: column;
    gap: 4px;
  }
}
</style>
