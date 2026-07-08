// The engine session: one `SolarxyClient` plus the mirror-sync glue. Every
// component mutates the document by calling `dispatch(command)`; the returned
// batch (and every frame's cook batch) is applied to the mirror store, with
// a full resnapshot on desync.

import { clearAutosaves, readLatestAutosave, saveToFile, writeAutosave } from "../persistence/opfs";
import { useMirror } from "../store/mirror";
import { SolarxyClient } from "./client";
import type { Command, EventBatch, GraphContext, NodeId, ParamSource } from "./types";

let client: SolarxyClient | null = null;
let booting: Promise<void> | null = null;
// An autosave found at boot (from a prior session), offered as recovery.
let pendingRecovery: { json: string; when: number } | null = null;

/** Boots the session over a canvas exactly once (safe under strict-mode
 * double-mount). Fetches the registry snapshot and captures any prior
 * autosave for the recovery prompt. */
export function bootSession(canvas: HTMLCanvasElement): Promise<void> {
  if (client) return Promise.resolve();
  if (booting) return booting;
  booting = SolarxyClient.create(canvas).then(async (c) => {
    client = c;
    useMirror.getState().setRegistry(c.registrySnapshot());
    pendingRecovery = await readLatestAutosave();
  });
  return booting;
}

/** Whether a prior-session autosave is available to recover. */
export function hasPendingRecovery(): boolean {
  return pendingRecovery !== null;
}

/** Reads and clears the pending recovery record (App shows the prompt). */
export function takeRecovery(): { json: string; when: number } | null {
  const r = pendingRecovery;
  pendingRecovery = null;
  return r;
}

export function getClient(): SolarxyClient {
  if (!client) throw new Error("engine session not booted");
  return client;
}

export function isBooted(): boolean {
  return client !== null;
}

function applyToMirror(batch: EventBatch): void {
  const store = useMirror.getState();
  if (store.applyBatch(batch)) {
    store.replaceFromSnapshot(getClient().snapshot(), batch.revision);
  }
}

/** Refreshes the stale (dirty) node set into the mirror store. */
export function refreshStale(): void {
  useMirror.getState().setStale(getClient().staleNodes());
}

/** Dispatches a command and mirrors the result. */
export function dispatch(cmd: Command): EventBatch {
  const batch = getClient().dispatch(cmd);
  applyToMirror(batch);
  refreshStale();
  markDirtyAndAutosave();
  return batch;
}

// Autosave: debounced 2s after the last mutation, forced at most every 15s.
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let lastSaveAt = 0;

function markDirtyAndAutosave(): void {
  useMirror.getState().setDirty(true);
  const now = Date.now();
  if (debounceTimer) clearTimeout(debounceTimer);
  const forceIn = Math.max(0, 15000 - (now - lastSaveAt));
  debounceTimer = setTimeout(doAutosave, Math.min(2000, forceIn));
}

function doAutosave(): void {
  debounceTimer = null;
  lastSaveAt = Date.now();
  try {
    void writeAutosave(JSON.stringify(getClient().saveScene()));
  } catch {
    /* not booted / serialize issue */
  }
}

/** Explicit save to a file; clears the dirty flag and the autosave ring. */
export async function explicitSave(): Promise<void> {
  await saveToFile(JSON.stringify(getClient().saveScene()), "scene.slxy.json");
  useMirror.getState().setDirty(false);
  await clearAutosaves();
}

/** Restores a document from an autosave JSON string (recovery). */
export function restoreDocument(json: string): void {
  const file = JSON.parse(json);
  const batch = getClient().loadScene(file);
  applyToMirror(batch);
  useMirror.getState().setCurrent("root");
  useMirror.getState().setDirty(false);
  refreshStale();
}

// In-memory clipboard fragment (application/x-solarxy-nodes shape). A
// system-clipboard bridge for cross-tab paste is a refinement.
let clipboard: unknown = null;

/** Copies the current context's selection to the clipboard. */
export function copySelection(): void {
  const s = useMirror.getState();
  const ids = (s.contexts[ctxKeyOf(s.current)]?.selection ?? []).slice();
  if (ids.length === 0) return;
  clipboard = getClient().copyNodes(s.current, ids);
}

/** Pastes the clipboard fragment into the current context. */
export function paste(): void {
  if (!clipboard) return;
  const s = useMirror.getState();
  dispatch({ type: "pasteNodes", ctx: s.current, fragment: clipboard, position: [30, 30] });
}

/** Duplicates the current context's selection (+24px), one undo step. */
export function duplicateSelection(): void {
  const s = useMirror.getState();
  const ids = (s.contexts[ctxKeyOf(s.current)]?.selection ?? []).slice();
  if (ids.length) dispatch({ type: "duplicateNodes", ctx: s.current, ids });
}

function ctxKeyOf(ctx: GraphContext): string {
  return ctx === "root" ? "root" : `sub:${ctx.subflow}`;
}

/** A transient param preview during a drag (no event, no undo). */
export function previewParam(ctx: GraphContext, node: NodeId, key: string, value: ParamSource): void {
  getClient().previewParam(ctx, node, key, value);
}

/** Runs one cook+render frame and mirrors the cook batch. In manual mode,
 * refresh the stale set each frame so a just-cooked node drops its badge. */
export function runFrame(dtMs: number): void {
  applyToMirror(getClient().frame(dtMs));
  if (useMirror.getState().cookMode === "manual") refreshStale();
}
