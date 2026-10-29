// File reads and native file pickers begin before a database operation enters
// the shared service queue. Retain them across editor unmounts for native quit.
const pending = new Set<Promise<void>>();
let failureEpoch = 0;
let latestFailure: Error | undefined;

export function beginEditorWrite(): (error?: unknown) => void {
  let resolve!: () => void;
  const work = new Promise<void>((done) => {
    resolve = done;
  });
  pending.add(work);
  let finished = false;
  return (error?: unknown) => {
    if (finished) return;
    finished = true;
    if (error !== undefined && error !== null) {
      failureEpoch += 1;
      latestFailure = error instanceof Error ? error : new Error(String(error));
    }
    pending.delete(work);
    resolve();
  };
}

export function editorWriteCheckpoint(): number {
  return failureEpoch;
}

export async function waitForEditorWrites(
  observedEpoch = failureEpoch,
): Promise<void> {
  // A native quit round captures this checkpoint before it drains WebKit input.
  // Imports that fail during that drain must also prevent closing the window.
  if (failureEpoch !== observedEpoch)
    throw latestFailure || new Error("An attachment could not be saved.");
  while (pending.size) {
    await Promise.all([...pending]);
    // An import can catch its own error to show it in the editor. A quit waiter
    // must still cancel closing instead of making that error disappear.
    if (failureEpoch !== observedEpoch)
      throw latestFailure || new Error("An attachment could not be saved.");
  }
}
