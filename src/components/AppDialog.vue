<script setup lang="ts">
import { nextTick, onBeforeMount, onBeforeUnmount, onMounted, ref } from "vue";
import { PhX } from "@phosphor-icons/vue";
import { registerEscapeLayer } from "../lib/escape-layers";
defineProps<{ title: string; wide?: boolean; panelClass?: string }>();
const emit = defineEmits<{ close: [] }>();
const panel = ref<HTMLElement>();
let previous: HTMLElement | null = null;
let unregisterEscapeLayer: (() => void) | undefined;
function keydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    if (event.isComposing || event.keyCode === 229 || event.defaultPrevented)
      return;
    event.preventDefault();
    event.stopPropagation();
    emit("close");
  }
  if (event.key !== "Tab") return;
  const targets = panel.value?.querySelectorAll<HTMLElement>(
    'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex="0"]',
  );
  if (!targets?.length) return;
  const first = targets[0],
    last = targets[targets.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last?.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first?.focus();
  }
}
onBeforeMount(() => {
  // Register before slot children mount, so a nested picker owns Escape while
  // this dialog still takes priority over menus left open behind it.
  unregisterEscapeLayer = registerEscapeLayer((event) => {
    if (event.target instanceof Node && panel.value?.contains(event.target))
      return;
    event.preventDefault();
    event.stopImmediatePropagation();
    emit("close");
  });
});
onMounted(async () => {
  previous = document.activeElement as HTMLElement;
  await nextTick();
  (
    (panel.value?.querySelector("[autofocus]") as HTMLElement) || panel.value
  )?.focus();
});
onBeforeUnmount(() => {
  unregisterEscapeLayer?.();
  previous?.focus();
});
</script>
<template>
  <Teleport to="body"
    ><div class="modal-backdrop" @mousedown.self="emit('close')">
      <!-- Keep the panel immediately visible: inactive WebKit windows may
           suspend an opacity animation before its first painted frame. -->
      <section
        ref="panel"
        class="dialog"
        :class="[{ wide }, panelClass]"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
        tabindex="-1"
        @keydown="keydown"
      >
        <slot name="header"><header>
          <h2>{{ title }}</h2>
          <button
            class="icon-button"
            aria-label="Close dialog"
            @click="emit('close')"
          >
            <PhX :size="18" />
          </button>
        </header></slot>
        <slot />
      </section></div
  ></Teleport>
</template>
