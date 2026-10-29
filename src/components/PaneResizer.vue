<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";
const props = defineProps<{
  label: string;
  value: number;
  min: number;
  max: number;
  defaultValue: number;
}>();
const emit = defineEmits<{
  resize: [value: number];
  commit: [value: number];
  cancel: [];
}>();
const dragging = ref(false);
let startX = 0,
  startValue = 0,
  currentValue = 0;
const clamp = (value: number) =>
  Math.round(Math.max(props.min, Math.min(props.max, value)));
function start(event: PointerEvent) {
  if (event.button !== 0) return;
  event.preventDefault();
  event.stopPropagation();
  startX = event.clientX;
  startValue = props.value;
  currentValue = props.value;
  dragging.value = true;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}
function move(event: PointerEvent) {
  if (!dragging.value) return;
  currentValue = clamp(startValue + event.clientX - startX);
  emit("resize", currentValue);
}
function finish(event: PointerEvent) {
  if (!dragging.value) return;
  dragging.value = false;
  (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  emit("commit", currentValue);
}
function cancel() {
  if (dragging.value) {
    dragging.value = false;
    emit("cancel");
  }
}
function keyboard(event: KeyboardEvent) {
  let value: number;
  if (event.key === "ArrowLeft")
    value = props.value - (event.shiftKey ? 30 : 10);
  else if (event.key === "ArrowRight")
    value = props.value + (event.shiftKey ? 30 : 10);
  else if (event.key === "Home") value = props.min;
  else if (event.key === "End") value = props.max;
  else return;
  event.preventDefault();
  event.stopPropagation();
  emit("commit", clamp(value));
}
onBeforeUnmount(cancel);
</script>
<template>
  <div class="pane-resizer" :class="{ dragging }">
    <div
      role="separator"
      tabindex="0"
      aria-orientation="vertical"
      :aria-label="label"
      :aria-valuenow="Math.round(value)"
      :aria-valuemin="Math.round(min)"
      :aria-valuemax="Math.round(max)"
      @pointerdown="start"
      @pointermove="move"
      @pointerup="finish"
      @pointercancel="cancel"
      @lostpointercapture="cancel"
      @keydown="keyboard"
      @dblclick="emit('commit', clamp(defaultValue))"
    ></div>
  </div>
</template>
<style scoped>
.pane-resizer {
  position: relative;
  flex: 0 0 0;
  width: 0;
  z-index: 12;
}
.pane-resizer > div {
  position: absolute;
  inset: 0 auto 0 -4px;
  width: 8px;
  cursor: col-resize;
  touch-action: none;
  outline: none;
}
.pane-resizer > div:after {
  content: "";
  position: absolute;
  inset: 0 auto 0 3px;
  width: 2px;
  background: var(--accent);
  opacity: 0;
  transition: opacity 0.15s;
}
.pane-resizer > div:hover:after,
.pane-resizer > div:focus-visible:after,
.pane-resizer.dragging > div:after {
  opacity: 0.65;
}
@media (max-width: 759px) {
  .pane-resizer {
    display: none;
  }
}
</style>
