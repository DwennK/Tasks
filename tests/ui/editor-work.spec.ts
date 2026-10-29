import { expect, test } from "@playwright/test";
import {
  beginEditorWrite,
  editorWriteCheckpoint,
  waitForEditorWrites,
} from "../../src/components/editorWork";

test("quit waits for attachment writes and allows canceled pickers", async () => {
  const completeWrite = beginEditorWrite();
  const cancelPicker = beginEditorWrite();
  let finished = false;
  const waiting = waitForEditorWrites().then(() => {
    finished = true;
  });
  completeWrite();
  await Promise.resolve();
  expect(finished).toBe(false);
  cancelPicker();
  await waiting;
  expect(finished).toBe(true);
});

test("attachment failures cancel all active quit waiters", async () => {
  const completeWrite = beginEditorWrite();
  const first = expect(waitForEditorWrites()).rejects.toThrow("Disk is full");
  const second = expect(waitForEditorWrites()).rejects.toThrow("Disk is full");
  completeWrite(new Error("Disk is full"));
  // A duplicate callback cannot change a completed operation's outcome.
  completeWrite();
  await Promise.all([first, second]);
});

test("an earlier reported import failure does not prevent a later quit", async () => {
  beginEditorWrite()(new Error("Previously reported error"));
  const completeWrite = beginEditorWrite();
  const waiting = waitForEditorWrites();
  completeWrite();
  await expect(waiting).resolves.toBeUndefined();
});

test("quit includes attachment writes that start while it is already waiting", async () => {
  const first = beginEditorWrite();
  const waiting = waitForEditorWrites();
  const rejection = expect(waiting).rejects.toThrow("Could not read image");
  const second = beginEditorWrite();
  first();
  await Promise.resolve();
  second("Could not read image");
  await rejection;
});

test("quit detects an import failure during its initial input drain", async () => {
  const checkpoint = editorWriteCheckpoint();
  const completeWrite = beginEditorWrite();
  completeWrite(new Error("Import failed before first wait"));
  await expect(waitForEditorWrites(checkpoint)).rejects.toThrow(
    "Import failed before first wait",
  );
});

test("one checkpoint protects every drain in the same quit round", async () => {
  const checkpoint = editorWriteCheckpoint();
  await waitForEditorWrites(checkpoint);
  beginEditorWrite()(new Error("Import failed between waits"));
  await expect(waitForEditorWrites(checkpoint)).rejects.toThrow(
    "Import failed between waits",
  );
  // A deliberate later quit starts with a fresh checkpoint and can proceed.
  await expect(
    waitForEditorWrites(editorWriteCheckpoint()),
  ).resolves.toBeUndefined();
});
