<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  useId,
  watch,
} from "vue";
import { PhCaretDown, PhCheck } from "@phosphor-icons/vue";

type SelectValue = string | number;
type SelectOption = { value: SelectValue; label: string };

const props = withDefaults(
  defineProps<{
    modelValue: SelectValue;
    options: SelectOption[];
    label: string;
    disabled?: boolean;
    menuMinWidth?: number;
  }>(),
  { disabled: false, menuMinWidth: 0 },
);
const emit = defineEmits<{
  "update:modelValue": [value: SelectValue];
  "open-change": [open: boolean];
}>();

const instanceId = useId();
const listId = `date-select-${instanceId}`;
const root = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const list = ref<HTMLElement>();
const open = ref(false);
const activeIndex = ref(-1);
const menuStyle = ref<Record<string, string>>({});
const selectedIndex = computed(() =>
  props.options.findIndex((option) =>
    Object.is(option.value, props.modelValue),
  ),
);
const selectedLabel = computed(
  () => props.options[selectedIndex.value]?.label ?? "",
);
const unavailable = computed(
  () => props.disabled || props.options.length === 0,
);
let typeahead = "";
let typeaheadTimer: ReturnType<typeof setTimeout> | undefined;
let positionFrame = 0;
let resizeObserver: ResizeObserver | undefined;

function optionId(index: number) {
  return `${listId}-option-${index}`;
}

function clearTypeahead() {
  typeahead = "";
  clearTimeout(typeaheadTimer);
}

function positionMenu() {
  const button = trigger.value;
  const menu = list.value;
  if (!open.value || !button || !menu) return;

  const anchor = button.getBoundingClientRect();
  const viewport = window.visualViewport;
  const leftEdge = (viewport?.offsetLeft ?? 0) + 8;
  const topEdge = (viewport?.offsetTop ?? 0) + 8;
  const rightEdge = leftEdge + (viewport?.width ?? window.innerWidth) - 16;
  const bottomEdge = topEdge + (viewport?.height ?? window.innerHeight) - 16;
  const availableWidth = Math.max(0, rightEdge - leftEdge);
  const minimumWidth = Math.min(
    availableWidth,
    Math.max(anchor.width, props.menuMinWidth),
  );

  // Measure its intrinsic text width before clamping it to the viewport.
  menu.style.width = "max-content";
  menu.style.minWidth = `${minimumWidth}px`;
  menu.style.maxWidth = `${availableWidth}px`;
  const width = Math.min(availableWidth, menu.getBoundingClientRect().width);
  const fullHeight = props.options.length * 34 + 8;
  const below = Math.max(0, bottomEdge - anchor.bottom);
  const above = Math.max(0, anchor.top - topEdge);
  const placeBelow =
    fullHeight <= below || (fullHeight > above && below >= above);
  const height = Math.min(fullHeight, placeBelow ? below : above);
  const left = Math.max(leftEdge, Math.min(anchor.left, rightEdge - width));
  const top = placeBelow ? anchor.bottom : anchor.top - height;

  menuStyle.value = {
    left: `${left}px`,
    top: `${Math.max(topEdge, Math.min(top, bottomEdge - height))}px`,
    width: `${width}px`,
    minWidth: `${minimumWidth}px`,
    maxWidth: `${availableWidth}px`,
    maxHeight: `${height}px`,
    visibility: "visible",
  };
}

function requestPosition() {
  if (!open.value || positionFrame) return;
  positionFrame = requestAnimationFrame(() => {
    positionFrame = 0;
    positionMenu();
  });
}

function revealActive() {
  const menu = list.value;
  const option = menu?.children[activeIndex.value] as HTMLElement | undefined;
  if (!menu || !option) return;
  const upper = option.offsetTop - 4;
  const lower = option.offsetTop + option.offsetHeight + 4;
  if (upper < menu.scrollTop) menu.scrollTop = upper;
  else if (lower > menu.scrollTop + menu.clientHeight)
    menu.scrollTop = lower - menu.clientHeight;
}

async function showMenu(index = selectedIndex.value) {
  if (unavailable.value) return;
  activeIndex.value = Math.max(0, Math.min(index, props.options.length - 1));
  if (!open.value) {
    open.value = true;
    menuStyle.value = { visibility: "hidden" };
    emit("open-change", true);
  }
  await nextTick();
  positionMenu();
  await nextTick();
  revealActive();
}

function closeMenu(restoreFocus = false) {
  if (!open.value) return;
  open.value = false;
  clearTypeahead();
  emit("open-change", false);
  if (restoreFocus) trigger.value?.focus({ preventScroll: true });
}

function closeOpenMenu() {
  if (!open.value) return false;
  closeMenu(true);
  return true;
}
defineExpose({ closeOpenMenu });

function choose(index: number) {
  const option = props.options[index];
  if (!option || unavailable.value) return;
  emit("update:modelValue", option.value);
  closeMenu(true);
}

function moveActive(index: number) {
  activeIndex.value = Math.max(0, Math.min(index, props.options.length - 1));
  void nextTick(revealActive);
}

function normalize(value: string) {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLocaleLowerCase();
}

function searchByKey(key: string) {
  clearTimeout(typeaheadTimer);
  typeahead += normalize(key);
  const repeated = [...typeahead].every((letter) => letter === typeahead[0]);
  const search = repeated ? typeahead[0] : typeahead;
  const from = repeated ? activeIndex.value + 1 : activeIndex.value;
  for (let offset = 0; offset < props.options.length; offset += 1) {
    const index = (Math.max(0, from) + offset) % props.options.length;
    if (normalize(props.options[index].label).startsWith(search)) {
      moveActive(index);
      break;
    }
  }
  typeaheadTimer = setTimeout(clearTypeahead, 700);
}

function keydown(event: KeyboardEvent) {
  if (unavailable.value || event.isComposing) return;
  if (event.key === "Tab") {
    closeMenu();
    return;
  }
  if (event.key === "Escape") {
    if (!open.value) return;
    event.preventDefault();
    event.stopPropagation();
    closeMenu(true);
    return;
  }
  if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
    event.preventDefault();
    event.stopPropagation();
    clearTypeahead();
    if (!open.value) {
      void showMenu(
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? props.options.length - 1
            : selectedIndex.value,
      );
    } else {
      moveActive(
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? props.options.length - 1
            : activeIndex.value + (event.key === "ArrowDown" ? 1 : -1),
      );
    }
    return;
  }
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    event.stopPropagation();
    if (open.value) choose(activeIndex.value);
    else void showMenu();
    return;
  }
  if (
    event.key.length === 1 &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.altKey
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (!open.value) void showMenu();
    searchByKey(event.key);
  }
}

function outsidePointer(event: PointerEvent) {
  if (event.target instanceof Node && !root.value?.contains(event.target))
    closeMenu();
}

function focusout(event: FocusEvent) {
  if (
    !(event.relatedTarget instanceof Node) ||
    !root.value?.contains(event.relatedTarget)
  )
    closeMenu();
}

function scrolled(event: Event) {
  if (event.target instanceof Node && list.value?.contains(event.target))
    return;
  requestPosition();
}

watch(unavailable, (value) => {
  if (value) closeMenu();
});
watch(
  [() => props.modelValue, () => props.options],
  () => {
    if (!open.value) return;
    activeIndex.value = Math.max(0, selectedIndex.value);
    void nextTick(() => {
      positionMenu();
      revealActive();
    });
  },
  { deep: true },
);

onMounted(() => {
  document.addEventListener("pointerdown", outsidePointer, true);
  document.addEventListener("scroll", scrolled, true);
  window.addEventListener("resize", requestPosition);
  window.visualViewport?.addEventListener("resize", requestPosition);
  window.visualViewport?.addEventListener("scroll", requestPosition);
  resizeObserver = new ResizeObserver(requestPosition);
  if (trigger.value) resizeObserver.observe(trigger.value);
});
onBeforeUnmount(() => {
  clearTypeahead();
  cancelAnimationFrame(positionFrame);
  resizeObserver?.disconnect();
  document.removeEventListener("pointerdown", outsidePointer, true);
  document.removeEventListener("scroll", scrolled, true);
  window.removeEventListener("resize", requestPosition);
  window.visualViewport?.removeEventListener("resize", requestPosition);
  window.visualViewport?.removeEventListener("scroll", requestPosition);
});
</script>

<template>
  <span ref="root" class="date-picker-select" @focusout="focusout">
    <button
      ref="trigger"
      type="button"
      class="date-select-trigger"
      :class="{ 'is-open': open }"
      role="combobox"
      aria-haspopup="listbox"
      :aria-label="label"
      :aria-expanded="open"
      :aria-controls="open ? listId : undefined"
      :aria-activedescendant="open ? optionId(activeIndex) : undefined"
      :disabled="unavailable"
      @click="open ? closeMenu() : showMenu()"
      @keydown="keydown"
    >
      <span class="date-select-value">{{ selectedLabel }}</span>
      <PhCaretDown :size="12" aria-hidden="true" />
    </button>
    <span
      v-if="open"
      :id="listId"
      ref="list"
      class="date-select-list"
      role="listbox"
      :aria-label="label"
      :style="menuStyle"
      @pointerdown.prevent
    >
      <span
        v-for="(option, index) in options"
        :id="optionId(index)"
        :key="`${typeof option.value}:${option.value}`"
        class="date-select-option"
        :class="{
          'is-selected': index === selectedIndex,
          'is-active': index === activeIndex,
        }"
        role="option"
        :aria-selected="index === selectedIndex"
        @pointermove="activeIndex = index"
        @click="choose(index)"
      >
        <span class="date-select-option-label">{{ option.label }}</span>
        <PhCheck v-if="index === selectedIndex" :size="16" aria-hidden="true" />
        <span v-else class="date-select-check-space" aria-hidden="true" />
      </span>
    </span>
  </span>
</template>

<style scoped>
.date-picker-select {
  position: relative;
  display: inline-block;
  width: 100%;
  min-width: 0;
  height: 24px;
  vertical-align: top;
  color: var(--text, #202020);
  font-size: 14px;
}
.date-picker-select > .date-select-trigger {
  display: flex;
  align-items: center;
  width: 100%;
  min-width: 0;
  height: 24px;
  gap: 8px;
  padding: 0 9px;
  border: 1px solid var(--border, #e4e4e4);
  border-radius: 6px;
  background: var(--popover, #fff);
  color: inherit;
  font: inherit;
  text-align: left;
  outline: none;
}
.date-select-trigger.is-open,
.date-select-trigger:focus-visible {
  border-color: var(--date-accent, #303c70);
  box-shadow: 0 0 0 0.5px var(--date-accent, #303c70);
}
.date-select-value {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.date-select-trigger > svg {
  flex-shrink: 0;
  color: var(--muted, #b9b9b9);
}
.date-select-list {
  position: fixed;
  z-index: 240;
  display: block;
  padding: 4px 0;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  border-radius: 12px;
  background: var(--popover, #fff);
  box-shadow: 0 5px 18px #0003;
  color: var(--text, #202020);
  font-size: 14px;
  line-height: 20px;
  text-align: left;
}
.date-select-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 34px;
  gap: 10px;
  padding: 0 14px 0 18px;
  cursor: pointer;
  user-select: none;
  -webkit-user-select: none;
}
.date-select-option-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.date-select-option.is-selected {
  color: var(--date-accent, #303c70);
}
.date-select-option.is-active:not(.is-selected) {
  background: var(--hover, #f2f2f4);
}
.date-select-check-space {
  width: 16px;
  flex-shrink: 0;
}
</style>
