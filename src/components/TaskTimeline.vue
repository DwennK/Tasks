<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { PhCaretLeft, PhCaretRight, PhCalendarBlank, PhPlus, PhX } from "@phosphor-icons/vue";
import type { Task, TaskList } from "../types";
import { localDate } from "../composables/useTasks";
import { moveTimelineTask, resizeTimelineTask, timelineAddDays, timelineDayNumber, timelineSpan, type TimelinePatch } from "../lib/task-timeline";

const props = withDefaults(defineProps<{ tasks: Task[]; lists?: TaskList[]; selectedId?: string | null }>(), { lists: () => [], selectedId: null });
const emit = defineEmits<{
  "open-task": [id: string];
  update: [id: string, patch: TimelinePatch];
  create: [input: { title: string; due_date: string }];
}>();
const zoom = ref<"day" | "week" | "month">("week");
const anchor = ref(timelineAddDays(localDate(), -7));
const scroll = ref<HTMLElement>();
const grid = ref<HTMLElement>();
const error = ref("");
const announcement = ref("");
const createDate = ref<string | null>(null);
const createTitle = ref("");
const createInput = ref<HTMLInputElement>();
const unscheduledId = ref<string | null>(null);
const dropDay = ref<string | null>(null);
const keyboardTaskId = ref<string | null>(null);
const dayWidth = computed(() => ({ day: 64, week: 32, month: 12 })[zoom.value]);
const dayCount = 120;
const labelWidth = 190;
const gridWidth = computed(() => dayWidth.value * dayCount);
const firstDay = computed(() => timelineDayNumber(anchor.value));
const days = computed(() => Array.from({ length: dayCount }, (_, index) => {
  const date = timelineAddDays(anchor.value, index);
  const utc = new Date(`${date}T12:00:00Z`);
  return { date, day: utc.getUTCDate(), weekday: utc.toLocaleDateString("en-US", { weekday: "short", timeZone: "UTC" }), weekend: [0, 6].includes(utc.getUTCDay()), monthStart: utc.getUTCDate() === 1, month: utc.toLocaleDateString("en-US", { month: "short", year: "numeric", timeZone: "UTC" }) };
}));
const active = computed(() => props.tasks.filter((task) => !task.deleted_at && !task.wont_do_at));
const scheduled = computed(() => active.value.filter((task) => task.due_date));
const unscheduled = computed(() => active.value.filter((task) => !task.due_date));
const monthGroups = computed(() => {
  const result: { label: string; days: number }[] = [];
  for (const day of days.value) {
    const current = result.at(-1);
    if (current?.label === day.month) current.days++;
    else result.push({ label: day.month, days: 1 });
  }
  return result;
});
const keyboardTask = computed(() => active.value.find((task) => task.id === keyboardTaskId.value));
const pending = ref<{ task: Task; edge: "move" | "start" | "end"; pointer: number; startX: number; scrollLeft: number; delta: number } | null>(null);
function range(task: Task) {
  const base = timelineSpan(task)!;
  const change = pending.value?.task.id === task.id ? pending.value : null;
  if (!change) return base;
  const start = base.start + (change.edge !== "end" ? change.delta : 0);
  const end = base.end + (change.edge !== "start" ? change.delta : 0);
  return { start, end, days: end - start + 1 };
}
function barStyle(task: Task) {
  const span = range(task);
  const left = (span.start - firstDay.value) * dayWidth.value;
  const width = span.days * dayWidth.value;
  const color = props.lists.find((list) => list.id === task.list_id)?.color;
  return { left: `${left + 2}px`, width: `${Math.max(8, width - 4)}px`, ...(color ? { "--task-color": color } : {}) };
}
function onScreen(task: Task) { const span = range(task); return span.end >= firstDay.value && span.start < firstDay.value + dayCount; }
function prettyDate(day: string) { return new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" }); }
function caption(task: Task) {
  return task.due_date ? `${task.start_date ? `${prettyDate(task.start_date)} – ` : ""}${prettyDate(task.due_date)}${task.due_time ? `, ${task.due_time}` : ""}` : "Not scheduled";
}
function focusDay(day: string) {
  anchor.value = timelineAddDays(day, -7);
  if (scroll.value) scroll.value.scrollLeft = 0;
}
function page(direction: number) {
  anchor.value = timelineAddDays(anchor.value, direction * ({ day: 7, week: 28, month: 90 })[zoom.value]);
  if (scroll.value) scroll.value.scrollLeft = 0;
}
function changeZoom(value: typeof zoom.value) {
  const visibleOffset = Math.floor((scroll.value?.scrollLeft || 0) / dayWidth.value);
  const visibleDay = timelineAddDays(anchor.value, visibleOffset);
  zoom.value = value;
  anchor.value = visibleDay;
  if (scroll.value) scroll.value.scrollLeft = 0;
}
function save(task: Task, build: () => TimelinePatch) {
  try {
    error.value = "";
    emit("update", task.id, build());
    announcement.value = `Schedule change requested for ${task.title}.`;
  } catch (e) { error.value = e instanceof Error ? e.message : String(e); }
}
function begin(event: PointerEvent, task: Task, edge: "move" | "start" | "end") {
  if (event.button !== 0 || task.completed_at) return;
  event.stopPropagation();
  keyboardTaskId.value = task.id;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  pending.value = { task, edge, pointer: event.pointerId, startX: event.clientX, scrollLeft: scroll.value?.scrollLeft || 0, delta: 0 };
}
function moving(event: PointerEvent) {
  const value = pending.value;
  if (!value || event.pointerId !== value.pointer) return;
  const delta = Math.round((event.clientX - value.startX + (scroll.value?.scrollLeft || 0) - value.scrollLeft) / dayWidth.value);
  const span = timelineSpan(value.task)!;
  value.delta = value.edge === "start" ? Math.min(delta, span.days - 1) : value.edge === "end" ? Math.max(delta, 1 - span.days) : delta;
  if (delta) event.preventDefault();
  if (scroll.value) {
    const rect = scroll.value.getBoundingClientRect();
    if (event.clientX > rect.right - 28) scroll.value.scrollLeft += 12;
    else if (event.clientX < rect.left + labelWidth + 28) scroll.value.scrollLeft -= 12;
  }
}
function finish(event: PointerEvent) {
  const value = pending.value;
  if (!value || event.pointerId !== value.pointer) return;
  pending.value = null;
  if (!value.delta) return;
  const task = value.task;
  if (value.edge === "move") save(task, () => moveTimelineTask(task, timelineAddDays(task.start_date || task.due_date!, value.delta)));
  else save(task, () => resizeTimelineTask(task, value.edge as "start" | "end", timelineAddDays(value.edge === "start" ? task.start_date || task.due_date! : task.due_date!, value.delta)));
}
function cancelDrag() { pending.value = null; unscheduledId.value = null; dropDay.value = null; }
function escape(event: KeyboardEvent) {
  if (event.key === "Escape" && (pending.value || createDate.value)) {
    event.preventDefault(); event.stopPropagation(); cancelDrag(); createDate.value = null;
  }
}
function startUnscheduled(event: DragEvent, task: Task) {
  unscheduledId.value = task.id;
  event.dataTransfer?.setData("application/x-tasks-timeline", task.id);
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}
function dateAt(clientX: number) {
  const left = grid.value?.getBoundingClientRect().left;
  if (left === undefined) return null;
  const column = Math.floor((clientX - left) / dayWidth.value);
  return column >= 0 && column < dayCount ? timelineAddDays(anchor.value, column) : null;
}
function dragOver(event: DragEvent) {
  if (!unscheduledId.value || !dateAt(event.clientX)) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  dropDay.value = dateAt(event.clientX);
}
function drop(event: DragEvent) {
  const id = unscheduledId.value;
  const day = dateAt(event.clientX);
  cancelDrag();
  if (!id || !day) return;
  event.preventDefault();
  const task = unscheduled.value.find((task) => task.id === id);
  if (task) save(task, () => moveTimelineTask(task, day));
}
async function openCreate(day: string) {
  createDate.value = day; createTitle.value = "";
  await nextTick(); createInput.value?.focus();
}
function create() {
  if (!createDate.value || !createTitle.value.trim()) return;
  emit("create", { title: createTitle.value.trim(), due_date: createDate.value });
  announcement.value = `New task requested on ${prettyDate(createDate.value)}.`;
  createDate.value = null;
}
function nudge(task: Task, days: number, resize = false) {
  if (!task.due_date || task.completed_at) return;
  save(task, () => resize ? resizeTimelineTask(task, "end", timelineAddDays(task.due_date!, days)) : moveTimelineTask(task, timelineAddDays(task.start_date || task.due_date!, days)));
}
function taskKeys(event: KeyboardEvent, task: Task) {
  if (event.altKey && ["ArrowLeft", "ArrowRight"].includes(event.key)) {
    event.preventDefault(); event.stopPropagation();
    nudge(task, event.key === "ArrowLeft" ? -1 : 1, event.shiftKey);
  }
}
onMounted(() => document.addEventListener("keydown", escape, true));
onBeforeUnmount(() => { cancelDrag(); document.removeEventListener("keydown", escape, true); });
</script>

<template>
  <section class="task-timeline" aria-label="Task timeline" @pointermove="moving" @pointerup="finish" @pointercancel="cancelDrag">
    <header class="timeline-toolbar">
      <div class="timeline-paging">
        <button class="icon-button" aria-label="Previous timeline period" @click="page(-1)"><PhCaretLeft :size="17" /></button>
        <button class="timeline-today" @click="focusDay(localDate())">Today</button>
        <button class="icon-button" aria-label="Next timeline period" @click="page(1)"><PhCaretRight :size="17" /></button>
      </div>
      <input class="timeline-jump" type="date" aria-label="Jump to timeline date" :value="anchor" @change="($event.target as HTMLInputElement).value && focusDay(($event.target as HTMLInputElement).value)" />
      <div class="timeline-zoom" role="group" aria-label="Timeline scale">
        <button v-for="scale in (['day', 'week', 'month'] as const)" :key="scale" :aria-pressed="zoom === scale" @click="changeZoom(scale)">{{ scale[0].toUpperCase() + scale.slice(1) }}</button>
      </div>
      <button class="icon-button" aria-label="Add timeline task" @click="openCreate(localDate())"><PhPlus :size="18" /></button>
    </header>
    <p v-if="error" class="timeline-error" role="alert">{{ error }}</p>
    <div class="timeline-workspace">
    <div class="timeline-main">
    <div ref="scroll" class="timeline-scroll" @dragover="dragOver" @drop="drop" @dragleave="dropDay = null">
      <div class="timeline-sheet" :style="{ width: `${labelWidth + gridWidth}px`, '--day-width': `${dayWidth}px`, '--label-width': `${labelWidth}px` }">
        <div class="timeline-label-head">Tasks <span>{{ scheduled.length }}</span></div>
        <div ref="grid" class="timeline-dates" :style="{ width: `${gridWidth}px` }">
          <div class="timeline-months"><span v-for="(month, index) in monthGroups" :key="index" :style="{ width: `${month.days * dayWidth}px` }">{{ month.label }}</span></div>
          <div class="timeline-days"><button v-for="day in days" :key="day.date" :class="{ weekend: day.weekend, today: day.date === localDate(), condensed: zoom === 'month' }" :aria-label="`Add task on ${prettyDate(day.date)}`" :title="prettyDate(day.date)" @click="openCreate(day.date)"><small v-if="zoom === 'day'">{{ day.weekday }}</small><span v-if="zoom !== 'month' || day.day === 1 || day.day % 5 === 0">{{ day.day }}</span></button></div>
        </div>
        <template v-for="task in scheduled" :key="task.id">
          <button class="timeline-task-label" :class="{ completed: task.completed_at, selected: selectedId === task.id }" :title="task.title" @click="emit('open-task', task.id)"><span>{{ task.title }}</span><small>{{ task.due_time }}</small></button>
          <div class="timeline-lane" :style="{ width: `${gridWidth}px` }" @click.self="dateAt($event.clientX) && openCreate(dateAt($event.clientX)!)">
            <div v-for="day in days.filter((value) => value.weekend)" :key="day.date" class="timeline-weekend" :style="{ left: `${(timelineDayNumber(day.date) - firstDay) * dayWidth}px` }" />
            <div v-if="onScreen(task)" class="timeline-bar" :class="{ completed: task.completed_at, moving: pending?.task.id === task.id, selected: selectedId === task.id }" :style="barStyle(task)" :data-task-id="task.id" :title="`${task.title} · ${caption(task)}`">
              <button v-if="!task.completed_at" class="timeline-edge start" :aria-label="`Resize start of ${task.title}`" @pointerdown="begin($event, task, 'start')" @keydown.left.prevent.stop="save(task, () => resizeTimelineTask(task, 'start', timelineAddDays(task.start_date || task.due_date!, -1)))" @keydown.right.prevent.stop="save(task, () => resizeTimelineTask(task, 'start', timelineAddDays(task.start_date || task.due_date!, 1)))" />
              <button class="timeline-bar-body" :aria-label="`Open timeline task ${task.title}`" @pointerdown="begin($event, task, 'move')" @dblclick.stop="emit('open-task', task.id)" @keydown.enter.prevent.stop="emit('open-task', task.id)" @keydown="taskKeys($event, task)" @focus="keyboardTaskId = task.id"><span>{{ task.title }}</span></button>
              <button v-if="!task.completed_at" class="timeline-edge end" :aria-label="`Resize end of ${task.title}`" @pointerdown="begin($event, task, 'end')" @keydown.left.prevent.stop="nudge(task, -1, true)" @keydown.right.prevent.stop="nudge(task, 1, true)" />
            </div>
            <button v-else class="timeline-outside" @click="focusDay(task.start_date || task.due_date!)">{{ caption(task) }} <span>Show</span></button>
          </div>
        </template>
        <div v-if="!scheduled.length" class="timeline-empty">No scheduled tasks. Drag a task below onto a date, or add one.</div>
        <div v-if="dropDay" class="timeline-drop-line" :style="{ left: `${labelWidth + (timelineDayNumber(dropDay) - firstDay) * dayWidth}px` }"><span>{{ prettyDate(dropDay) }}</span></div>
      </div>
    </div>
    <div v-if="keyboardTask?.due_date && !keyboardTask.completed_at" class="timeline-selection-tools" aria-label="Selected timeline task schedule">
      <span>{{ keyboardTask.title }}</span>
      <button @click="nudge(keyboardTask, -1)" aria-label="Move selected task one day earlier">← 1 day</button>
      <button @click="nudge(keyboardTask, 1)" aria-label="Move selected task one day later">1 day →</button>
      <button @click="emit('open-task', keyboardTask.id)">Edit date</button>
      <button class="icon-button" aria-label="Close timeline selection" @click="keyboardTaskId = null"><PhX :size="14" /></button>
    </div>
    <form v-if="createDate" class="timeline-create" @submit.prevent="create">
      <PhCalendarBlank :size="16" /><input v-model="createDate" type="date" required aria-label="New timeline task date" />
      <input ref="createInput" v-model="createTitle" placeholder="Add task" aria-label="New timeline task title" required maxlength="1000" />
      <button class="primary-button" :disabled="!createTitle.trim()">Add</button>
      <button type="button" class="icon-button" aria-label="Cancel timeline task" @click="createDate = null"><PhX :size="16" /></button>
    </form>
    </div>
    <aside class="timeline-unscheduled" aria-label="Arrange tasks">
      <h2>Arrange Tasks <span>{{ unscheduled.length }}</span></h2>
      <p v-if="!unscheduled.length">All tasks have a date.</p>
      <div v-else class="timeline-unscheduled-items">
        <div v-for="task in unscheduled" :key="task.id" class="timeline-unscheduled-task" draggable="true" @dragstart="startUnscheduled($event, task)" @dragend="cancelDrag">
          <button @click="emit('open-task', task.id)">{{ task.title }}</button>
          <button class="icon-button" :aria-label="`Schedule ${task.title} today`" title="Schedule today" @click="save(task, () => moveTimelineTask(task, localDate()))"><PhCalendarBlank :size="16" /></button>
        </div>
      </div>
    </aside>
    </div>
    <span class="timeline-sr" role="status">{{ announcement }}</span>
  </section>
</template>

<style scoped>
.task-timeline { display: flex; flex-direction: column; flex: 1; min-height: 0; min-width: 0; font-size: 13px; container-type: inline-size; container-name: timeline; }
.timeline-workspace { display: flex; flex: 1; min-height: 0; min-width: 0; }
.timeline-main { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; }
.timeline-toolbar { display: flex; align-items: center; gap: 10px; padding: 8px 16px 12px; flex-wrap: wrap; }
.timeline-paging, .timeline-zoom { display: flex; align-items: center; gap: 2px; }
.timeline-today { padding: 4px 8px; border: 1px solid var(--border); border-radius: 5px; }
.timeline-jump { color: var(--secondary); background: transparent; border: 0; min-width: 120px; }
.timeline-zoom { margin-left: auto; padding: 2px; border-radius: 6px; background: var(--input); }
.timeline-zoom button { padding: 5px 10px; border-radius: 4px; color: var(--secondary); }
.timeline-zoom button[aria-pressed="true"] { color: var(--text); background: var(--popover); box-shadow: 0 1px 3px #0001; }
.timeline-error { padding: 6px 16px; margin: 0; color: var(--danger); }
.timeline-scroll { flex: 1; overflow: auto; min-height: 160px; overscroll-behavior: contain; border-block: 1px solid var(--border); }
.timeline-sheet { display: grid; grid-template-columns: var(--label-width) 1fr; position: relative; min-height: 100%; align-content: start; }
.timeline-label-head { position: sticky; left: 0; top: 0; z-index: 5; padding: 23px 16px 0; font-weight: 500; background: var(--surface); border-right: 1px solid var(--border); border-bottom: 1px solid var(--border); }
.timeline-label-head span { margin-left: 5px; font-weight: 400; color: var(--muted); }
.timeline-dates { position: sticky; top: 0; z-index: 4; background: var(--surface); border-bottom: 1px solid var(--border); }
.timeline-months { display: flex; height: 27px; }
.timeline-months span { padding: 7px 7px 0; flex-shrink: 0; white-space: nowrap; overflow: hidden; color: var(--secondary); border-left: 1px solid var(--border); }
.timeline-days { display: flex; height: 36px; }
.timeline-days button { display: flex; flex-direction: column; justify-content: center; align-items: center; flex: 0 0 var(--day-width); border-left: 1px solid var(--border); color: var(--secondary); font-size: 11px; }
.timeline-days small { font-size: 9px; line-height: 12px; color: var(--muted); }
.timeline-days .today { color: var(--accent); font-weight: 700; background: var(--selection); }
.timeline-days .condensed { font-size: 8px; padding: 0; }
.timeline-days .weekend { background: var(--input); }
.timeline-task-label { position: sticky; left: 0; z-index: 2; display: flex; align-items: center; gap: 8px; height: 42px; padding: 0 14px; text-align: left; background: var(--surface); border-right: 1px solid var(--border); border-bottom: 1px solid var(--border); }
.timeline-task-label span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
.timeline-task-label small { color: var(--muted); font-size: 10px; }
.timeline-task-label:hover, .timeline-task-label.selected { background: var(--row-selection); }
.timeline-task-label.completed span, .timeline-bar.completed span { text-decoration: line-through; opacity: .65; }
.timeline-lane { height: 42px; position: relative; overflow: hidden; border-bottom: 1px solid var(--border); background-size: var(--day-width) 100%; background-image: repeating-linear-gradient(90deg, transparent, transparent calc(var(--day-width) - 1px), var(--border) calc(var(--day-width) - 1px), var(--border) var(--day-width)); }
.timeline-weekend { position: absolute; height: 100%; width: var(--day-width); background: var(--input); pointer-events: none; }
.timeline-bar { position: absolute; top: 7px; height: 28px; display: flex; align-items: stretch; color: var(--text); background: var(--selection); border-left: 3px solid var(--task-color, var(--accent)); border-radius: 5px; z-index: 1; }
.timeline-bar.selected { outline: 1px solid var(--accent); }
.timeline-bar.moving { opacity: .85; box-shadow: 0 3px 8px #0002; z-index: 3; }
.timeline-bar-body { flex: 1; min-width: 0; padding: 0 5px; text-align: left; cursor: grab; touch-action: none; }
.timeline-bar-body span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
.timeline-bar-body:active { cursor: grabbing; }
.timeline-edge { width: 7px; padding: 0; cursor: ew-resize; touch-action: none; border-radius: 3px; flex-shrink: 0; }
.timeline-edge:hover, .timeline-edge:focus-visible { background: var(--accent); }
.timeline-outside { height: 100%; color: var(--muted); padding-left: 12px; font-size: 11px; }
.timeline-outside span { color: var(--accent); padding-left: 8px; }
.timeline-empty { grid-column: 1 / -1; width: min(100%, 600px); padding: 40px 20px; color: var(--muted); }
.timeline-drop-line { position: absolute; top: 0; bottom: 0; width: var(--day-width); background: #587bff18; outline: 1px solid var(--accent); pointer-events: none; z-index: 6; }
.timeline-drop-line span { position: absolute; top: 63px; background: var(--accent); color: white; white-space: nowrap; padding: 4px 7px; border-radius: 4px; }
.timeline-unscheduled { padding: 16px 12px; width: 210px; overflow: auto; flex-shrink: 0; border-left: 1px solid var(--border); }
.timeline-unscheduled h2 { font-size: 12px; font-weight: 500; margin: 0 0 8px; }
.timeline-unscheduled h2 span { color: var(--muted); margin-left: 6px; }
.timeline-unscheduled p { font-size: 12px; color: var(--muted); margin: 0; }
.timeline-unscheduled-items { display: flex; flex-direction: column; gap: 6px; }
.timeline-unscheduled-task { display: flex; align-items: center; justify-content: space-between; gap: 5px; max-width: 100%; border: 1px solid var(--border); background: var(--popover); border-radius: 6px; padding: 3px 4px 3px 9px; cursor: grab; }
.timeline-unscheduled-task > button:first-child { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; text-align: left; font-size: 12px; }
.timeline-create, .timeline-selection-tools { display: flex; align-items: center; gap: 8px; padding: 8px 16px; border-bottom: 1px solid var(--border); }
.timeline-create input { background: var(--input); border: 0; border-radius: 4px; padding: 6px; }
.timeline-create input[type="date"] { width: 125px; }
.timeline-create input:not([type="date"]) { flex: 1; min-width: 50px; }
.timeline-create .primary-button { padding: 6px 10px; }
.timeline-selection-tools { font-size: 11px; }
.timeline-selection-tools > span { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.timeline-selection-tools > button:not(.icon-button) { padding: 4px 6px; border: 1px solid var(--border); border-radius: 4px; white-space: nowrap; }
.timeline-sr { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
@container timeline (max-width: 700px) {
  .timeline-workspace { flex-direction: column; }
  .timeline-unscheduled { width: auto; max-height: 160px; border-left: 0; border-top: 1px solid var(--border); }
  .timeline-unscheduled-items { flex-direction: row; flex-wrap: wrap; }
  .timeline-jump { display: none; }
}
@media(max-width: 600px) {
  .timeline-workspace { flex-direction: column; }
  .timeline-unscheduled { width: auto; max-height: 160px; border-left: 0; border-top: 1px solid var(--border); }
  .timeline-unscheduled-items { flex-direction: row; flex-wrap: wrap; }
  .timeline-toolbar { gap: 6px; padding: 8px; }
  .timeline-jump { display: none; }
  .timeline-zoom button { padding: 7px; }
  .timeline-create { flex-wrap: wrap; padding: 8px; }
  .timeline-create input:not([type="date"]) { flex-basis: calc(100% - 50px); }
  .timeline-selection-tools { flex-wrap: wrap; padding: 8px; }
  .timeline-selection-tools > span { flex-basis: 100%; }
}
</style>
