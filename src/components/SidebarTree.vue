<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import {
  PhCaretDown,
  PhCaretRight,
  PhDotsThree,
  PhFolder,
  PhFolderOpen,
  PhList,
  PhNote,
  PhPlus,
} from "@phosphor-icons/vue";
import type { Folder, Task, TaskList } from "../types";

const props = defineProps<{
  folders: Folder[];
  lists: TaskList[];
  tasks: Task[];
  activeList: string | null;
  showCounts?: boolean;
  hideNoteCounts?: boolean;
  showColor?: boolean;
}>();
const emit = defineEmits<{
  select: [id: string];
  remove: [value: { kind: "folder" | "list"; id: string }];
  edit: [value: { kind: "folder" | "list"; id: string }];
  create: [value: { kind: "folder" | "list"; parentId: string | null }];
  mutate: [action: string, input: Record<string, unknown>];
  "reorder-container": [
    value: {
      kind: "folder" | "list";
      id: string;
      parentId: string | null;
      targetId: string;
      zone: "before" | "after";
    },
  ];
}>();

type Row =
  | { kind: "folder"; value: Folder; depth: number }
  | { kind: "list"; value: TaskList; depth: number };
type DragItem = { kind: "task" | "list" | "folder"; id: string };
type DropZone = "before" | "after" | "inside";
const closed = ref(new Set<string>());
const addMenu = ref<string | null>(null);
const dragging = ref<DragItem | null>(null);
const dropTarget = ref<string | null>(null);
const dropZone = ref<DropZone>("inside");
const tree = ref<HTMLElement | null>(null);
const contextElement = ref<HTMLElement | null>(null);
const context = ref<{
  kind: "folder" | "list";
  id: string;
  x: number;
  y: number;
} | null>(null);
let contextTrigger: HTMLElement | null = null;
const contextItem = computed(() =>
  context.value?.kind === "folder"
    ? props.folders.find((item) => item.id === context.value?.id)
    : props.lists.find((item) => item.id === context.value?.id),
);
async function openContext(event: MouseEvent | KeyboardEvent, row: Row) {
  event.preventDefault();
  event.stopPropagation();
  addMenu.value = null;
  contextTrigger = event.currentTarget as HTMLElement;
  const bounds = contextTrigger.getBoundingClientRect();
  const pointer = event instanceof MouseEvent && event.type === "contextmenu";
  const x = pointer ? event.clientX : bounds.right - 16;
  const y = pointer ? event.clientY : bounds.bottom;
  context.value = {
    kind: row.kind,
    id: row.value.id,
    x: Math.min(Math.max(8, x), window.innerWidth - 148),
    y: Math.min(
      Math.max(8, y),
      window.innerHeight - (row.kind === "list" ? 184 : 150),
    ),
  };
  await nextTick();
  if (event instanceof KeyboardEvent)
    contextElement.value
      ?.querySelector<HTMLElement>('[role="menuitem"]')
      ?.focus();
  else contextElement.value?.focus();
}
function closeContext(restore = true) {
  const id = context.value?.id;
  const trigger = contextTrigger;
  context.value = null;
  if (restore)
    void nextTick(() => {
      if (trigger?.isConnected) trigger.focus();
      else if (id)
        [
          ...(tree.value?.querySelectorAll<HTMLElement>(
            "[data-container-id]",
          ) || []),
        ]
          .find((element) => element.dataset.containerId === id)
          ?.focus();
    });
}
function editContext() {
  if (!context.value) return;
  emit("edit", { kind: context.value.kind, id: context.value.id });
  closeContext(false);
}
function removeContext() {
  if (!context.value) return;
  emit("remove", { kind: context.value.kind, id: context.value.id });
  closeContext(false);
}
function archiveContext() {
  const item = contextItem.value;
  if (!context.value || !item || !("archived_at" in item)) return;
  emit("mutate", "lists.archive", { id: item.id, archived: !item.archived_at });
  closeContext();
}
function pinContext() {
  const item = contextItem.value;
  if (!context.value || !item) return;
  emit(
    "mutate",
    context.value.kind === "list" ? "lists.update" : "folders.update",
    { id: item.id, pinned: !item.pinned },
  );
  closeContext();
}
function duplicateContext() {
  if (context.value?.kind !== "list") return;
  emit("mutate", "lists.duplicate", { id: context.value.id });
  closeContext();
}
function ungroupContext() {
  if (context.value?.kind !== "folder") return;
  emit("mutate", "folders.delete", { id: context.value.id });
  closeContext(false);
}
function menuKeys(event: KeyboardEvent) {
  event.stopPropagation();
  const items = [
    ...(contextElement.value?.querySelectorAll<HTMLElement>(
      ":scope > button:not(:disabled)",
    ) || []),
  ];
  if (!items.length) return;
  const index = items.indexOf(document.activeElement as HTMLElement);
  if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
    event.preventDefault();
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : index < 0
            ? event.key === "ArrowDown"
              ? 0
              : items.length - 1
            : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) %
              items.length;
    items[next]?.focus();
  } else if (event.key === "Escape" || event.key === "Tab") {
    if (event.key === "Escape") event.preventDefault();
    closeContext();
  }
}
function outsidePointer(event: PointerEvent) {
  const target = event.target as Node;
  if (
    context.value &&
    !contextElement.value?.contains(target) &&
    !contextTrigger?.contains(target)
  )
    closeContext(false);
  if (addMenu.value && !tree.value?.contains(target)) addMenu.value = null;
}
function closeOnWindowChange() {
  closeContext(false);
  addMenu.value = null;
}
onMounted(() => {
  document.addEventListener("pointerdown", outsidePointer);
  window.addEventListener("resize", closeOnWindowChange);
});
onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", outsidePointer);
  window.removeEventListener("resize", closeOnWindowChange);
});

const ordered = <
  T extends { position: number; name: string; pinned?: boolean },
>(
  values: T[],
) =>
  [...values].sort(
    (a, b) =>
      Number(!!b.pinned) - Number(!!a.pinned) ||
      a.position - b.position ||
      a.name.localeCompare(b.name),
  );
const rows = computed(() => {
  const result: Row[] = [];
  const visited = new Set<string>();
  function visit(parent: string | null, depth: number) {
    const siblings: Row[] = [
      ...ordered(props.folders.filter((item) => item.parent_id === parent)).map(
        (value) => ({ kind: "folder" as const, value, depth }),
      ),
      ...ordered(
        props.lists.filter(
          (item) => item.id !== "inbox" && item.folder_id === parent,
        ),
      ).map((value) => ({ kind: "list" as const, value, depth })),
    ];
    siblings.sort(
      (a, b) => Number(!!b.value.pinned) - Number(!!a.value.pinned),
    );
    for (const row of siblings) {
      if (visited.has(row.value.id)) continue;
      visited.add(row.value.id);
      result.push(row);
      if (row.kind === "folder" && !closed.value.has(row.value.id))
        visit(row.value.id, depth + 1);
    }
  }
  visit(null, 0);
  return result;
});
const listCounts = computed(() => {
  const counts = new Map<string, number>();
  for (const task of props.tasks) {
    if (
      !task.parent_id &&
      !task.completed_at &&
      !task.deleted_at &&
      !task.wont_do_at &&
      !(props.hideNoteCounts && task.is_note)
    )
      counts.set(task.list_id, (counts.get(task.list_id) || 0) + 1);
  }
  return counts;
});
function folderIds(id: string) {
  const ids = new Set([id]);
  let added = true;
  while (added) {
    added = false;
    for (const folder of props.folders) {
      if (
        folder.parent_id &&
        ids.has(folder.parent_id) &&
        !ids.has(folder.id)
      ) {
        ids.add(folder.id);
        added = true;
      }
    }
  }
  return ids;
}
function count(row: Row) {
  if (row.kind === "list") return listCounts.value.get(row.value.id) || 0;
  const ids = folderIds(row.value.id);
  return props.lists.reduce(
    (total, list) =>
      total +
      (list.folder_id && ids.has(list.folder_id)
        ? listCounts.value.get(list.id) || 0
        : 0),
    0,
  );
}
function toggle(id: string, expand?: boolean) {
  const next = new Set(closed.value);
  if (expand ?? next.has(id)) next.delete(id);
  else next.add(id);
  closed.value = next;
  addMenu.value = null;
}
function activate(row: Row) {
  if (row.kind === "folder") toggle(row.value.id);
  else emit("select", row.value.id);
}
function keydown(event: KeyboardEvent, row: Row, index: number) {
  if ((event.target as HTMLElement).closest("button")) return;
  if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
    void openContext(event, row);
  } else if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    activate(row);
  } else if (event.key === "ArrowRight" && row.kind === "folder") {
    event.preventDefault();
    toggle(row.value.id, true);
  } else if (event.key === "ArrowLeft" && row.kind === "folder") {
    event.preventDefault();
    toggle(row.value.id, false);
  } else if (
    event.key === "ArrowDown" ||
    event.key === "ArrowUp" ||
    event.key === "Home" ||
    event.key === "End"
  ) {
    event.preventDefault();
    const targets =
      tree.value?.querySelectorAll<HTMLElement>('[role="treeitem"]');
    if (!targets?.length) return;
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? targets.length - 1
          : Math.max(
              0,
              Math.min(
                targets.length - 1,
                index + (event.key === "ArrowDown" ? 1 : -1),
              ),
            );
    targets[next]?.focus();
  } else if (event.key === "F2") {
    event.preventDefault();
    emit("edit", { kind: row.kind, id: row.value.id });
  } else if (event.key === "Escape") addMenu.value = null;
}
function dragStart(event: DragEvent, row: Row) {
  const item = { kind: row.kind, id: row.value.id };
  dragging.value = item;
  addMenu.value = null;
  closeContext(false);
  event.dataTransfer?.setData("application/x-tasks", JSON.stringify(item));
  event.dataTransfer?.setData("text/plain", row.value.name);
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}
function getDrag(event: DragEvent): DragItem | null {
  if (dragging.value) return dragging.value;
  try {
    const value = JSON.parse(
      event.dataTransfer?.getData("application/x-tasks") || "null",
    );
    if (
      value &&
      ["task", "list", "folder"].includes(value.kind) &&
      typeof value.id === "string"
    )
      return value;
  } catch {
    /* Ignore foreign drags. */
  }
  return null;
}
function validDropZone(
  event: DragEvent,
  row: Row,
  item: DragItem | null,
): DropZone | null {
  // Browsers hide transfer payloads during dragover. Container drags started
  // here remain known; an unknown task drag can still target a list safely.
  if (!item) return row.kind === "list" ? "inside" : null;
  if (item.id === row.value.id) return null;
  if (item.kind === "task") return row.kind === "list" ? "inside" : null;
  const source =
    item.kind === "folder"
      ? props.folders.find((folder) => folder.id === item.id)
      : props.lists.find((list) => list.id === item.id);
  if (!source || item.id === "inbox") return null;
  if (
    item.kind === "folder" &&
    row.kind === "folder" &&
    folderIds(item.id).has(row.value.id)
  )
    return null;
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const ratio = (event.clientY - box.top) / box.height;
  if (row.kind === "folder" && ratio > 0.25 && ratio < 0.75) return "inside";
  // Lists and folders use separate orderings, with pinned siblings first.
  // Do not advertise a relative placement those orderings cannot represent.
  if (item.kind !== row.kind || !!source.pinned !== !!row.value.pinned)
    return null;
  return ratio < 0.5 ? "before" : "after";
}
function dragOver(event: DragEvent, row: Row) {
  if (!event.dataTransfer?.types.includes("application/x-tasks")) return;
  const zone = validDropZone(event, row, getDrag(event));
  if (!zone) {
    dropTarget.value = null;
    event.dataTransfer.dropEffect = "none";
    return;
  }
  event.preventDefault();
  event.dataTransfer.dropEffect = "move";
  dropTarget.value = row.value.id;
  dropZone.value = zone;
}
function dragEnd() {
  dragging.value = null;
  dropTarget.value = null;
}
function dragLeave(event: DragEvent) {
  const destination = event.relatedTarget;
  if (
    !(destination instanceof Node) ||
    !(event.currentTarget as HTMLElement).contains(destination)
  )
    dropTarget.value = null;
}
function drop(event: DragEvent, row: Row) {
  event.preventDefault();
  event.stopPropagation();
  const item = getDrag(event);
  const zone = validDropZone(event, row, item);
  dragEnd();
  if (!item || !zone) return;
  if (item.kind === "task") {
    if (row.kind === "list")
      emit("mutate", "tasks.update", {
        id: item.id,
        list_id: row.value.id,
        parent_id: null,
      });
    return;
  }
  const targetParent =
    row.kind === "folder" && zone === "inside"
      ? row.value.id
      : row.kind === "folder"
        ? row.value.parent_id
        : row.value.folder_id;
  if (zone !== "inside") {
    emit("reorder-container", {
      kind: item.kind,
      id: item.id,
      parentId: targetParent,
      targetId: row.value.id,
      zone,
    });
  } else {
    emit("mutate", item.kind === "folder" ? "folders.update" : "lists.update", {
      id: item.id,
      [item.kind === "folder" ? "parent_id" : "folder_id"]: targetParent,
    });
  }
  if (targetParent) toggle(targetParent, true);
}
function rootOver(event: DragEvent) {
  if (!event.dataTransfer?.types.includes("application/x-tasks")) return;
  const item = getDrag(event);
  if (!item || item.kind === "task" || item.id === "inbox") {
    dropTarget.value = null;
    event.dataTransfer.dropEffect = "none";
    return;
  }
  event.preventDefault();
  event.dataTransfer.dropEffect = "move";
  dropTarget.value = "root";
}
function rootDrop(event: DragEvent) {
  event.preventDefault();
  const item = getDrag(event);
  dragEnd();
  if (item?.kind === "folder")
    emit("mutate", "folders.update", { id: item.id, parent_id: null });
  if (item?.kind === "list" && item.id !== "inbox")
    emit("mutate", "lists.update", { id: item.id, folder_id: null });
}
function create(kind: "folder" | "list", parentId: string | null) {
  addMenu.value = null;
  closeContext(false);
  emit("create", { kind, parentId });
}
</script>

<template>
  <div class="sidebar-tree" ref="tree" @keydown.esc="addMenu = null">
    <div
      class="tree-heading"
      :class="{ 'root-over': dropTarget === 'root' }"
      @dragover="rootOver"
      @drop="rootDrop"
      @dragleave="dragLeave"
    >
      <span>Lists</span>
      <button
        class="tree-tool heading-add"
        aria-label="Create list or folder"
        title="Create list or folder"
        @click="addMenu = addMenu === 'root' ? null : 'root'"
        :aria-expanded="addMenu === 'root'"
      >
        <PhPlus :size="16" />
      </button>
      <div v-if="addMenu === 'root'" class="tree-create-menu" role="menu">
        <button role="menuitem" @click="create('list', null)">
          <PhList :size="17" />New List
        </button>
        <button role="menuitem" @click="create('folder', null)">
          <PhFolder :size="17" />New Folder
        </button>
      </div>
    </div>
    <div role="tree" aria-label="Task lists" class="tree-rows">
      <div
        v-for="(row, index) in rows"
        :key="row.kind + row.value.id"
        class="tree-row"
        :class="{
          selected: row.kind === 'list' && activeList === row.value.id,
          'drop-before': dropTarget === row.value.id && dropZone === 'before',
          'drop-after': dropTarget === row.value.id && dropZone === 'after',
          'drop-inside': dropTarget === row.value.id && dropZone === 'inside',
        }"
        :style="{ paddingLeft: `${13 + row.depth * 14}px` }"
        :data-container-id="row.value.id"
        role="treeitem"
        :aria-level="row.depth + 1"
        :aria-selected="
          row.kind === 'list' ? activeList === row.value.id : undefined
        "
        :aria-expanded="
          row.kind === 'folder' ? !closed.has(row.value.id) : undefined
        "
        :aria-label="row.value.name"
        tabindex="0"
        draggable="true"
        @click="activate(row)"
        @keydown="keydown($event, row, index)"
        @contextmenu.prevent="openContext($event, row)"
        @dragstart="dragStart($event, row)"
        @dragover.stop="dragOver($event, row)"
        @drop="drop($event, row)"
        @dragend="dragEnd"
        @dragleave="dragLeave"
      >
        <span class="tree-leading">
          <template v-if="row.kind === 'folder'">
            <PhCaretRight
              v-if="closed.has(row.value.id)"
              class="folder-caret"
              :size="11"
            />
            <PhCaretDown v-else class="folder-caret" :size="11" />
            <span v-if="row.value.icon" class="tree-emoji" aria-hidden="true">{{
              row.value.icon
            }}</span>
            <PhFolder v-else-if="closed.has(row.value.id)" :size="19" />
            <PhFolderOpen v-else :size="19" />
          </template>
          <span
            v-else-if="row.value.icon"
            class="tree-emoji"
            aria-hidden="true"
            >{{ row.value.icon }}</span
          >
          <component
            v-else
            :is="row.value.list_type === 'note' ? PhNote : PhList"
            :size="19"
            :style="
              showColor !== false &&
              row.value.color &&
              row.value.color !== '#5b7fff'
                ? { color: row.value.color }
                : undefined
            "
          />
        </span>
        <span class="tree-label">{{ row.value.name }}</span>
        <span
          v-if="showCounts !== false && count(row)"
          class="tree-count"
          aria-hidden="true"
          >{{ count(row) }}</span
        >
        <div class="tree-actions">
          <button
            v-if="row.kind === 'folder'"
            class="tree-tool"
            :aria-label="`Add to ${row.value.name}`"
            :title="`Add to ${row.value.name}`"
            :aria-expanded="addMenu === row.value.id"
            @click.stop="
              addMenu = addMenu === row.value.id ? null : row.value.id
            "
          >
            <PhPlus :size="14" />
          </button>
          <button
            class="tree-tool"
            :aria-label="`Actions for ${row.value.name}`"
            :title="`Actions for ${row.value.name}`"
            aria-haspopup="menu"
            :aria-expanded="context?.id === row.value.id"
            @click.stop="openContext($event, row)"
          >
            <PhDotsThree :size="18" weight="bold" />
          </button>
        </div>
        <div
          v-if="row.kind === 'folder' && addMenu === row.value.id"
          class="tree-create-menu"
          role="menu"
          @click.stop
        >
          <button role="menuitem" @click="create('list', row.value.id)">
            <PhList :size="17" />New List
          </button>
          <button role="menuitem" @click="create('folder', row.value.id)">
            <PhFolder :size="17" />New Folder
          </button>
        </div>
      </div>
    </div>
    <button
      v-if="!rows.length"
      class="tree-empty"
      @click="create('list', null)"
    >
      <PhPlus :size="16" />Add a list
    </button>
    <div
      class="tree-root-drop"
      :class="{ 'root-over': dropTarget === 'root', 'is-dragging': !!dragging }"
      @dragover="rootOver"
      @drop="rootDrop"
      @dragleave="dragLeave"
    >
      <span v-if="dragging">Move to top level</span>
    </div>
  </div>
  <Teleport to="body">
    <div
      v-if="context && contextItem"
      ref="contextElement"
      class="sidebar-context-menu"
      :style="{ left: `${context.x}px`, top: `${context.y}px` }"
      role="menu"
      tabindex="-1"
      :aria-label="`${contextItem.name} actions`"
      @keydown="menuKeys"
    >
      <button
        v-if="context.kind === 'folder'"
        role="menuitem"
        @click="create('list', context.id)"
      >
        Add List
      </button>
      <button role="menuitem" @click="editContext">Edit</button>
      <button role="menuitem" @click="pinContext">
        {{ contextItem.pinned ? "Unpin" : "Pin" }}
      </button>
      <button
        v-if="context.kind === 'list'"
        role="menuitem"
        @click="duplicateContext"
      >
        Duplicate
      </button>
      <button
        v-if="context.kind === 'list'"
        role="menuitem"
        @click="archiveContext"
      >
        {{
          "archived_at" in contextItem && contextItem.archived_at
            ? "Unarchive"
            : "Archive"
        }}
      </button>
      <button
        v-if="context.kind === 'list'"
        role="menuitem"
        @click="removeContext"
      >
        Delete
      </button>
      <button v-else role="menuitem" @click="ungroupContext">Ungroup</button>
    </div>
  </Teleport>
</template>

<style scoped>
.sidebar-tree {
  padding: 0 7px 0 4px;
  color: var(--text, #292a2d);
}
.tree-heading {
  height: 55px;
  display: flex;
  align-items: center;
  padding: 16px 15px 0;
  color: var(--sidebar-heading, var(--muted, #98999f));
  font-size: 12px;
  font-weight: 500;
  position: relative;
}
.heading-add {
  margin-left: auto;
  opacity: 0;
}
.tree-heading:hover .heading-add,
.tree-heading:focus-within .heading-add {
  opacity: 1;
}
.tree-rows {
  display: flex;
  flex-direction: column;
}
.tree-row {
  height: 36px;
  margin: 1px 0;
  display: flex;
  align-items: center;
  gap: 9px;
  padding-right: 11px;
  border-radius: 10px;
  cursor: default;
  user-select: none;
  position: relative;
  font-size: 14px;
  outline: none;
}
.tree-row:hover {
  background: var(--hover, #e4e5ea);
}
.tree-row.selected {
  background: var(--selection, #dcdde5);
}
.tree-row:focus-visible {
  box-shadow: inset 0 0 0 2px var(--accent, #5873de);
}
.tree-leading {
  width: 21px;
  min-width: 21px;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  position: relative;
}
.folder-caret {
  position: absolute;
  left: -9px;
  color: var(--muted, #98999f);
}
.tree-label {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  flex: 1;
  min-width: 0;
}
.tree-count {
  color: var(--muted, #98999f);
  font-size: 10px;
}
.tree-actions {
  display: none;
  align-items: center;
  gap: 1px;
  margin-right: -4px;
}
.tree-row:hover .tree-actions,
.tree-row:focus-within .tree-actions {
  display: flex;
}
.tree-row:hover .tree-count,
.tree-row:focus-within .tree-count {
  display: none;
}
.tree-tool {
  border: 0;
  padding: 3px;
  width: 23px;
  height: 23px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 5px;
  background: transparent;
  color: var(--secondary, #7e818c);
  cursor: pointer;
}
.tree-tool:hover {
  background: var(--selection, #dcdde5);
  color: var(--text, #292a2d);
}
.tree-tool:focus-visible,
.tree-empty:focus-visible,
.tree-create-menu button:focus-visible {
  outline: 2px solid var(--accent, #5873de);
  outline-offset: 1px;
}
.tree-create-menu {
  position: absolute;
  top: calc(100% - 2px);
  right: 4px;
  min-width: 154px;
  padding: 5px;
  z-index: 50;
  background: var(--popover, #fff);
  border: 1px solid var(--border, #e3e4e8);
  border-radius: 10px;
  box-shadow: 0 8px 25px #0002;
}
.tree-create-menu button {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 8px;
  border: 0;
  border-radius: 5px;
  font: inherit;
  font-size: 13px;
  color: var(--text, #292a2d);
  background: transparent;
  text-align: left;
  cursor: pointer;
}
.tree-create-menu button:hover {
  background: var(--hover, #f0f0f3);
}
.tree-empty {
  margin: 0 0 4px 9px;
  display: flex;
  align-items: center;
  gap: 8px;
  border: 0;
  background: transparent;
  color: var(--secondary, #8a8d96);
  padding: 7px 4px;
  font: inherit;
  font-size: 13px;
  cursor: pointer;
  border-radius: 5px;
}
.tree-root-drop {
  height: 0;
  overflow: hidden;
  margin-top: 0;
  border-radius: 6px;
  color: var(--muted, #92959f);
  font-size: 10px;
  text-align: center;
  display: flex;
  align-items: center;
  justify-content: center;
}
.tree-root-drop.is-dragging {
  height: 20px;
  margin-top: 4px;
}
.drop-inside,
.root-over {
  background: var(--selection, #dcdde5) !important;
  box-shadow: inset 0 0 0 1px var(--accent, #5873de);
}
.drop-before::before,
.drop-after::after {
  content: "";
  position: absolute;
  left: 8px;
  right: 8px;
  height: 2px;
  border-radius: 1px;
  background: var(--accent, #5873de);
}
.drop-before::before {
  top: -1px;
}
.drop-after::after {
  bottom: -1px;
}
@media (hover: none) {
  .heading-add {
    opacity: 1;
  }
  .tree-row:focus-within .tree-actions {
    display: flex;
  }
}
.tree-emoji {
  width: 18px;
  height: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-family: "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", sans-serif;
  font-size: 16px;
  line-height: 1;
  flex-shrink: 0;
}
.sidebar-context-menu {
  position: fixed;
  z-index: 300;
  width: 140px;
  padding: 5px;
  color: var(--text, #262626);
  background: var(--popover, #fff);
  border: 1px solid var(--border, #e5e5e7);
  border-radius: 13px;
  box-shadow:
    0 7px 26px #0002,
    0 1px 5px #0001;
  font-size: 13px;
  font-weight: 400;
  outline: none;
}
.sidebar-context-menu > button {
  width: 100%;
  min-height: 34px;
  display: flex;
  align-items: center;
  border: none;
  background: transparent;
  color: inherit;
  padding: 5px 11px;
  border-radius: 7px;
  text-align: left;
  font: inherit;
  cursor: default;
}
.sidebar-context-menu button:hover,
.sidebar-context-menu button:focus-visible {
  outline: none;
  background: var(--hover, #eceef3);
}
</style>
