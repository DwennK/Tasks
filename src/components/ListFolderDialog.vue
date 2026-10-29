<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeMount,
  onBeforeUnmount,
  onMounted,
  ref,
} from "vue";
import {
  PhCaretDown,
  PhCheck,
  PhKanban,
  PhCalendarBlank,
  PhFolder,
  PhList,
  PhListBullets,
  PhNote,
  PhProhibit,
  PhSquare,
  PhX,
} from "@phosphor-icons/vue";
import type { Folder } from "../types";
import ListIconPicker from "./ListIconPicker.vue";
import { registerEscapeLayer } from "../lib/escape-layers";

interface ContainerValue {
  id?: string;
  name: string;
  color?: string;
  folder_id?: string | null;
  parent_id?: string | null;
  icon?: string;
  view_mode?: "list" | "board" | "timeline";
  show_in_smart_lists?: boolean;
  list_type?: "task" | "note";
}
const props = withDefaults(
  defineProps<{
    kind: "list" | "folder";
    item?: ContainerValue | null;
    folders: Folder[];
    initialParentId?: string | null;
    busy?: boolean;
    deleting?: boolean;
    error?: string;
  }>(),
  {
    item: null,
    initialParentId: null,
    busy: false,
    deleting: false,
    error: "",
  },
);
const emit = defineEmits<{
  save: [value: Record<string, unknown>];
  close: [];
  delete: [id: string];
}>();
const panel = ref<HTMLElement>(),
  nameInput = ref<HTMLInputElement>();
const name = ref(props.item?.name || "");
const color = ref(props.item?.color || "");
const icon = ref(props.item?.icon || "");
const parentId = ref<string | null>(
  props.kind === "list"
    ? (props.item?.folder_id ?? props.initialParentId)
    : (props.item?.parent_id ?? props.initialParentId),
);
const viewMode = ref<"list" | "board" | "timeline">(
  props.item?.view_mode || "list",
);
const listType = ref<"task" | "note">(props.item?.list_type || "task");
const showInSmartLists = ref(props.item?.show_in_smart_lists ?? true);
const dropdown = ref<"folder" | "type" | "smart" | null>(null);
const iconPicker = ref(false);
let previousFocus: HTMLElement | null = null;
let unregisterEscapeLayer: (() => void) | undefined;
const title = computed(() =>
  props.deleting
    ? props.kind === "list"
      ? "Delete List"
      : "Ungroup Folder"
    : `${props.item?.id ? "Edit" : "Add"} ${props.kind === "list" ? "List" : "Folder"}`,
);
const palette = [
  { value: "", name: "No color" },
  { value: "#ff6168", name: "Red" },
  { value: "#ffac3b", name: "Orange" },
  { value: "#ffda32", name: "Yellow" },
  { value: "#e0e650", name: "Lime" },
  { value: "#35cf6a", name: "Green" },
  { value: "#509bef", name: "Blue" },
  { value: "#7670ee", name: "Purple" },
];
const customColor = computed(
  () =>
    !!color.value &&
    !palette.some(
      (option) => option.value.toLowerCase() === color.value.toLowerCase(),
    ),
);
const availableFolders = computed(() => {
  const excluded = new Set<string>();
  if (props.kind === "folder" && props.item?.id) {
    excluded.add(props.item.id);
    let added = true;
    while (added) {
      added = false;
      for (const folder of props.folders)
        if (
          folder.parent_id &&
          excluded.has(folder.parent_id) &&
          !excluded.has(folder.id)
        ) {
          excluded.add(folder.id);
          added = true;
        }
    }
  }
  const rows: { id: string; name: string; icon: string; depth: number }[] = [];
  const visited = new Set<string>();
  function append(parent: string | null, depth: number) {
    for (const folder of props.folders
      .filter((f) => f.parent_id === parent && !excluded.has(f.id))
      .sort((a, b) => a.position - b.position)) {
      if (visited.has(folder.id)) continue;
      visited.add(folder.id);
      rows.push({
        id: folder.id,
        name: folder.name,
        icon: (folder as Folder & { icon?: string }).icon || "",
        depth,
      });
      append(folder.id, depth + 1);
    }
  }
  append(null, 0);
  return rows;
});
const selectedFolder = computed(() =>
  availableFolders.value.find((folder) => folder.id === parentId.value),
);
const nestedFolder = computed(
  () =>
    props.kind === "folder" &&
    !!(props.item?.parent_id || props.initialParentId),
);
function save() {
  if (!name.value.trim() || props.busy) return;
  emit("save", {
    name: name.value.trim(),
    icon: icon.value,
    ...(props.kind === "list"
      ? {
          color: color.value,
          folder_id: parentId.value,
          view_mode: viewMode.value,
          list_type: listType.value,
          show_in_smart_lists: showInSmartLists.value,
        }
      : { parent_id: parentId.value }),
  });
}
async function openMenu(menu: "folder" | "type" | "smart") {
  iconPicker.value = false;
  dropdown.value = dropdown.value === menu ? null : menu;
  if (dropdown.value) {
    await nextTick();
    panel.value
      ?.querySelector<HTMLElement>(
        '.container-options [role="option"][aria-selected="true"]',
      )
      ?.focus();
  }
}
function selectFolder(id: string | null) {
  parentId.value = id;
  dropdown.value = null;
}
function selectedIcon(value: string) {
  icon.value = value;
  iconPicker.value = false;
  nameInput.value?.focus();
}
function outside(event: PointerEvent) {
  if (
    !(event.target instanceof Element) ||
    event.target.closest(".container-selector, .container-icon-anchor")
  )
    return;
  dropdown.value = null;
  iconPicker.value = false;
}
function menuKey(event: KeyboardEvent) {
  if (!["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
  const elements = [
    ...(panel.value?.querySelectorAll<HTMLElement>(
      '.container-options [role="option"]',
    ) || []),
  ];
  if (!elements.length) return;
  event.preventDefault();
  const index = elements.indexOf(document.activeElement as HTMLElement);
  const next =
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? elements.length - 1
        : (index + (event.key === "ArrowDown" ? 1 : -1) + elements.length) %
          elements.length;
  elements[next]?.focus();
}
function keydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    if (event.defaultPrevented || event.isComposing || event.keyCode === 229)
      return;
    event.preventDefault();
    event.stopPropagation();
    if (dropdown.value || iconPicker.value) {
      dropdown.value = null;
      iconPicker.value = false;
      nameInput.value?.focus();
    } else emit("close");
    return;
  }
  if (event.key !== "Tab") return;
  const buttons = panel.value?.querySelectorAll<HTMLElement>(
    'button:not(:disabled), input:not(:disabled), [tabindex="0"]',
  );
  if (!buttons?.length) return;
  const first = buttons[0],
    last = buttons[buttons.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last?.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first?.focus();
  }
}
onBeforeMount(() => {
  unregisterEscapeLayer = registerEscapeLayer((event) => {
    if (event.target instanceof Node && panel.value?.contains(event.target))
      return;
    keydown(event);
    event.stopImmediatePropagation();
  });
});
onMounted(async () => {
  previousFocus = document.activeElement as HTMLElement;
  document.addEventListener("pointerdown", outside);
  await nextTick();
  if (props.deleting) panel.value?.focus();
  else {
    nameInput.value?.focus();
    nameInput.value?.select();
  }
});
onBeforeUnmount(() => {
  unregisterEscapeLayer?.();
  document.removeEventListener("pointerdown", outside);
  previousFocus?.focus();
});
</script>

<template>
  <Teleport to="body"
    ><div class="container-backdrop" @mousedown.self="!busy && emit('close')">
      <section
        ref="panel"
        class="list-folder-dialog"
        :class="{
          'folder-dialog': kind === 'folder',
          'delete-container-dialog': deleting,
        }"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
        tabindex="-1"
        @keydown="keydown"
      >
        <button
          class="container-close"
          aria-label="Close dialog"
          :disabled="busy"
          @click="emit('close')"
        >
          <PhX :size="9" weight="bold" />
        </button>
        <form class="container-form-pane" @submit.prevent="save">
          <h2>{{ title }}</h2>
          <template v-if="!deleting"
            ><div class="container-name-field">
              <div class="container-icon-anchor">
                <button
                  type="button"
                  class="container-icon-button"
                  :aria-label="`Choose ${kind} icon`"
                  :aria-expanded="iconPicker"
                  @click="
                    iconPicker = !iconPicker;
                    dropdown = null;
                  "
                >
                  <span v-if="icon" class="container-emoji">{{ icon }}</span
                  ><PhFolder v-else-if="kind === 'folder'" :size="18" /><PhList
                    v-else
                    :size="18"
                  />
                </button>
                <div v-if="iconPicker" class="container-icon-popover">
                  <ListIconPicker
                    :value="icon"
                    @select="selectedIcon"
                    @close="iconPicker = false"
                  />
                </div>
              </div>
              <input
                ref="nameInput"
                v-model="name"
                :aria-label="`${kind === 'list' ? 'List' : 'Folder'} name`"
                :placeholder="`${kind === 'list' ? 'List' : 'Folder'} name`"
                maxlength="200"
                :disabled="busy"
              />
            </div>
            <template v-if="kind === 'list'"
              ><div class="container-preference color-preference">
                <span>List Color</span>
                <div
                  class="container-color-palette"
                  role="group"
                  aria-label="List color"
                >
                  <button
                    v-for="option in palette"
                    :key="option.name"
                    type="button"
                    class="container-color-choice"
                    :class="{
                      selected:
                        color.toLowerCase() === option.value.toLowerCase(),
                      'no-color-choice': !option.value,
                    }"
                    :style="
                      option.value ? { backgroundColor: option.value } : {}
                    "
                    :aria-label="option.name"
                    :aria-pressed="
                      color.toLowerCase() === option.value.toLowerCase()
                    "
                    @click="color = option.value"
                  >
                    <PhProhibit v-if="!option.value" :size="15" /></button
                  ><label
                    class="container-custom-color"
                    :class="{ selected: customColor }"
                    title="Custom color"
                    ><input
                      type="color"
                      :value="color || '#7670ee'"
                      aria-label="Custom list color"
                      @input="
                        color = ($event.target as HTMLInputElement).value
                      "
                  /></label>
                </div>
              </div>
              <div class="container-preference view-preference">
                <span>View Type</span>
                <div
                  class="container-view-types"
                  role="group"
                  aria-label="View type"
                >
                  <button
                    type="button"
                    :class="{ selected: viewMode === 'list' }"
                    :aria-pressed="viewMode === 'list'"
                    aria-label="List view"
                    title="List"
                    @click="viewMode = 'list'"
                  >
                    <PhListBullets :size="21" /></button
                  ><button
                    type="button"
                    :class="{ selected: viewMode === 'board' }"
                    :aria-pressed="viewMode === 'board'"
                    aria-label="Board view"
                    title="Board"
                    @click="viewMode = 'board'"
                  >
                    <PhKanban :size="21" />
                  </button>
                  <button
                    type="button"
                    :class="{ selected: viewMode === 'timeline' }"
                    :aria-pressed="viewMode === 'timeline'"
                    aria-label="Timeline view"
                    title="Timeline"
                    @click="viewMode = 'timeline'"
                  >
                    <PhCalendarBlank :size="21" />
                  </button>
                </div></div
            ></template>
            <div
              v-if="kind === 'list' || nestedFolder"
              class="container-preference folder-preference"
            >
              <span>Folder</span>
              <div class="container-selector">
                <button
                  type="button"
                  class="container-select-trigger"
                  aria-label="Parent folder"
                  aria-haspopup="listbox"
                  :aria-expanded="dropdown === 'folder'"
                  @click="openMenu('folder')"
                >
                  <span
                    ><span
                      v-if="selectedFolder?.icon"
                      class="select-folder-icon"
                      >{{ selectedFolder.icon }}</span
                    >{{ selectedFolder?.name || "No Folder" }}</span
                  ><PhCaretDown :size="11" />
                </button>
                <div
                  v-if="dropdown === 'folder'"
                  class="container-options"
                  role="listbox"
                  aria-label="Folder"
                  @keydown="menuKey"
                >
                  <button
                    type="button"
                    role="option"
                    :aria-selected="parentId === null"
                    @click="selectFolder(null)"
                  >
                    <span>No Folder</span
                    ><PhCheck v-if="!parentId" :size="16" /></button
                  ><button
                    v-for="folder in availableFolders"
                    :key="folder.id"
                    type="button"
                    role="option"
                    :aria-selected="parentId === folder.id"
                    :style="{ paddingLeft: `${16 + folder.depth * 12}px` }"
                    @click="selectFolder(folder.id)"
                  >
                    <span>{{ folder.icon }}{{ folder.name }}</span
                    ><PhCheck v-if="parentId === folder.id" :size="16" />
                  </button>
                </div>
              </div>
            </div>
            <template v-if="kind === 'list'"
              ><div class="container-preference select-preference">
                <span>List Type</span>
                <div class="container-selector">
                  <button
                    type="button"
                    class="container-select-trigger"
                    aria-label="List type"
                    aria-haspopup="listbox"
                    :aria-expanded="dropdown === 'type'"
                    @click="openMenu('type')"
                  >
                    <span>{{
                      listType === "task" ? "Task List" : "Note List"
                    }}</span
                    ><PhCaretDown :size="11" />
                  </button>
                  <div
                    v-if="dropdown === 'type'"
                    class="container-options"
                    role="listbox"
                    aria-label="List type"
                    @keydown="menuKey"
                  >
                    <button
                      v-for="type in ['task', 'note'] as const"
                      :key="type"
                      type="button"
                      role="option"
                      :aria-selected="listType === type"
                      @click="
                        listType = type;
                        dropdown = null;
                      "
                    >
                      <span>{{
                        type === "task" ? "Task List" : "Note List"
                      }}</span
                      ><PhCheck v-if="listType === type" :size="16" />
                    </button>
                  </div>
                </div>
              </div>
              <div class="container-preference select-preference">
                <span>Show in Smart List</span>
                <div class="container-selector">
                  <button
                    type="button"
                    class="container-select-trigger"
                    aria-label="Show in smart lists"
                    aria-haspopup="listbox"
                    :aria-expanded="dropdown === 'smart'"
                    @click="openMenu('smart')"
                  >
                    <span>{{
                      showInSmartLists ? "All tasks" : "Do not show"
                    }}</span
                    ><PhCaretDown :size="11" />
                  </button>
                  <div
                    v-if="dropdown === 'smart'"
                    class="container-options"
                    role="listbox"
                    aria-label="Show in smart lists"
                    @keydown="menuKey"
                  >
                    <button
                      v-for="show in [true, false]"
                      :key="String(show)"
                      type="button"
                      role="option"
                      :aria-selected="showInSmartLists === show"
                      @click="
                        showInSmartLists = show;
                        dropdown = null;
                      "
                    >
                      <span>{{ show ? "All tasks" : "Do not show" }}</span
                      ><PhCheck v-if="showInSmartLists === show" :size="16" />
                    </button>
                  </div>
                </div></div
            ></template>
          </template>
          <p v-else class="container-delete-copy">
            {{
              kind === "list"
                ? `Delete “${item?.name}”? Its tasks and notes will move to Inbox.`
                : `Ungroup “${item?.name}”? Its lists and subfolders will be kept.`
            }}
          </p>
          <p v-if="error" class="container-error" role="alert">{{ error }}</p>
          <footer class="container-dialog-actions">
            <button
              type="button"
              class="container-cancel"
              :disabled="busy"
              @click="emit('close')"
            >
              Cancel</button
            ><button
              v-if="deleting"
              type="button"
              class="container-save container-delete"
              :disabled="busy || !item?.id"
              @click="item?.id && emit('delete', item.id)"
            >
              {{ kind === "list" ? "Delete" : "Ungroup" }}</button
            ><button
              v-else
              type="submit"
              class="container-save"
              :disabled="busy || !name.trim()"
            >
              {{ busy ? "Saving…" : "Save" }}
            </button>
          </footer>
        </form>
        <aside
          v-if="kind === 'list' && !deleting"
          class="container-live-preview"
          aria-label="List appearance preview"
        >
          <div
            class="mini-workspace"
            :class="{ 'mini-board': viewMode === 'board' }"
          >
            <div class="mini-task-panel">
              <header>
                <span v-if="icon" class="mini-icon">{{ icon }}</span
                ><PhList
                  v-else
                  :size="9"
                  :style="color ? { color } : {}"
                /><strong>{{ name.trim() || "List name" }}</strong>
              </header>
              <div class="mini-input"></div>
              <template v-if="viewMode === 'list'"
                ><div
                  v-for="group in [0, 1]"
                  :key="group"
                  class="mini-task-group"
                >
                  <span class="mini-section-name"></span
                  ><span class="mini-count">{{ group === 0 ? 5 : 2 }}</span>
                  <div
                    v-for="(width, index) in group === 0
                      ? [86, 51, 124, 74, 103]
                      : [124, 87]"
                    :key="index"
                    class="mini-task-row"
                  >
                    <PhNote
                      v-if="listType === 'note'"
                      :size="9"
                      :style="{ color: color || '#a7a7a7' }"
                    /><PhSquare
                      v-else
                      :size="9"
                      :style="{
                        color:
                          index === 0 && group === 0
                            ? '#d8535a'
                            : index === 1 || group === 1
                              ? '#5677e9'
                              : '#a7a7a7',
                      }"
                    /><span
                      :style="{
                        width: `${width}px`,
                        borderLeft: color ? `2px solid ${color}` : undefined,
                      }"
                    ></span>
                  </div></div
              ></template>
              <div v-else class="mini-columns">
                <div v-for="column in [0, 1, 2]" :key="column">
                  <span class="mini-column-title"></span>
                  <div
                    v-for="card in [0, 1, 2]"
                    :key="card"
                    class="mini-board-card"
                  >
                    <PhSquare
                      :size="8"
                      :style="{ color: color || '#9498a7' }"
                    /><span></span><span></span>
                  </div>
                </div>
              </div>
            </div>
            <div v-if="viewMode === 'list'" class="mini-detail">
              <PhSquare :size="9" color="#d8535a" />
              <div class="mini-input"></div>
              <span></span><span></span>
            </div>
          </div>
        </aside>
      </section></div
  ></Teleport>
</template>

<style scoped>
.container-backdrop {
  position: fixed;
  inset: 0;
  z-index: 130;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
  background: transparent;
}
.list-folder-dialog {
  --container-surface: #fff;
  --container-text: #202020;
  --container-muted: #7d7d7d;
  --container-border: #e1e1e1;
  --container-accent: #303e70;
  position: relative;
  display: grid;
  grid-template-columns: 396px 384px;
  width: 780px;
  height: 426px;
  background: var(--container-surface);
  color: var(--container-text);
  border-radius: 11px;
  border: 1px solid #bfc0c4;
  box-shadow: 0 8px 24px #0004;
  font-size: 14px;
  outline: none;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 32px);
}
.list-folder-dialog:focus-visible {
  outline: none;
}
.container-close {
  position: absolute;
  left: 8px;
  top: 8px;
  width: 12px;
  height: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid #e75853;
  border-radius: 50%;
  padding: 0;
  color: #992b25;
  background: #ff625d;
  z-index: 2;
}
.container-close svg {
  opacity: 0;
}
.container-close:hover svg,
.container-close:focus-visible svg {
  opacity: 1;
}
.container-form-pane {
  display: flex;
  flex-direction: column;
  padding: 19px 24px 16px;
  min-width: 0;
  min-height: 0;
}
.container-form-pane h2 {
  font-size: 16px;
  line-height: 20px;
  font-weight: 600;
  text-align: center;
  margin: 0 0 24px;
  letter-spacing: -0.2px;
}
.container-name-field {
  display: flex;
  align-items: center;
  height: 36px;
  min-height: 36px;
  border: 1px solid var(--container-border);
  border-radius: 8px;
  background: var(--container-surface);
  gap: 6px;
  padding: 0 7px;
  position: relative;
}
.container-name-field:focus-within {
  border-color: var(--container-accent);
}
.container-name-field input {
  height: 100%;
  border: 0;
  background: none;
  flex: 1;
  min-width: 0;
  padding: 0 3px;
  font-size: 14px;
  color: var(--container-text);
}
.container-name-field input::placeholder {
  color: #aaa;
}
.container-icon-anchor {
  position: relative;
  flex-shrink: 0;
  display: flex;
  align-items: center;
}
.container-icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 26px;
  padding: 0;
  color: #aaa;
  border-radius: 4px;
}
.container-icon-button:hover {
  background: #eee;
}
.container-emoji {
  font-size: 16px;
  line-height: 20px;
}
.container-icon-popover {
  position: absolute;
  left: -20px;
  top: 31px;
  z-index: 20;
}
.container-preference {
  display: grid;
  grid-template-columns: 121px minmax(0, 1fr);
  align-items: center;
  min-height: 32px;
  gap: 0;
}
.container-preference > span {
  color: var(--container-muted);
  font-size: 13px;
  white-space: nowrap;
}
.color-preference {
  margin-top: 24px;
  min-height: 20px;
}
.container-color-palette {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}
.container-color-choice,
.container-custom-color {
  width: 20px;
  height: 20px;
  min-width: 20px;
  border: 0;
  border-radius: 50%;
  padding: 0;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.container-color-choice.selected:after,
.container-custom-color.selected:after {
  content: "";
  position: absolute;
  inset: 0;
  border: 2px solid var(--container-accent);
  border-radius: 50%;
  pointer-events: none;
}
.no-color-choice {
  background: #fff;
  box-shadow: inset 0 0 0 1px #e0e0e0;
  color: #edb2b5;
}
.no-color-choice.selected {
  box-shadow: inset 0 0 0 3px #fff;
}
.container-custom-color {
  background: conic-gradient(
    #f2ce88,
    #ed95c8,
    #aca2ef,
    #88d6df,
    #bce3ad,
    #f2ce88
  );
  overflow: hidden;
}
.container-custom-color input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  opacity: 0;
  cursor: pointer;
  border: 0;
  padding: 0;
}
.view-preference {
  margin-top: 20px;
  min-height: 36px;
}
.container-view-types {
  display: flex;
  gap: 12px;
}
.container-view-types button {
  width: 50px;
  height: 36px;
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #f7f7f7;
  color: #a4a4a4;
  padding: 0;
  border: 1px solid transparent;
}
.container-view-types button.selected {
  border-color: #8a90a8;
  background: #f6f5fa;
  color: var(--container-accent);
}
.folder-preference {
  margin-top: 19px;
}
.select-preference {
  margin-top: 12px;
}
.container-selector {
  position: relative;
  min-width: 0;
}
.container-select-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  height: 32px;
  border: 1px solid var(--container-border);
  border-radius: 9px;
  background: var(--container-surface);
  font-size: 14px;
  padding: 0 10px;
  color: var(--container-text);
}
.container-select-trigger > span {
  display: flex;
  align-items: center;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.container-select-trigger > svg {
  color: #c3c3c3;
}
.container-select-trigger[aria-expanded="true"] {
  border-color: var(--container-accent);
}
.select-folder-icon {
  font-size: 16px;
  margin-right: 1px;
}
.container-options {
  position: absolute;
  left: 0;
  right: 0;
  top: calc(100% + 4px);
  z-index: 15;
  background: var(--container-surface);
  border-radius: 12px;
  box-shadow: 0 5px 15px #0003;
  padding: 5px 0;
  max-height: 190px;
  overflow: auto;
}
.container-options button {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 7px 16px;
  min-height: 34px;
  width: 100%;
  font-size: 13px;
  text-align: left;
  color: var(--container-text);
}
.container-options button > span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.container-options button[aria-selected="true"] {
  color: var(--container-accent);
}
.container-options button:hover,
.container-options button:focus-visible {
  background: #f5f5f8;
  outline: none;
}
.container-dialog-actions {
  margin-top: auto;
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  align-items: center;
  padding-top: 10px;
}
.container-dialog-actions button {
  height: 30px;
  min-width: 80px;
  border-radius: 9px;
  font-size: 13px;
  padding: 0 17px;
}
.container-cancel {
  background: var(--container-surface);
  border: 1px solid var(--container-border);
}
.container-save {
  background: var(--container-accent);
  color: #fff;
}
.container-save:disabled {
  opacity: 0.45;
}
.container-dialog-actions .container-delete {
  background: #ce525b;
}
.container-error {
  font-size: 11px;
  line-height: 1.3;
  color: #c34d55;
  margin: 7px 0 -7px;
}
.container-live-preview {
  border-radius: 0 11px 11px 0;
  background: #f6f5fa;
  position: relative;
  overflow: hidden;
}
.mini-workspace {
  position: absolute;
  left: 36px;
  top: 52px;
  width: 500px;
  height: 322px;
  border-radius: 16px;
  background: #fff;
  box-shadow: -8px 8px 28px #00000004;
  display: flex;
  overflow: hidden;
  font-size: 9px;
}
.mini-task-panel {
  width: 253px;
  flex-shrink: 0;
  padding: 14px 14px;
  min-width: 0;
}
.mini-task-panel header {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 13px;
  margin-bottom: 9px;
  color: #333;
}
.mini-task-panel header > svg {
  color: #999;
}
.mini-task-panel strong {
  font-size: 9px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mini-icon {
  font-size: 9px;
}
.mini-input {
  height: 16px;
  border-radius: 3px;
  background: #fafafa;
  margin-bottom: 13px;
  width: 100%;
}
.mini-task-group {
  margin-bottom: 14px;
}
.mini-section-name {
  display: inline-block;
  width: 33px;
  height: 7px;
  background: #f5f5f5;
  border-radius: 1px;
  margin-right: 4px;
}
.mini-count {
  font-size: 5px;
  color: #999;
}
.mini-task-row {
  height: 17px;
  display: flex;
  align-items: center;
  gap: 5px;
}
.mini-task-row > span {
  height: 7px;
  background: #f5f5f5;
  border-radius: 1px;
}
.mini-detail {
  border-left: 1px solid #f8f8f8;
  flex: 1;
  padding: 16px 10px;
}
.mini-detail > .mini-input {
  margin-top: 11px;
  margin-bottom: 8px;
}
.mini-detail > span {
  display: block;
  background: #f5f5f5;
  width: 90px;
  height: 7px;
  margin-bottom: 8px;
}
.mini-detail > span:last-child {
  width: 55px;
}
.mini-board .mini-task-panel {
  width: 348px;
}
.mini-columns {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 9px;
}
.mini-column-title {
  width: 35px;
  height: 7px;
  background: #f3f3f4;
  display: block;
  margin: 3px 0 9px;
}
.mini-board-card {
  height: 47px;
  background: #fafafa;
  border: 1px solid #f4f4f4;
  border-radius: 5px;
  padding: 6px;
  margin-bottom: 8px;
}
.mini-board-card > span {
  display: block;
  height: 5px;
  background: #eeeeef;
  margin: 4px 0;
  width: 70%;
}
.mini-board-card > span:last-child {
  width: 45%;
}
.folder-dialog {
  width: 420px;
  height: 203px;
  display: block;
}
.folder-dialog .container-form-pane {
  height: 100%;
}
.folder-dialog .folder-preference {
  margin-top: 12px;
  min-height: 28px;
}
.folder-dialog
  .folder-preference
  > .container-selector
  > .container-select-trigger {
  height: 28px;
  font-size: 12px;
}
.folder-dialog .container-preference {
  grid-template-columns: 121px minmax(0, 1fr);
}
.delete-container-dialog {
  display: block;
  width: 420px;
  height: auto;
  min-height: 203px;
}
.container-delete-copy {
  font-size: 13px;
  line-height: 1.6;
  margin: 8px 0 25px;
  color: var(--container-muted);
}
.list-folder-dialog button:focus-visible {
  outline: 2px solid var(--container-accent);
  outline-offset: 2px;
}
.list-folder-dialog .container-options button:focus-visible {
  outline: 0;
  box-shadow: inset 0 0 0 1px var(--container-accent);
}
:global(:root[data-theme="dark"]) .list-folder-dialog {
  --container-surface: #2c2d35;
  --container-text: #e4e5e9;
  --container-muted: #a3a6b2;
  --container-border: #484b57;
  --container-accent: #637cbf;
  border-color: #545966;
}
:global(:root[data-theme="dark"]) .container-live-preview {
  background: #252733;
}
:global(:root[data-theme="dark"]) .mini-workspace {
  background: #30323d;
}
:global(:root[data-theme="dark"]) .mini-task-panel strong {
  color: #d4d7e2;
}
:global(:root[data-theme="dark"]) .mini-input,
:global(:root[data-theme="dark"]) .mini-task-row > span,
:global(:root[data-theme="dark"]) .mini-section-name,
:global(:root[data-theme="dark"]) .mini-detail > span,
:global(:root[data-theme="dark"]) .mini-column-title {
  background: #3d404d;
}
:global(:root[data-theme="dark"]) .mini-detail {
  border-color: #3a3d49;
}
:global(:root[data-theme="dark"]) .container-view-types button,
:global(:root[data-theme="dark"]) .container-options button:hover,
:global(:root[data-theme="dark"]) .container-icon-button:hover {
  background: #393b46;
}
:global(:root[data-theme="dark"]) .container-view-types button.selected {
  background: #353b50;
  color: #a8b8ec;
}
:global(:root[data-theme="dark"]) .mini-board-card {
  background: #383b48;
  border-color: #424552;
}
@media (max-width: 790px) {
  .list-folder-dialog {
    grid-template-columns: minmax(348px, 396px) minmax(0, 1fr);
    width: 780px;
  }
  .container-live-preview {
    min-width: 0;
  }
  .mini-workspace {
    left: 25px;
  }
  .container-form-pane {
    padding-left: 20px;
    padding-right: 20px;
  }
  .container-preference {
    grid-template-columns: 117px minmax(0, 1fr);
  }
  .container-color-palette {
    gap: 4px;
  }
  .folder-dialog,
  .delete-container-dialog {
    width: 420px;
    display: block;
  }
}
@media (max-width: 600px) {
  .container-backdrop {
    padding: 12px;
  }
  .list-folder-dialog {
    display: block;
    width: 396px;
    max-width: 100%;
    height: 426px;
    max-height: calc(100vh - 24px);
  }
  .container-form-pane {
    height: 100%;
    padding-left: 20px;
    padding-right: 20px;
  }
  .container-live-preview {
    display: none;
  }
  .container-preference {
    grid-template-columns: 117px minmax(0, 1fr);
  }
  .container-preference > span {
    font-size: 12px;
  }
  .container-color-palette {
    gap: 4px;
  }
  .container-color-choice,
  .container-custom-color {
    width: 18px;
    height: 18px;
    min-width: 18px;
  }
  .container-icon-popover {
    left: -13px;
    max-width: calc(100vw - 30px);
  }
  .folder-dialog {
    height: 203px;
  }
  .delete-container-dialog {
    height: auto;
    min-height: 203px;
  }
  .container-options {
    max-height: 150px;
  }
}
</style>
