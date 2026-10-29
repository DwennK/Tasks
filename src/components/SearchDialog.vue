<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import {
  PhCheckCircle,
  PhCircle,
  PhFunnel,
  PhList,
  PhMagnifyingGlass,
  PhTag,
  PhX,
  PhXCircle,
} from "@phosphor-icons/vue";
import AppDialog from "./AppDialog.vue";
import { taskMatchesSearch } from "../composables/useTasks";
import type { SavedFilter, Tag, Task, TaskList } from "../types";
const props = defineProps<{
  tasks: Task[];
  lists: TaskList[];
  tags: Tag[];
  filters: SavedFilter[];
}>();
export type SearchTarget = {
  kind: "task" | "list" | "tag" | "filter";
  id: string;
};
type SearchResult = SearchTarget & {
  name: string;
  detail?: string;
  icon: string;
  completed: boolean;
};
const emit = defineEmits<{ close: []; select: [target: SearchTarget] }>();
const query = ref("");
const input = ref<HTMLInputElement>();
const resultArea = ref<HTMLElement>();
const cursor = ref(-1);
const term = computed(() => query.value.trim().toLocaleLowerCase());
const groups = computed<{ label: string; items: SearchResult[] }[]>(() => {
  if (!term.value) return [];
  const named = (name: string) => name.toLocaleLowerCase().includes(term.value);
  const lists = props.lists.filter((list) => !list.archived_at);
  const listNames = new Map(lists.map((list) => [list.id, list.name]));
  return [
    {
      label: "Tasks",
      items: props.tasks
        .filter(
          (task) =>
            !task.deleted_at &&
            !task.wont_do_at &&
            listNames.has(task.list_id) &&
            taskMatchesSearch(task, term.value),
        )
        .map((task) => ({
          kind: "task" as const,
          id: task.id,
          name: task.title,
          detail: listNames.get(task.list_id),
          icon: "",
          completed: !!task.completed_at,
        })),
    },
    {
      label: "Tags",
      items: props.tags
        .filter((tag) => named(tag.name))
        .map((tag) => ({
          kind: "tag" as const,
          id: tag.id,
          name: tag.name,
          detail: "",
          icon: "",
          completed: false,
        })),
    },
    {
      label: "Lists",
      items: lists
        .filter((list) => named(list.name))
        .map((list) => ({
          kind: "list" as const,
          id: list.id,
          name: list.name,
          detail: "",
          icon: list.icon,
          completed: false,
        })),
    },
    {
      label: "Filters",
      items: props.filters
        .filter((filter) => named(filter.name))
        .map((filter) => ({
          kind: "filter" as const,
          id: filter.id,
          name: filter.name,
          detail: "",
          icon: "",
          completed: false,
        })),
    },
  ].filter((group) => group.items.length);
});
const results = computed(() => groups.value.flatMap((group) => group.items));
watch(query, () => {
  cursor.value = -1;
  if (resultArea.value) resultArea.value.scrollTop = 0;
});
watch(results, (items) => {
  if (cursor.value >= items.length) cursor.value = -1;
});
async function move(direction: number) {
  if (!results.value.length) return;
  cursor.value =
    cursor.value < 0
      ? direction > 0
        ? 0
        : results.value.length - 1
      : (cursor.value + direction + results.value.length) %
        results.value.length;
  await nextTick();
  const button =
    resultArea.value?.querySelectorAll<HTMLButtonElement>(".search-result")[
      cursor.value
    ];
  button?.focus();
  button?.scrollIntoView({ block: "nearest" });
}
function keys(event: KeyboardEvent) {
  if (event.isComposing || event.keyCode === 229) return;
  if (["ArrowDown", "ArrowUp"].includes(event.key)) {
    event.preventDefault();
    event.stopPropagation();
    void move(event.key === "ArrowDown" ? 1 : -1);
  } else if (
    event.key === "Enter" &&
    event.target === input.value &&
    results.value.length
  ) {
    event.preventDefault();
    emit("select", results.value[Math.max(cursor.value, 0)]!);
  }
}
function clear() {
  query.value = "";
  input.value?.focus();
}
</script>
<template>
  <AppDialog title="Search" panel-class="search-dialog" @close="emit('close')">
    <template #header>
      <div class="search-dialog-field" @keydown="keys">
        <PhMagnifyingGlass :size="21" aria-hidden="true" />
        <input
          ref="input"
          v-model="query"
          placeholder="Search"
          aria-label="Search tasks and notes"
          autofocus
          autocomplete="off"
          spellcheck="false"
        />
        <button
          class="search-control search-clear"
          aria-label="Clear search"
          :disabled="!query"
          @click="clear"
        >
          <PhXCircle :size="18" weight="fill" />
        </button>
        <button
          class="search-control"
          aria-label="Close search"
          @click="emit('close')"
        >
          <PhX :size="21" />
        </button>
      </div>
    </template>
    <div v-if="!term || !results.length" class="search-empty" role="status">
      <img src="/assets/search-empty.png" alt="" width="160" height="140" />
      <p>
        {{
          term ? "No results found" : "Search tasks, tags, lists and filters"
        }}
      </p>
      <small v-if="term">Try another word or phrase.</small>
    </div>
    <div v-else ref="resultArea" class="search-results" @keydown="keys">
      <section
        v-for="group in groups"
        :key="group.label"
        :aria-label="group.label"
      >
        <h2>
          {{ group.label }} <span>{{ group.items.length }}</span>
        </h2>
        <button
          v-for="item in group.items"
          :key="item.kind + item.id"
          class="search-result"
          :aria-label="`Open ${item.kind} ${item.name}`"
          @click="emit('select', item)"
          @focus="cursor = results.indexOf(item)"
        >
          <span
            v-if="item.icon"
            class="search-result-emoji"
            aria-hidden="true"
            >{{ item.icon }}</span
          >
          <component
            v-else
            :is="
              item.kind === 'task'
                ? item.completed
                  ? PhCheckCircle
                  : PhCircle
                : item.kind === 'tag'
                  ? PhTag
                  : item.kind === 'filter'
                    ? PhFunnel
                    : PhList
            "
            :size="18"
            aria-hidden="true"
          />
          <span
            class="search-result-title"
            :class="{ completed: item.completed }"
            >{{ item.name }}</span
          >
          <small v-if="item.detail">{{ item.detail }}</small>
        </button>
      </section>
    </div>
  </AppDialog>
</template>
<style>
.modal-backdrop:has(.search-dialog) {
  align-items: center;
  padding: 32px 20px;
}
.dialog.search-dialog {
  width: 560px;
  height: 426px;
  max-width: 100%;
  max-height: calc(100dvh - 64px);
  border: 0;
  border-radius: 12px;
  padding: 0 20px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  box-shadow: 0 4px 18px #0003;
}
.search-dialog-field {
  height: 55px;
  min-height: 55px;
  display: flex;
  align-items: center;
  gap: 10px;
  border-bottom: 1px solid #2d3b78;
  color: #808080;
}
.search-dialog-field input {
  flex: 1;
  width: 0;
  height: 100%;
  padding: 0;
  background: transparent;
  border: 0;
  outline: none;
  font-size: 16px;
  color: var(--text);
}
.search-dialog-field input::placeholder {
  color: #cdcdcd;
}
.search-control {
  display: inline-flex;
  justify-content: center;
  align-items: center;
  flex-shrink: 0;
  width: 26px;
  height: 32px;
  padding: 0;
  border-radius: 5px;
}
.search-control:hover:enabled {
  background: var(--hover);
}
.search-clear:disabled {
  opacity: 1;
  cursor: default;
}
.search-empty {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  align-items: center;
  padding-bottom: 56px;
  gap: 18px;
  color: #808080;
  text-align: center;
}
.search-empty img {
  width: 160px;
  height: 140px;
  object-fit: contain;
}
.search-empty p {
  margin: 0;
  font-size: 14px;
  line-height: 1.5;
}
.search-empty small {
  font-size: 12px;
  margin-top: -12px;
}
.search-results {
  overflow-y: auto;
  min-height: 0;
  padding: 10px 0 18px;
}
.search-results section + section {
  margin-top: 14px;
}
.search-results h2 {
  display: flex;
  gap: 8px;
  color: var(--secondary);
  font-size: 11px;
  font-weight: 500;
  padding: 5px 9px;
}
.search-results h2 span {
  color: var(--muted);
}
.search-result {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 10px;
  text-align: left;
  min-height: 38px;
  padding: 7px 9px;
  border-radius: 6px;
  font-size: 14px;
}
.search-result:hover,
.search-result:focus-visible {
  background: var(--selection);
  outline: none;
}
.search-result > svg {
  flex-shrink: 0;
  color: var(--secondary);
}
.search-result-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.search-result-title.completed {
  text-decoration: line-through;
  color: var(--secondary);
}
.search-result small {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  color: var(--muted);
}
.search-result-emoji {
  width: 18px;
  font-size: 14px;
  text-align: center;
  flex-shrink: 0;
}
:root[data-theme="dark"] .search-dialog-field {
  border-color: #839aff;
}
:root[data-theme="dark"] .search-dialog-field,
:root[data-theme="dark"] .search-empty {
  color: var(--secondary);
}
@media (max-width: 600px) {
  .modal-backdrop:has(.search-dialog) {
    padding: 24px 12px;
  }
  .dialog.search-dialog {
    height: 426px;
    max-height: calc(100dvh - 48px);
    padding: 0 16px;
  }
  .search-control {
    width: 36px;
    height: 44px;
  }
  .search-dialog-field {
    gap: 6px;
  }
  .search-result {
    min-height: 44px;
  }
  .search-empty p {
    font-size: 13px;
  }
}
</style>
