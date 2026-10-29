import { nextTick, onBeforeUnmount, ref, watch, type Ref } from "vue";
import type { Task } from "../types";
import { useTasks } from "./useTasks";

type TitleElement = HTMLInputElement | HTMLTextAreaElement;
type TitleField = {
  reconcile: (final?: boolean) => void;
  release: () => void;
};
const fields = new Set<TitleField>();
const owners = new Map<string, TitleField>();

/** Capture pending WebKit input before blur, then final IME text after draining. */
export function captureTaskTitleFields(final = false) {
  for (const field of [...fields]) field.reconcile(final);
}

export function useTaskTitleField<T extends TitleElement>(
  task: () => Task,
  element: Ref<T | undefined>,
) {
  const { state, editTask } = useTasks();
  const composing = ref(false);
  let boundId = "";
  let projectedTitle = "";
  let baseVersion = 0;
  let disposed = false;
  let publishing = false;
  let observedDraft: (typeof state.drafts)[string] | undefined;
  let observedDraftVersion = 0;

  function projection() {
    const current = task();
    // Most list rows have no mounted title input. Their already-projected
    // task is enough; avoid scanning the snapshot for every passive row.
    if (!element.value)
      return {
        id: current.id,
        title: current.title,
        version: current.version,
        deleted: !!current.deleted_at,
      };
    const saved = state.snapshot.tasks.find((entry) => entry.id === current.id);
    const draft = state.drafts[current.id];
    return {
      id: current.id,
      title:
        typeof draft?.patch.title === "string"
          ? draft.patch.title
          : (saved?.title ?? current.title),
      version: draft?.version ?? saved?.version ?? current.version,
      deleted: !!(saved?.deleted_at ?? current.deleted_at),
    };
  }

  function ownsField() {
    return owners.get(boundId) === field;
  }

  function release() {
    if (ownsField()) owners.delete(boundId);
  }

  function observeDraftVersion() {
    const draft = state.drafts[boundId];
    // Only the shared draft's acknowledged own write (or explicit Retry) can
    // advance an unpublished edit. A CLI refresh changes the snapshot alone.
    if (
      draft &&
      draft === observedDraft &&
      draft.version > observedDraftVersion &&
      baseVersion === observedDraftVersion
    )
      baseVersion = draft.version;
    observedDraft = draft;
    observedDraftVersion = draft?.version ?? 0;
  }

  function project() {
    if (disposed || publishing) return;
    const current = projection();
    if (current.id !== boundId) return;
    const input = element.value;
    // The ref watcher still owns the previous DOM until its post-flush capture.
    // A detached input must not adopt a refreshed CLI version in that gap.
    if (!input && ownsField()) return;
    // IME and undelivered native input own their DOM until explicitly captured.
    if (
      composing.value ||
      (ownsField() && input && input.value !== projectedTitle)
    )
      return;
    projectedTitle = current.title;
    baseVersion = current.version;
    if (input && input.value !== current.title) input.value = current.title;
  }

  function reconcile(final = false, input = element.value) {
    if (disposed || !ownsField() || !input || (composing.value && !final))
      return;
    const saved = state.snapshot.tasks.find((entry) => entry.id === boundId);
    const title = input.value;
    // A passive or unchanged DOM value is never a new edit, even if a refresh
    // has advanced the shared title while this field was composing.
    if (title === projectedTitle) {
      project();
      return;
    }
    const shared = state.drafts[boundId]?.patch.title ?? saved?.title;
    projectedTitle = title;
    if (title !== shared) {
      // Creating a draft and assigning its patch are separate reactive writes.
      // Do not project their intermediate state back into an active text field.
      publishing = true;
      try {
        if (!saved || saved.deleted_at) {
          // A remote deletion can make the field readonly before WebKit ends
          // composition. Retain that text locally without updating the task.
          const draft = state.drafts[boundId];
          if (draft) {
            draft.patch.title = title;
            draft.version = Math.min(draft.version, baseVersion);
            draft.failed = true;
          } else {
            state.drafts[boundId] = {
              patch: { title },
              version: baseVersion,
              failed: true,
            };
          }
          state.error =
            "This task was deleted while you were editing. Your title draft is kept.";
        } else editTask(boundId, { title }, baseVersion);
      } finally {
        publishing = false;
      }
    }
    if (!composing.value && task().id === boundId) project();
  }

  const field: TitleField = { reconcile, release };

  function claim() {
    if (disposed || task().id !== boundId || projection().deleted) return false;
    const previous = owners.get(boundId);
    if (previous && previous !== field) {
      previous.reconcile(true);
      previous.release();
    }
    owners.set(boundId, field);
    return true;
  }

  function focus() {
    if (!claim()) return;
    project();
  }

  function beforeInput() {
    if (document.activeElement === element.value) claim();
  }

  function input(event: Event) {
    if (!ownsField() && document.activeElement !== element.value) return;
    if (!claim()) return;
    if (composing.value || (event as InputEvent).isComposing) return;
    reconcile();
  }

  function compositionStart() {
    if (!ownsField() && document.activeElement !== element.value) return;
    if (!claim()) return;
    // This version belongs to the text displayed when composition began. A
    // concurrent CLI refresh must not silently rebase the eventual title.
    composing.value = true;
  }

  function compositionEnd(_event?: Event) {
    if (!composing.value) {
      project();
      return;
    }
    composing.value = false;
    reconcile(true);
    if (document.activeElement !== element.value) release();
    project();
  }

  function blur() {
    if (composing.value) return;
    reconcile();
    release();
    project();
  }

  watch(
    projection,
    (current) => {
      if (current.id !== boundId) {
        // Capture against the old ID before a reused textarea receives its
        // next task. It has no Vue :value binding that can replace this DOM.
        reconcile(true);
        release();
        composing.value = false;
        boundId = current.id;
        projectedTitle = current.title;
        baseVersion = current.version;
        observedDraft = state.drafts[boundId];
        observedDraftVersion = observedDraft?.version ?? 0;
        if (element.value) element.value.value = current.title;
      } else {
        observeDraftVersion();
        project();
      }
    },
    { immediate: true, flush: "sync" },
  );
  watch(
    element,
    (current, previous) => {
      if (previous && previous !== current) {
        reconcile(true, previous);
        release();
        composing.value = false;
      }
      if (current) fields.add(field);
      else fields.delete(field);
      project();
      if (current === document.activeElement) focus();
    },
    { immediate: true, flush: "post" },
  );
  void nextTick(project);

  onBeforeUnmount(() => {
    reconcile(true);
    release();
    fields.delete(field);
    composing.value = false;
    disposed = true;
  });

  return {
    focus,
    beforeInput,
    input,
    compositionStart,
    compositionEnd,
    blur,
    reconcile,
    composing,
  };
}
