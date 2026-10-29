import { shallowRef } from "vue";
import type { Task } from "../types";

// DataTransfer payloads are protected during dragover in native WebKit.
// Keep the local source available so the preview uses the same rules as drop.
export const draggedTask = shallowRef<Task | null>(null);
export const taskDropTarget = shallowRef<{
  id: string;
  position: "before" | "after";
} | null>(null);

export function canDropTask(source: Task | null | undefined, target: Task) {
  return !!source && source.id !== target.id &&
    source.list_id === target.list_id && source.parent_id === target.parent_id;
}

export function taskDropPosition(event: DragEvent): "before" | "after" {
  const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
  return event.clientY >= bounds.top + bounds.height / 2 ? "after" : "before";
}

export function clearTaskDrag() {
  draggedTask.value = null;
  taskDropTarget.value = null;
}
