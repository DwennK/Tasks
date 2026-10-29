<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import { invoke } from "@tauri-apps/api/core";
import { registerEscapeLayer } from "../lib/escape-layers";
import {
  PhArrowLeft,
  PhArrowCounterClockwise,
  PhArrowSquareOut,
  PhBell,
  PhCalendarDots,
  PhCaretDown,
  PhChatCircleText,
  PhCheck,
  PhCopy,
  PhDotsThree,
  PhDownloadSimple,
  PhFile,
  PhFlag,
  PhImage,
  PhLink,
  PhList,
  PhListBullets,
  PhPaperclip,
  PhPlus,
  PhPushPin,
  PhRepeat,
  PhSmiley,
  PhTag,
  PhTextAa,
  PhTrash,
  PhX,
  PhXSquare,
  PhPrinter,
  PhFloppyDisk,
  PhTreeStructure,
  PhArrowsInSimple,
  PhNote,
  PhExport,
} from "@phosphor-icons/vue";
import {
  IconCalendarWeek,
  IconFlag,
  IconFlagFilled,
  IconMessage2,
  IconListDetails,
  IconTypography,
} from "@tabler/icons-vue";
import TaskNavigationGlyph from "./TaskNavigationGlyph.vue";
import type { Attachment, Tag, Task } from "../types";
import { dispatch, isNative } from "../adapter";
import { dateLabel, localDate, useTasks } from "../composables/useTasks";
import { useTaskTitleField } from "../composables/useTaskTitleField";
import TaskHistory from "./TaskHistory.vue";
import TaskProgress from "./TaskProgress.vue";
import AppDialog from "./AppDialog.vue";
import RichNotes from "./RichNotes.vue";
import TaskDatePicker from "./TaskDatePicker.vue";
import ParentTaskPicker from "./ParentTaskPicker.vue";
import TaskExportDialog from "./TaskExportDialog.vue";
import { beginEditorWrite, waitForEditorWrites } from "./editorWork";

const props = defineProps<{ task: Task }>();
const emit = defineEmits<{ close: []; "open-task": [id: string] }>();
const { state, tasks, mutate, editTask, flushAll, refresh } = useTasks();
const menu = ref<"date" | "priority" | "more" | "tags" | "move" | null>(null);
const dialog = ref<"link" | "parent" | "template" | "export" | null>(null);
const titleInput = ref<HTMLTextAreaElement>();
const titleField = useTaskTitleField(() => props.task, titleInput);
const richNotes = ref<InstanceType<typeof RichNotes>>();
const fileInput = ref<HTMLInputElement>();
const imageInput = ref<HTMLInputElement>();
const subtaskInput = ref<HTMLInputElement>();
const commentInput = ref<HTMLTextAreaElement>();
const tagInput = ref<HTMLInputElement>();
const subtaskTitle = ref("");
const showSubtasks = ref(false);
const showCompletedSubtasks = ref(false);
const toolbar = ref(false);
const historyOpen = ref(false);
watch(
  () => props.task.id,
  () => {
    historyOpen.value = false;
  },
);
const focusMode = ref(false);
const commentsOpen = ref(false);
const emojiOpen = ref(false);
const commentBody = ref("");
const commentAttachments = ref<string[]>([]);
const editingComment = ref<string | null>(null);
const editingCommentVersion = ref<number | null>(null);
const adding = ref(false);
const uploading = ref(false);
const localError = ref("");
const tagQuery = ref("");
const linkQuery = ref("");
const templateName = ref("");
const notice = ref("");
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
const children = computed(() =>
  tasks.value
    .filter(
      (task) =>
        task.parent_id === props.task.id &&
        !task.deleted_at &&
        !task.wont_do_at,
    )
    .sort((a, b) => a.position - b.position),
);
const activeChildren = computed(() =>
  children.value.filter((task) => !task.completed_at),
);
const completedChildren = computed(() =>
  children.value.filter((task) => !!task.completed_at),
);
const parent = computed(() =>
  tasks.value.find((task) => task.id === props.task.parent_id),
);
const currentList = computed(() =>
  state.snapshot.lists.find((list) => list.id === props.task.list_id),
);
const taskTags = computed(() =>
  state.snapshot.tags.filter((tag) => props.task.tag_ids?.includes(tag.id)),
);
const filteredTags = computed(() =>
  state.snapshot.tags.filter((tag) =>
    tag.name
      .toLocaleLowerCase()
      .includes(tagQuery.value.trim().replace(/^#/, "").toLocaleLowerCase()),
  ),
);
const linkedTasks = computed(() =>
  tasks.value.filter((task) => props.task.linked_task_ids?.includes(task.id)),
);
const linkCandidates = computed(() =>
  tasks.value
    .filter(
      (task) =>
        task.id !== props.task.id &&
        !task.deleted_at &&
        !props.task.linked_task_ids?.includes(task.id) &&
        task.title
          .toLocaleLowerCase()
          .includes(linkQuery.value.toLocaleLowerCase()),
    )
    .slice(0, 50),
);
const sections = computed(() =>
  state.snapshot.sections.filter(
    (section) => section.list_id === props.task.list_id,
  ),
);
type LocalComment = {
  id: string;
  task_id: string;
  body: string;
  author?: string;
  created_at: string;
  updated_at: string;
  version: number;
  attachment_ids?: string[];
};
const comments = computed(
  () => (props.task as Task & { comments?: LocalComment[] }).comments || [],
);
const commentsDrafts = new Map<
  string,
  {
    body: string;
    attachments: string[];
    editing: string | null;
    version: number | null;
  }
>();
function calendarDay(value: string) {
  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(0);
  date.setUTCFullYear(year, month - 1, day);
  return date.getTime() / 86_400_000;
}
const overdueDays = computed(() => {
  if (
    !props.task.due_date ||
    props.task.completed_at ||
    props.task.wont_do_at ||
    props.task.deleted_at
  )
    return 0;
  // Compare calendar components in UTC, not elapsed local-midnight hours.
  return Math.max(
    0,
    calendarDay(localDate()) - calendarDay(props.task.due_date),
  );
});
const dueText = computed(() => {
  if (!props.task.due_date) return "Due Date";
  const dueDate = new Date(`${props.task.due_date}T12:00:00`);
  let label = overdueDays.value
    ? `${dueDate.getDate()} ${dueDate.toLocaleDateString("en", { month: "short" })}`
    : dateLabel(props.task.due_date);
  if (props.task.due_time) {
    const [hours, minutes] = props.task.due_time.split(":");
    const time =
      Number(state.snapshot.settings.time_format) === 12
        ? `${Number(hours) % 12 || 12}:${minutes} ${Number(hours) >= 12 ? "PM" : "AM"}`
        : props.task.due_time;
    label += `, ${time}`;
  }
  return overdueDays.value ? `${label}, ${overdueDays.value}d overdue` : label;
});

function message(value: string) {
  notice.value = value;
  if (noticeTimer) clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => {
    notice.value = "";
  }, 2500);
}
function dismissMenu(event: PointerEvent) {
  if (
    !(event.target instanceof Element) ||
    event.target.closest(
      ".popover, .popover-anchor, .task-date-picker, .tag-picker, .emoji-picker",
    )
  )
    return;
  menu.value = null;
  emojiOpen.value = false;
}
const editorRoot = ref<HTMLElement>();
let unregisterEscapeLayer: (() => void) | undefined;
function editorEscapeFromOutside(event: KeyboardEvent) {
  if (event.target instanceof Node && editorRoot.value?.contains(event.target))
    return;
  menuEscape(event);
}
function menuEscape(event: KeyboardEvent) {
  if (event.key !== "Escape" || event.isComposing || event.defaultPrevented)
    return;
  if (dialog.value || menu.value === "date") return;
  if (menu.value || toolbar.value || focusMode.value) {
    event.preventDefault();
    event.stopImmediatePropagation();
    if (menu.value) menu.value = null;
    else if (toolbar.value) {
      toolbar.value = false;
      richNotes.value?.closeMenus();
    } else focusMode.value = false;
  }
}
onMounted(() => {
  unregisterEscapeLayer = registerEscapeLayer(
    editorEscapeFromOutside,
    () =>
      !dialog.value &&
      menu.value !== "date" &&
      !!(menu.value || toolbar.value || focusMode.value),
  );
  document.addEventListener("pointerdown", dismissMenu);
  document.addEventListener("keydown", menuEscape);
  resizeTitle();
});
onBeforeUnmount(() => {
  unregisterEscapeLayer?.();
  reconcileEditorDrafts();
  document.removeEventListener("pointerdown", dismissMenu);
  document.removeEventListener("keydown", menuEscape);
  if (noticeTimer) clearTimeout(noticeTimer);
});
watch(
  () => props.task.id,
  (_next, previous) => {
    richNotes.value?.reconcile();
    commentsDrafts.set(previous, {
      body: commentBody.value,
      attachments: [...commentAttachments.value],
      editing: editingComment.value,
      version: editingCommentVersion.value,
    });
    const draft = commentsDrafts.get(props.task.id);
    commentBody.value = draft?.body || "";
    commentAttachments.value = draft?.attachments || [];
    editingComment.value = draft?.editing || null;
    editingCommentVersion.value = draft?.version || null;
    menu.value = null;
    dialog.value = null;
    localError.value = "";
    subtaskTitle.value = "";
    showSubtasks.value = false;
    showCompletedSubtasks.value = false;
    nextTick(resizeTitle);
  },
);
watch(
  () => props.task.title,
  () => nextTick(resizeTitle),
);
watch(menu, (value) => {
  if (value === "tags") nextTick(() => tagInput.value?.focus());
});
function resizeTitle() {
  if (!titleInput.value) return;
  titleInput.value.style.height = "auto";
  titleInput.value.style.height = `${Math.max(28, titleInput.value.scrollHeight)}px`;
}
function changeTitle(event: Event) {
  titleField.input(event);
  resizeTitle();
}
function titleCompositionEnd() {
  titleField.compositionEnd();
  resizeTitle();
}
function titleKeydown(event: KeyboardEvent) {
  if (
    event.isComposing ||
    titleField.composing.value ||
    event.keyCode === 229
  ) {
    event.stopPropagation();
    return;
  }
  if (event.key !== "Enter") return;
  event.preventDefault();
  focusNotes();
}
function changeNotes(id: string, notes: string) {
  editTask(id, { notes, notes_format: "html" });
}
function reconcileEditorDrafts() {
  richNotes.value?.reconcile();
  titleField.reconcile();
}
function openDate() {
  menu.value = menu.value === "date" ? null : "date";
}
function openPriority() {
  menu.value = "priority";
}
function openTags() {
  menu.value = "tags";
  tagQuery.value = "";
}
function focusNotes() {
  richNotes.value?.focus();
}
async function focusSubtask() {
  menu.value = null;
  showSubtasks.value = true;
  await nextTick();
  subtaskInput.value?.focus();
}
defineExpose({
  reconcileEditorDrafts,
  focusSubtask,
  openDate,
  openPriority,
  openTags,
  focusNotes,
  printTask,
  waitForPendingWrites: waitForEditorWrites,
});
async function update(patch: Record<string, unknown>) {
  try {
    reconcileEditorDrafts();
    await mutate("tasks.update", { id: props.task.id, ...patch });
    menu.value = null;
  } catch (error) {
    localError.value = String(error);
  }
}
async function complete(task: Task) {
  try {
    await mutate("tasks.complete", {
      id: task.id,
      completed: !task.completed_at,
    });
  } catch (error) {
    localError.value = String(error);
  }
}
async function addSubtask() {
  if (!subtaskTitle.value.trim() || adding.value) return;
  adding.value = true;
  try {
    await mutate("tasks.create", {
      title: subtaskTitle.value.trim(),
      parent_id: props.task.id,
    });
    subtaskTitle.value = "";
    await nextTick();
    subtaskInput.value?.focus();
  } catch (error) {
    localError.value = String(error);
  } finally {
    adding.value = false;
  }
}
function openTask(id: string) {
  state.selectedId = id;
  emit("open-task", id);
}
async function remove() {
  try {
    await mutate("tasks.delete", { id: props.task.id });
    menu.value = null;
    emit("close");
  } catch (error) {
    localError.value = String(error);
  }
}
async function restore() {
  try {
    await mutate("tasks.restore", { id: props.task.id });
    menu.value = null;
  } catch (error) {
    localError.value = String(error);
  }
}
async function duplicate() {
  try {
    const task = await mutate<Task>("tasks.duplicate", { id: props.task.id });
    menu.value = null;
    openTask(task.id);
  } catch (error) {
    localError.value = String(error);
  }
}
async function dismissTask() {
  try {
    await mutate("tasks.wont_do", {
      id: props.task.id,
      wont_do: !props.task.wont_do_at,
    });
    menu.value = null;
  } catch (error) {
    localError.value = String(error);
  }
}
async function saveTemplate() {
  if (!templateName.value.trim()) return;
  try {
    await mutate("templates.create", {
      name: templateName.value.trim(),
      task_id: props.task.id,
    });
    dialog.value = null;
    message("Template saved");
  } catch (error) {
    localError.value = String(error);
  }
}
async function copyTaskLink() {
  try {
    const text = `tasks://task/${props.task.id}`;
    if (isNative) await invoke("copy_text", { text });
    else await navigator.clipboard.writeText(text);
    menu.value = null;
    message("Task link copied");
  } catch {
    localError.value = "The task link could not be copied.";
  }
}
async function setParent(parentTask: Task | null) {
  try {
    reconcileEditorDrafts();
    await mutate("tasks.update", {
      id: props.task.id,
      parent_id: parentTask?.id || null,
    });
    dialog.value = null;
  } catch (error) {
    localError.value = String(error);
  }
}
function openExport() {
  reconcileEditorDrafts();
  menu.value = null;
  dialog.value = "export";
}
function toggleTag(tag: Tag) {
  const ids = new Set(props.task.tag_ids || []);
  if (ids.has(tag.id)) ids.delete(tag.id);
  else ids.add(tag.id);
  // Like notes, tag replacement is a versioned draft based on the snapshot
  // the user saw. Queuing an unguarded full array can erase a concurrent tag.
  editTask(props.task.id, { tag_ids: [...ids] });
}
async function createTag() {
  const name = tagQuery.value.trim().replace(/^#/, "");
  if (!name) return;
  const existing = state.snapshot.tags.find(
    (tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase(),
  );
  if (existing) {
    toggleTag(existing);
    tagQuery.value = "";
    return;
  }
  const taskId = props.task.id;
  try {
    const tag = await mutate<Tag>("tags.create", { name });
    const task = tasks.value.find((value) => value.id === taskId);
    if (task)
      editTask(taskId, {
        tag_ids: [...new Set([...(task.tag_ids || []), tag.id])],
      });
    if (props.task.id === taskId) tagQuery.value = "";
  } catch (error) {
    localError.value = String(error);
  }
}
async function linkTask(task: Task) {
  try {
    await mutate("links.add", { id: props.task.id, linked_task_id: task.id });
    dialog.value = null;
  } catch (error) {
    localError.value = String(error);
  }
}
async function unlinkTask(task: Task) {
  try {
    await mutate("links.remove", {
      id: props.task.id,
      linked_task_id: task.id,
    });
  } catch (error) {
    localError.value = String(error);
  }
}
async function attachFiles() {
  menu.value = null;
  if (!isNative) {
    fileInput.value?.click();
    return;
  }
  if (uploading.value) return;
  uploading.value = true;
  const finishWork = beginEditorWrite();
  const taskId = props.task.id;
  let failure: unknown;
  try {
    reconcileEditorDrafts();
    if (!(await flushAll())) return;
    const result = await invoke<{
      attachments: Attachment[];
      errors: string[];
    }>("import_attachments", { taskId });
    await refresh(true);
    if (result.errors.length) {
      localError.value = result.errors.join("\n");
      failure = new Error(localError.value);
    }
  } catch (error) {
    localError.value = String(error);
    failure = error;
  } finally {
    uploading.value = false;
    finishWork(failure);
  }
}
function fileBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",", 2)[1] || "");
    reader.onerror = () => reject(new Error(`Could not read ${file.name}`));
    reader.readAsDataURL(file);
  });
}
async function uploadFiles(files: File[], forComment = false) {
  if (uploading.value || props.task.deleted_at) return;
  uploading.value = true;
  const finishWork = beginEditorWrite();
  const id = props.task.id;
  const failures: string[] = [];
  try {
    for (const file of files) {
      if (file.size > 20 * 1024 * 1024) {
        localError.value = `${file.name} exceeds the 20 MB file limit.`;
        failures.push(localError.value);
        continue;
      }
      if (forComment && !file.type.startsWith("image/")) {
        localError.value = "Choose an image to attach to a comment.";
        failures.push(localError.value);
        continue;
      }
      const attachment = await mutate<Attachment>("attachments.add", {
        id,
        name: file.name,
        mime_type: file.type || "application/octet-stream",
        content_base64: await fileBase64(file),
      });
      if (forComment) {
        if (props.task.id === id) commentAttachments.value.push(attachment.id);
        else commentsDrafts.get(id)?.attachments.push(attachment.id);
      }
    }
  } catch (error) {
    localError.value = String(error);
    failures.push(localError.value);
  } finally {
    uploading.value = false;
    if (fileInput.value) fileInput.value.value = "";
    if (imageInput.value) imageInput.value.value = "";
    finishWork(failures.length ? new Error(failures.join("\n")) : undefined);
  }
}
async function downloadAttachment(attachment: Attachment, open = false) {
  const finishWork = beginEditorWrite();
  let failure: unknown;
  try {
    if (isNative) {
      await invoke(open ? "open_attachment" : "save_attachment", {
        id: attachment.id,
      });
      return;
    }
    const value = await dispatch<Attachment & { content_base64: string }>(
      "attachments.get",
      { id: attachment.id },
    );
    const bytes = Uint8Array.from(atob(value.content_base64), (char) =>
      char.charCodeAt(0),
    );
    const url = URL.createObjectURL(
      new Blob([bytes], { type: value.mime_type }),
    );
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = value.name;
    anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  } catch (error) {
    localError.value = String(error);
    failure = error;
  } finally {
    finishWork(failure);
  }
}
async function deleteAttachment(attachment: Attachment) {
  try {
    await mutate("attachments.remove", { id: attachment.id });
  } catch (error) {
    localError.value = String(error);
  }
}
function fileSize(bytes: number) {
  return bytes >= 1048576
    ? `${(bytes / 1048576).toFixed(1)} MB`
    : bytes >= 1024
      ? `${Math.round(bytes / 1024)} KB`
      : `${bytes} B`;
}
async function openURL(url: string) {
  try {
    if (isNative) await invoke("open_external_link", { url });
    else window.open(url, "_blank", "noopener,noreferrer");
  } catch (error) {
    localError.value = String(error);
  }
}
async function dropSubtask(event: DragEvent, target: Task) {
  try {
    const payload = JSON.parse(
      event.dataTransfer?.getData("application/x-tasks") || "{}",
    );
    if (
      payload.kind !== "task" ||
      payload.id === target.id ||
      !children.value.some((task) => task.id === payload.id)
    )
      return;
    const ids = children.value
      .map((task) => task.id)
      .filter((id) => id !== payload.id);
    ids.splice(ids.indexOf(target.id), 0, payload.id);
    await mutate("tasks.reorder", { ids });
  } catch (error) {
    localError.value = String(error);
  }
}
function dragChild(event: DragEvent, task: Task) {
  event.dataTransfer?.setData(
    "application/x-tasks",
    JSON.stringify({ kind: "task", id: task.id }),
  );
}
function toggleChecklist(id: string) {
  editTask(props.task.id, {
    checklist: props.task.checklist.map((item) =>
      item.id === id ? { ...item, completed: !item.completed } : item,
    ),
  });
}
function editChecklist(id: string, text: string) {
  editTask(props.task.id, {
    checklist: props.task.checklist.map((item) =>
      item.id === id ? { ...item, text } : item,
    ),
  });
}
function removeChecklist(id: string) {
  editTask(props.task.id, {
    checklist: props.task.checklist.filter((item) => item.id !== id),
  });
}
async function sendComment() {
  if (
    (!commentBody.value.trim() && !commentAttachments.value.length) ||
    adding.value
  )
    return;
  adding.value = true;
  const taskId = props.task.id;
  const draftBody = commentBody.value;
  const commentId = editingComment.value;
  const attachmentIds = [...commentAttachments.value];
  try {
    await mutate(commentId ? "comments.update" : "comments.add", {
      id: commentId || taskId,
      body: draftBody.trim(),
      attachment_ids: attachmentIds,
      ...(commentId && editingCommentVersion.value
        ? { expected_version: editingCommentVersion.value }
        : {}),
    });
    if (
      props.task.id === taskId &&
      commentBody.value === draftBody &&
      editingComment.value === commentId
    ) {
      commentBody.value = "";
      commentAttachments.value = [];
      editingComment.value = null;
      editingCommentVersion.value = null;
    }
    if (commentsDrafts.get(taskId)?.body === draftBody)
      commentsDrafts.delete(taskId);
  } catch (error) {
    localError.value = String(error);
  } finally {
    adding.value = false;
  }
}
function editComment(comment: LocalComment) {
  editingComment.value = comment.id;
  editingCommentVersion.value = comment.version;
  commentBody.value = comment.body;
  commentAttachments.value = [...(comment.attachment_ids || [])];
  nextTick(() => commentInput.value?.focus());
}
async function deleteComment(comment: LocalComment) {
  try {
    await mutate("comments.delete", {
      id: comment.id,
      expected_version: comment.version,
    });
  } catch (error) {
    localError.value = String(error);
  }
}
function commentFiles(comment: LocalComment) {
  return (
    props.task.attachments?.filter((file) =>
      comment.attachment_ids?.includes(file.id),
    ) || []
  );
}
async function printTask() {
  menu.value = null;
  localError.value = "";
  try {
    await nextTick();
    if (isNative) {
      // Tauri's macOS print command opens the system sheet for this webview.
      // Await IPC so permission/runtime failures appear in the task editor.
      await invoke("plugin:webview|print");
    } else {
      window.print();
    }
  } catch (error) {
    localError.value = `Could not open Print: ${String(error)}`;
  }
}
</script>

<template>
  <section
    ref="editorRoot"
    class="task-editor"
    :class="{
      'deleted-editor': task.deleted_at,
      'editor-focus-mode': focusMode,
    }"
    aria-label="Task details"
  >
    <div class="editor-toolbar" data-tauri-drag-region="deep">
      <button
        class="icon-button detail-back"
        aria-label="Back to tasks"
        @click="emit('close')"
      >
        <PhArrowLeft :size="19" />
      </button>
      <button
        v-if="!task.is_note"
        class="task-checkbox"
        :class="[
          `priority-${task.priority}`,
          { checked: task.completed_at, dismissed: task.wont_do_at },
        ]"
        :aria-label="task.completed_at ? 'Reopen task' : 'Complete task'"
        :disabled="!!task.deleted_at"
        @click="complete(task)"
      >
        <PhCheck v-if="task.completed_at" :size="12" weight="bold" /><PhX
          v-else-if="task.wont_do_at"
          :size="12"
        />
      </button>
      <PhNote v-else class="note-type-icon" :size="17" aria-label="Note" />
      <span class="toolbar-separator"></span>
      <div class="popover-anchor date-anchor" data-tauri-drag-region="false">
        <button
          class="date-button"
          :class="{ 'has-date': task.due_date, overdue: overdueDays > 0 }"
          aria-label="Set due date"
          :aria-description="dueText"
          :title="dueText"
          :disabled="!!task.deleted_at"
          @click="openDate"
        >
          <IconCalendarWeek :size="19" :stroke-width="1.5" /><span>{{
            dueText
          }}</span
          ><PhBell v-if="task.reminder_at" :size="13" /><PhRepeat
            v-if="task.recurrence"
            :size="14"
          />
        </button>
        <TaskDatePicker
          v-if="menu === 'date'"
          :task="task"
          :settings="state.snapshot.settings"
          @save="update"
          @close="menu = null"
        />
      </div>
      <PhPushPin
        v-if="task.pinned"
        class="pinned-marker"
        :size="15"
        weight="fill"
      />
      <button
        v-if="focusMode"
        class="icon-button"
        aria-label="Exit focus mode"
        @click="focusMode = false"
      >
        <PhArrowsInSimple :size="18" />
      </button>
      <div
        class="popover-anchor priority-anchor"
        data-tauri-drag-region="false"
      >
        <button
          class="icon-button"
          :class="`flag-priority-${task.priority}`"
          aria-label="Set priority"
          :disabled="!!task.deleted_at"
          @click="menu = menu === 'priority' ? null : 'priority'"
        >
          <component
            :is="task.priority ? IconFlagFilled : IconFlag"
            :size="20"
            :stroke-width="1.5"
          />
        </button>
        <div
          v-if="menu === 'priority'"
          class="popover menu-popover align-right"
          role="menu"
          aria-label="Priority"
        >
          <button
            v-for="priority in [3, 2, 1, 0]"
            :key="priority"
            role="menuitem"
            @click="update({ priority })"
          >
            <PhFlag
              :class="`flag-priority-${priority}`"
              :size="17"
              :weight="priority ? 'fill' : 'regular'"
            />{{
              [
                "No Priority",
                "Low Priority",
                "Medium Priority",
                "High Priority",
              ][priority]
            }}<PhCheck
              v-if="task.priority === priority"
              class="menu-check"
              :size="14"
            />
          </button>
        </div>
      </div>
    </div>
    <TaskProgress v-if="!task.is_note && !task.deleted_at" :task="task" />
    <TaskHistory
      v-if="historyOpen"
      :task-id="task.id"
      @close="historyOpen = false"
      @restored="refresh"
    />
    <div class="editor-scroll" @click.self="menu = null">
      <button v-if="parent" class="parent-task" @click="openTask(parent.id)">
        <PhArrowLeft :size="13" />{{ parent.title }}
      </button>
      <div v-if="task.deleted_at" class="trash-notice">
        This task is in Trash.<button @click="restore">
          <PhArrowCounterClockwise :size="14" />Restore
        </button>
      </div>
      <div v-else-if="task.wont_do_at" class="trash-notice">
        This task is marked Won't Do.<button @click="dismissTask">
          Reopen
        </button>
      </div>
      <div class="editor-title-line">
        <textarea
          ref="titleInput"
          class="editor-title"
          rows="1"
          aria-label="Task title"
          placeholder="Task title"
          :readonly="!!task.deleted_at"
          @focus="titleField.focus"
          @beforeinput="titleField.beforeInput"
          @input="changeTitle"
          @compositionstart="titleField.compositionStart"
          @compositionend="titleCompositionEnd"
          @blur="titleField.blur"
          @keydown="titleKeydown"
        ></textarea
        ><button
          class="icon-button"
          aria-label="Add subtask"
          :disabled="!!task.deleted_at"
          @click="focusSubtask"
        >
          <IconListDetails :size="18" :stroke-width="1.5" />
        </button>
      </div>
      <div class="notes-container">
        <RichNotes
          :key="task.id"
          ref="richNotes"
          :task-id="task.id"
          :model-value="task.notes"
          :format="task.notes_format || 'plain'"
          :readonly="!!task.deleted_at"
          :toolbar="toolbar"
          @notes-change="changeNotes"
          @attachment="attachFiles"
          @files="uploadFiles"
          @subtask="focusSubtask"
          @tag="openTags"
          @link-task="
            dialog = 'link';
            linkQuery = '';
          "
          @open-url="openURL"
          @focus-mode="focusMode = !focusMode"
        />
      </div>
      <div
        v-if="task.checklist?.length"
        class="standalone-checklist"
        aria-label="Task checklist"
      >
        <div
          v-for="item in task.checklist"
          :key="item.id"
          class="checklist-row"
        >
          <button
            class="task-checkbox"
            :class="{ checked: item.completed }"
            :aria-label="`${item.completed ? 'Uncheck' : 'Check'} ${item.text}`"
            :disabled="!!task.deleted_at"
            @click="toggleChecklist(item.id)"
          >
            <PhCheck v-if="item.completed" :size="12" /></button
          ><input
            :value="item.text"
            :class="{ done: item.completed }"
            :aria-label="`Checklist item ${item.text}`"
            :readonly="!!task.deleted_at"
            @input="
              editChecklist(item.id, ($event.target as HTMLInputElement).value)
            "
          /><button
            class="icon-button"
            :aria-label="`Remove checklist item ${item.text}`"
            :disabled="!!task.deleted_at"
            @click="removeChecklist(item.id)"
          >
            <PhX :size="13" />
          </button>
        </div>
      </div>
      <div
        v-if="task.attachments?.length || uploading"
        class="task-attachments"
        aria-label="Attachments"
      >
        <div
          v-for="attachment in task.attachments"
          :key="attachment.id"
          class="attachment-card"
        >
          <button
            class="attachment-main"
            :aria-label="`Open attachment ${attachment.name}`"
            @click="downloadAttachment(attachment, true)"
          >
            <PhImage
              v-if="attachment.mime_type.startsWith('image/')"
              :size="29"
            /><PhFile v-else :size="29" /><span
              ><strong>{{ attachment.name }}</strong
              ><small>{{ fileSize(attachment.size) }}</small></span
            ></button
          ><button
            class="icon-button"
            :aria-label="`Save attachment ${attachment.name}`"
            @click="downloadAttachment(attachment)"
          >
            <PhDownloadSimple :size="15" /></button
          ><button
            v-if="!task.deleted_at"
            class="icon-button"
            :aria-label="`Remove attachment ${attachment.name}`"
            @click="deleteAttachment(attachment)"
          >
            <PhX :size="14" />
          </button>
        </div>
        <p v-if="uploading" class="attachment-progress" role="status">
          Adding attachment…
        </p>
      </div>
      <div
        v-if="linkedTasks.length"
        class="linked-tasks"
        aria-label="Linked tasks"
      >
        <div v-for="linked in linkedTasks" :key="linked.id">
          <button @click="openTask(linked.id)">
            <PhLink :size="16" /><span>{{ linked.title }}</span
            ><PhArrowSquareOut :size="13" /></button
          ><button
            v-if="!task.deleted_at"
            class="icon-button"
            :aria-label="`Unlink ${linked.title}`"
            @click="unlinkTask(linked)"
          >
            <PhX :size="13" />
          </button>
        </div>
      </div>
      <div v-if="children.length || showSubtasks" class="subtasks">
        <div
          v-for="child in activeChildren"
          :key="child.id"
          class="subtask-row"
          draggable="true"
          @dragstart="dragChild($event, child)"
          @dragover.prevent
          @drop.prevent="dropSubtask($event, child)"
        >
          <button
            v-if="!child.is_note"
            class="task-checkbox"
            :aria-label="`Complete ${child.title}`"
            @click="complete(child)"
          >
            <PhCheck v-if="child.completed_at" :size="12" /></button
          ><PhNote v-else class="note-type-icon" :size="16" aria-label="Note" />
          <button class="subtask-title" @click="openTask(child.id)">
            {{ child.title }}</button
          ><span class="due-label">{{ dateLabel(child.due_date) }}</span
          ><button
            class="icon-button subtask-remove"
            :aria-label="`Delete ${child.title}`"
            @click="mutate('tasks.delete', { id: child.id }).catch(() => {})"
          >
            <PhTrash :size="14" />
          </button>
        </div>
        <button
          v-if="completedChildren.length"
          class="completed-subtasks-toggle"
          :aria-expanded="showCompletedSubtasks"
          @click="showCompletedSubtasks = !showCompletedSubtasks"
        >
          <PhCaretDown
            :size="12"
            :class="{ collapsed: !showCompletedSubtasks }"
          />{{ completedChildren.length }} completed
          {{ completedChildren.length === 1 ? "task" : "tasks" }}
        </button>
        <template v-if="showCompletedSubtasks"
          ><div
            v-for="child in completedChildren"
            :key="child.id"
            class="subtask-row"
          >
            <button
              v-if="!child.is_note"
              class="task-checkbox checked"
              :aria-label="`Reopen ${child.title}`"
              @click="complete(child)"
            >
              <PhCheck :size="12" /></button
            ><PhNote
              v-else
              class="note-type-icon"
              :size="16"
              aria-label="Note"
            />
            <button class="subtask-title done" @click="openTask(child.id)">
              {{ child.title }}
            </button>
          </div></template
        >
        <form
          v-if="!task.deleted_at"
          class="add-subtask"
          @submit.prevent="addSubtask"
        >
          <PhPlus :size="16" /><input
            ref="subtaskInput"
            v-model="subtaskTitle"
            placeholder="Add Subtask"
            aria-label="Add subtask"
            :disabled="adding"
          /><button
            v-if="subtaskTitle"
            class="text-button"
            type="submit"
            :disabled="adding"
          >
            Add
          </button>
        </form>
      </div>
      <div v-if="taskTags.length" class="task-tag-chips">
        <button
          v-for="tag in taskTags"
          :key="tag.id"
          :style="{ color: tag.color }"
          @click="openTags"
        >
          <PhTag :size="13" />{{ tag.name }}</button
        ><button
          v-if="!task.deleted_at"
          class="add-tag-chip"
          aria-label="Add tags"
          @click="openTags"
        >
          <PhPlus :size="13" />
        </button>
      </div>
      <p v-if="localError" class="editor-error" role="alert">
        {{ localError
        }}<button aria-label="Dismiss error" @click="localError = ''">
          <PhX :size="13" />
        </button>
      </p>
    </div>
    <section
      v-if="commentsOpen"
      class="comments-pane"
      aria-label="Task comments"
    >
      <div v-if="comments.length" class="comment-list">
        <article
          v-for="comment in comments"
          :key="comment.id"
          class="comment-item"
        >
          <div class="comment-avatar">Y</div>
          <div class="comment-content">
            <header>
              <strong>{{ comment.author || "You" }}</strong
              ><time>{{
                new Date(comment.created_at).toLocaleString(undefined, {
                  month: "short",
                  day: "numeric",
                  hour: "2-digit",
                  minute: "2-digit",
                })
              }}</time
              ><button
                :aria-label="`Edit comment ${comment.body}`"
                @click="editComment(comment)"
              >
                Edit</button
              ><button
                :aria-label="`Delete comment ${comment.body}`"
                @click="deleteComment(comment)"
              >
                Delete
              </button>
            </header>
            <p>{{ comment.body }}</p>
            <button
              v-for="file in commentFiles(comment)"
              :key="file.id"
              class="comment-file"
              @click="downloadAttachment(file, true)"
            >
              <PhImage :size="17" />{{ file.name }}
            </button>
          </div>
        </article>
      </div>
      <form
        v-if="!task.deleted_at"
        class="comment-form"
        @submit.prevent="sendComment"
      >
        <p v-if="editingComment" class="editing-comment">
          Editing comment<button
            type="button"
            @click="
              editingComment = null;
              editingCommentVersion = null;
              commentBody = '';
              commentAttachments = [];
            "
          >
            Cancel
          </button>
        </p>
        <div class="comment-compose">
          <textarea
            ref="commentInput"
            v-model="commentBody"
            rows="1"
            aria-label="Write a comment"
            placeholder="Write a comment"
            @keydown.meta.enter.prevent="sendComment"
            @keydown.ctrl.enter.prevent="sendComment"
          ></textarea>
          <div class="popover-anchor">
            <button
              type="button"
              class="icon-button"
              aria-label="Emoji"
              @click="emojiOpen = !emojiOpen"
            >
              <PhSmiley :size="17" />
            </button>
            <div v-if="emojiOpen" class="emoji-picker">
              <button
                v-for="emoji in [
                  '🙂',
                  '👍',
                  '✅',
                  '🎉',
                  '❤️',
                  '👀',
                  '🙏',
                  '💡',
                  '⭐',
                  '📌',
                  '🚀',
                  '💪',
                ]"
                :key="emoji"
                type="button"
                @click="
                  commentBody += emoji;
                  emojiOpen = false;
                  commentInput?.focus();
                "
              >
                {{ emoji }}
              </button>
            </div>
          </div>
          <button
            type="button"
            class="icon-button"
            aria-label="Attach comment image"
            @click="imageInput?.click()"
          >
            <PhImage :size="18" /></button
          ><button
            v-if="commentBody.trim() || commentAttachments.length"
            type="submit"
            class="comment-send"
            :disabled="adding"
          >
            {{ editingComment ? "Save" : "Send" }}
          </button>
        </div>
        <div v-if="commentAttachments.length" class="comment-pending-files">
          <span v-for="id in commentAttachments" :key="id"
            >{{
              task.attachments?.find((file) => file.id === id)?.name || "Image"
            }}<button
              type="button"
              :aria-label="`Remove comment image ${id}`"
              @click="
                commentAttachments = commentAttachments.filter(
                  (value) => value !== id,
                )
              "
            >
              <PhX :size="12" /></button
          ></span>
        </div>
      </form>
    </section>
    <footer class="editor-footer">
      <div class="popover-anchor footer-list">
        <button
          class="footer-list-button"
          aria-label="Move to list"
          :disabled="!!task.deleted_at"
          @click="menu = menu === 'move' ? null : 'move'"
        >
          <TaskNavigationGlyph
            v-if="task.list_id === 'inbox'"
            kind="inbox"
            :size="18"
          /><span
            v-else-if="currentList?.icon"
            class="footer-list-emoji"
            aria-hidden="true"
            >{{ currentList.icon }}</span
          ><PhList v-else :size="18" /><span>{{
            currentList?.name || "Inbox"
          }}</span>
        </button>
        <div
          v-if="menu === 'move'"
          class="popover menu-popover above move-menu"
          role="menu"
          aria-label="Move to list"
        >
          <button
            v-for="list in state.snapshot.lists.filter(
              (value) => !value.archived_at,
            )"
            :key="list.id"
            role="menuitem"
            @click="update({ list_id: list.id })"
          >
            <PhList :size="16" :style="{ color: list.color }" />{{ list.name
            }}<PhCheck
              v-if="task.list_id === list.id"
              class="menu-check"
              :size="14"
            /></button
          ><template v-if="sections.length"
            ><div class="menu-divider"></div>
            <span class="menu-label">Section</span
            ><button @click="update({ section_id: null })">
              No section<PhCheck
                v-if="!task.section_id"
                class="menu-check"
                :size="14"
              /></button
            ><button
              v-for="section in sections"
              :key="section.id"
              @click="update({ section_id: section.id })"
            >
              {{ section.name
              }}<PhCheck
                v-if="task.section_id === section.id"
                class="menu-check"
                :size="14"
              /></button
          ></template>
        </div>
      </div>
      <span class="save-state" aria-live="polite">{{
        state.drafts[task.id]?.failed
          ? "Not saved"
          : state.drafts[task.id] || state.busy
            ? "Saving…"
            : notice
      }}</span>
      <div class="popover-anchor">
        <button
          class="icon-button tags-control"
          aria-label="Task tags"
          :disabled="!!task.deleted_at"
          @click="menu = menu === 'tags' ? null : 'tags'"
        >
          <PhTag :size="17" />
        </button>
        <div
          v-if="menu === 'tags'"
          class="popover above align-right tag-picker"
          role="dialog"
          aria-label="Task tags"
        >
          <input
            ref="tagInput"
            v-model="tagQuery"
            aria-label="Search tags"
            placeholder="Search tags"
            @keydown.enter.prevent="createTag"
          />
          <div class="tag-options">
            <button
              v-for="tag in filteredTags"
              :key="tag.id"
              :aria-pressed="task.tag_ids?.includes(tag.id) || false"
              @click="toggleTag(tag)"
            >
              <PhTag :size="16" :style="{ color: tag.color }" />{{ tag.name
              }}<PhCheck
                v-if="task.tag_ids?.includes(tag.id)"
                class="menu-check"
                :size="14"
              />
            </button>
            <p v-if="!filteredTags.length && !tagQuery">No tags yet</p>
          </div>
          <button
            v-if="
              tagQuery.trim() &&
              !state.snapshot.tags.some(
                (tag) =>
                  tag.name.toLocaleLowerCase() ===
                  tagQuery.trim().replace(/^#/, '').toLocaleLowerCase(),
              )
            "
            class="create-tag"
            @click="createTag"
          >
            <PhPlus :size="16" />Create “{{ tagQuery.trim() }}”
          </button>
        </div>
      </div>
      <button
        class="icon-button"
        aria-label="Text formatting"
        :aria-pressed="toolbar"
        :disabled="!!task.deleted_at"
        @click="
          toolbar = !toolbar;
          menu = null;
        "
      >
        <IconTypography :size="20" :stroke-width="1.4" />
      </button>
      <button
        class="icon-button"
        aria-label="Comments"
        :aria-pressed="commentsOpen"
        @click="
          commentsOpen = !commentsOpen;
          toolbar = false;
        "
      >
        <IconMessage2 :size="20" :stroke-width="1.5" /><sup
          v-if="comments.length"
          >{{ comments.length }}</sup
        >
      </button>
      <div class="popover-anchor">
        <button
          class="icon-button"
          aria-label="Task actions"
          @click="menu = menu === 'more' ? null : 'more'"
        >
          <PhDotsThree :size="23" weight="bold" />
        </button>
        <div
          v-if="menu === 'more'"
          class="popover menu-popover align-right above task-action-menu"
          role="menu"
          aria-label="Task actions"
        >
          <template v-if="!task.deleted_at">
            <button role="menuitem" @click="focusSubtask">
              <PhTreeStructure :size="17" />Add Subtask
            </button>
            <button
              role="menuitem"
              @click="
                menu = null;
                dialog = 'parent';
              "
            >
              <PhTreeStructure :size="17" />Link Parent Task
            </button>
            <button role="menuitem" @click="update({ pinned: !task.pinned })">
              <PhPushPin :size="17" />{{ task.pinned ? "Unpin" : "Pin" }}
            </button>
            <button v-if="!task.is_note" role="menuitem" @click="dismissTask">
              <PhXSquare :size="17" />{{
                task.wont_do_at ? "Reopen" : "Won't Do"
              }}
            </button>
            <button role="menuitem" @click="openTags">
              <PhTag :size="17" />Tags
            </button>
            <button role="menuitem" @click="attachFiles">
              <PhPaperclip :size="17" />Upload Attachment
            </button>
            <div class="menu-divider"></div>
            <button
              role="menuitem"
              @click="
                menu = null;
                dialog = 'template';
                templateName = task.title;
              "
            >
              <PhFloppyDisk :size="17" />Save as Template
            </button>
            <button role="menuitem" @click="duplicate">
              <PhCopy :size="17" />Duplicate
            </button>
            <button role="menuitem" @click="copyTaskLink">
              <PhLink :size="17" />Copy Link
            </button>
            <button role="menuitem" @click="update({ is_note: !task.is_note })">
              <PhNote :size="17" />{{
                task.is_note ? "Convert to Task" : "Convert to Note"
              }}
            </button>
            <button
              role="menuitem"
              @click="
                historyOpen = true;
                menu = null;
              "
            >
              <PhArrowCounterClockwise :size="17" />Task Activities
            </button>
            <button role="menuitem" @click="openExport">
              <PhExport :size="17" />Export
            </button>
            <button role="menuitem" @click="printTask">
              <PhPrinter :size="17" />Print
            </button>
            <button role="menuitem" @click="remove">
              <PhTrash :size="17" />Delete
            </button>
          </template>
          <button v-else role="menuitem" @click="restore">
            <PhArrowCounterClockwise :size="17" />Restore task
          </button>
        </div>
      </div>
    </footer>
    <input
      ref="fileInput"
      class="hidden-file-input"
      type="file"
      multiple
      aria-label="Attach files"
      @change="
        uploadFiles(Array.from(($event.target as HTMLInputElement).files || []))
      "
    />
    <input
      ref="imageInput"
      class="hidden-file-input"
      type="file"
      multiple
      accept="image/*"
      aria-label="Attach comment images"
      @change="
        uploadFiles(
          Array.from(($event.target as HTMLInputElement).files || []),
          true,
        )
      "
    />
    <TaskExportDialog
      v-if="dialog === 'export'"
      :task="task"
      :tasks="tasks"
      @close="dialog = null"
    />
    <ParentTaskPicker
      v-if="dialog === 'parent'"
      :task="task"
      :tasks="tasks"
      :lists="state.snapshot.lists"
      @select="setParent"
      @close="dialog = null"
    />
    <AppDialog v-if="dialog === 'link'" title="Link Task" @close="dialog = null"
      ><div class="link-task-dialog">
        <input
          v-model="linkQuery"
          placeholder="Search tasks"
          aria-label="Search tasks to link"
          autofocus
        />
        <div>
          <button
            v-for="candidate in linkCandidates"
            :key="candidate.id"
            @click="linkTask(candidate)"
          >
            <PhLink :size="17" /><span>{{ candidate.title }}</span
            ><small>{{
              state.snapshot.lists.find((list) => list.id === candidate.list_id)
                ?.name
            }}</small>
          </button>
          <p v-if="!linkCandidates.length">No matching tasks</p>
        </div>
      </div></AppDialog
    >
    <AppDialog
      v-if="dialog === 'template'"
      title="Save as Template"
      @close="dialog = null"
      ><form class="dialog-form" @submit.prevent="saveTemplate">
        <label
          >Name<input
            v-model="templateName"
            aria-label="Template name"
            autofocus
        /></label>
        <footer>
          <button class="text-button" type="button" @click="dialog = null">
            Cancel</button
          ><button
            class="primary-button"
            type="submit"
            :disabled="!templateName.trim()"
          >
            Save
          </button>
        </footer>
      </form></AppDialog
    >
  </section>
</template>

<style scoped>
.footer-list-emoji {
  flex: 0 0 18px;
  width: 18px;
  font-size: 15px;
  line-height: 20px;
}
.editor-title {
  resize: none;
  overflow: hidden;
  display: block;
  min-height: 28px;
}
.editor-title-line {
  align-items: flex-start;
}
.editor-title-line > .icon-button {
  margin-top: 0;
}
.date-anchor {
  min-width: 0;
}
.date-anchor > :deep(.task-date-picker) {
  position: absolute;
  top: calc(100% + 7px);
  left: 0;
  z-index: 70;
}
.date-button {
  max-width: 100%;
}
.date-button.overdue {
  color: var(--danger);
}
.date-button > span {
  overflow: hidden;
  text-overflow: ellipsis;
}
.pinned-marker {
  color: var(--muted);
  margin-left: auto;
}
.pinned-marker + .priority-anchor {
  margin-left: 0;
}
.task-checkbox.dismissed {
  border-color: var(--muted);
  color: var(--muted);
}
.editor-focus-mode {
  position: fixed;
  inset: 6px 6px 6px 62px;
  z-index: 80;
  border-radius: 14px;
  overflow: hidden;
  box-shadow: 0 0 0 100vmax #0004;
}
.editor-focus-mode :deep(.rich-toolbar) {
  left: calc(50% + 31px);
}
.completed-subtasks-toggle {
  display: flex;
  gap: 6px;
  align-items: center;
  color: var(--muted);
  font-size: 11px;
  padding: 10px 0 2px;
}
.completed-subtasks-toggle .collapsed {
  transform: rotate(-90deg);
}
.task-tag-chips {
  display: flex;
  gap: 7px;
  flex-wrap: wrap;
  padding-top: 18px;
}
.task-tag-chips button {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 7px;
  border: 1px solid var(--border);
  border-radius: 5px;
  font-size: 11px;
}
.task-tag-chips .add-tag-chip {
  border: 0;
  color: var(--muted);
}
.tags-control {
  opacity: 0;
}
.editor-footer:hover .tags-control,
.tags-control:focus-visible {
  opacity: 1;
}
.editor-footer {
  position: relative;
}
.editor-footer .icon-button sup {
  position: absolute;
  top: 1px;
  right: -1px;
  font-size: 8px;
  background: var(--surface);
  padding: 1px 2px;
}
.editor-footer .icon-button {
  position: relative;
}
.footer-list {
  max-width: 45%;
  min-width: 0;
}
.footer-list-button {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  font-size: 13px;
  max-width: 100%;
}
.footer-list-button span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.footer-list-button svg {
  flex-shrink: 0;
}
.move-menu {
  max-height: 400px;
  overflow-y: auto;
  width: 210px;
}
.menu-label {
  color: var(--muted);
  font-size: 11px;
  display: block;
  padding: 3px 8px;
}
.task-action-menu {
  width: 198px;
  padding: 6px 4px;
  border-radius: 13px;
  max-height: min(510px, 75vh);
  overflow-y: auto;
}
.task-action-menu button {
  min-height: 34px;
  padding: 7px 11px;
  gap: 10px;
}
.note-type-icon {
  color: var(--muted);
  flex-shrink: 0;
}
.tag-picker {
  width: 220px;
  padding: 7px;
}
.tag-picker > input {
  padding: 7px 8px;
  width: 100%;
  border: 0;
  border-bottom: 1px solid var(--border);
  background: none;
  margin-bottom: 4px;
  font-size: 12px;
}
.tag-picker button {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px;
  width: 100%;
  font-size: 13px;
  text-align: left;
  border-radius: 5px;
}
.tag-picker button:hover {
  background: var(--input);
}
.tag-options {
  max-height: 210px;
  overflow-y: auto;
}
.tag-options p {
  font-size: 12px;
  color: var(--muted);
  padding: 7px;
}
.create-tag {
  color: var(--accent);
}
.task-attachments {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 8px;
  margin-top: 12px;
}
.attachment-card {
  display: flex;
  align-items: center;
  padding: 8px;
  border: 1px solid var(--border);
  border-radius: 7px;
  gap: 3px;
  min-width: 0;
}
.attachment-main {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  text-align: left;
}
.attachment-main > svg {
  color: var(--muted);
  flex-shrink: 0;
}
.attachment-main > span {
  min-width: 0;
}
.attachment-main strong {
  font-weight: 400;
  font-size: 12px;
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.attachment-main small {
  font-size: 10px;
  color: var(--muted);
}
.attachment-card > .icon-button {
  width: 22px;
  flex-shrink: 0;
}
.attachment-progress {
  font-size: 12px;
  color: var(--muted);
}
.linked-tasks {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 14px 0;
}
.linked-tasks > div {
  display: flex;
  gap: 5px;
  align-items: center;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 7px;
}
.linked-tasks > div > button:first-child {
  display: flex;
  align-items: center;
  gap: 7px;
  flex: 1;
  font-size: 12px;
  text-align: left;
}
.linked-tasks span {
  flex: 1;
}
.checklist-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin: 7px 0;
}
.checklist-row > .task-checkbox {
  margin-top: 4px;
}
.checklist-row input {
  flex: 1;
  border: 0;
  background: transparent;
  padding: 0;
  min-width: 0;
  font-size: 14px;
  line-height: 1.6;
}
.checklist-row input.done {
  color: var(--muted);
  text-decoration: line-through;
}
.checklist-row > .icon-button {
  opacity: 0;
  height: 23px;
  width: 23px;
}
.checklist-row:hover > .icon-button,
.checklist-row > .icon-button:focus-visible {
  opacity: 1;
}
.editor-error {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  color: var(--danger);
  background: var(--input);
  border-radius: 6px;
  padding: 10px;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-line;
}
.editor-error > button {
  margin-left: auto;
}
.hidden-file-input {
  display: none;
}
.comments-pane {
  border-top: 1px solid var(--border);
  flex-shrink: 0;
  max-height: 42%;
  overflow: auto;
}
.comment-list {
  padding: 12px 23px 0;
}
.comment-item {
  display: flex;
  gap: 9px;
  margin-bottom: 14px;
}
.comment-avatar {
  width: 25px;
  height: 25px;
  border-radius: 50%;
  background: var(--input);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--secondary);
  font-size: 10px;
  flex-shrink: 0;
}
.comment-content {
  min-width: 0;
  flex: 1;
}
.comment-content header {
  display: flex;
  align-items: center;
  gap: 7px;
  flex-wrap: wrap;
}
.comment-content strong {
  font-size: 11px;
  font-weight: 600;
}
.comment-content time {
  font-size: 10px;
  color: var(--muted);
  margin-right: auto;
}
.comment-content header button {
  font-size: 10px;
  color: var(--muted);
  opacity: 0;
}
.comment-item:hover header button,
.comment-content header button:focus-visible {
  opacity: 1;
}
.comment-content p {
  margin: 4px 0;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.comment-form {
  padding: 11px 23px;
}
.comment-compose {
  display: flex;
  gap: 4px;
  align-items: center;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 5px 7px;
  min-height: 36px;
}
.comment-compose:focus-within {
  border-color: var(--control-accent, #303c70);
}
.comment-compose textarea {
  flex: 1;
  resize: none;
  border: 0;
  background: transparent;
  font-size: 12px;
  padding: 4px;
  min-height: 24px;
  max-height: 110px;
  line-height: 1.4;
}
.comment-compose textarea::placeholder {
  color: var(--muted);
}
.comment-compose .icon-button {
  width: 25px;
  height: 25px;
  color: var(--secondary);
}
.comment-send {
  color: var(--accent);
  font-size: 12px;
  padding: 3px 7px;
}
.comment-pending-files {
  display: flex;
  gap: 7px;
  flex-wrap: wrap;
  padding-top: 7px;
}
.comment-pending-files > span {
  display: flex;
  gap: 5px;
  align-items: center;
  font-size: 11px;
  color: var(--secondary);
  background: var(--input);
  border-radius: 4px;
  padding: 4px 6px;
}
.comment-file {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: var(--accent);
  padding: 5px 0;
}
.editing-comment {
  font-size: 11px;
  color: var(--muted);
  margin: 0 0 7px;
  display: flex;
  justify-content: space-between;
}
.emoji-picker {
  position: absolute;
  bottom: 30px;
  right: 0;
  width: 176px;
  display: grid;
  grid-template-columns: repeat(6, 1fr);
  padding: 5px;
  border-radius: 8px;
  background: var(--popover);
  box-shadow: 0 4px 18px #0002;
  z-index: 70;
}
.emoji-picker button {
  width: 27px;
  height: 28px;
  border-radius: 4px;
}
.emoji-picker button:hover {
  background: var(--input);
}
.link-task-dialog > input {
  width: 100%;
  border: 1px solid var(--border);
  background: var(--surface);
  border-radius: 6px;
  padding: 9px;
  font-size: 13px;
}
.link-task-dialog > div {
  max-height: 300px;
  overflow: auto;
  padding-top: 8px;
}
.link-task-dialog button {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 10px 6px;
  font-size: 13px;
  text-align: left;
  border-radius: 5px;
}
.link-task-dialog button:hover {
  background: var(--input);
}
.link-task-dialog button > span {
  flex: 1;
}
.link-task-dialog small {
  font-size: 10px;
  color: var(--muted);
}
.link-task-dialog p {
  color: var(--muted);
  font-size: 13px;
  text-align: center;
  padding: 20px;
}
@media (max-width: 650px) {
  .tags-control {
    opacity: 1;
  }
  .editor-focus-mode {
    inset: 0;
    border-radius: 0;
  }
  .comment-form {
    padding: 10px 16px;
  }
  .editor-footer {
    padding-left: 16px;
    padding-right: 13px;
  }
  .task-attachments {
    grid-template-columns: 1fr;
  }
}
@media print {
  :global(html),
  :global(body),
  :global(#app),
  :global(.app-frame),
  :global(.workspace) {
    display: block !important;
    position: static !important;
    height: auto !important;
    overflow: visible !important;
    background: white !important;
    color: black !important;
    padding: 0 !important;
  }
  :global(.app-rail),
  :global(.sidebar),
  :global(.task-list-panel),
  :global(.modal-backdrop) {
    display: none !important;
  }
  .task-editor {
    position: static;
    width: 100%;
    overflow: visible;
    display: block;
    background: white;
  }
  .editor-toolbar,
  .editor-footer,
  .comments-pane,
  .icon-button,
  .editor-error {
    display: none !important;
  }
  :deep(.rich-toolbar),
  :deep(.slash-menu),
  .add-subtask,
  .subtask-remove {
    display: none !important;
  }
  .editor-scroll {
    overflow: visible;
  }
}
</style>
