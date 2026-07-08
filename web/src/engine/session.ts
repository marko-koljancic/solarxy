// The engine session: one `SolarxyClient` plus the mirror-sync glue. Every
// component mutates the document by calling `dispatch(command)`; the returned
// batch (and every frame's cook batch) is applied to the mirror store, with
// a full resnapshot on desync.

import {
  clearAutosaves,
  openSceneFile,
  readLatestAutosave,
  saveToFile,
  writeAutosave,
} from "../persistence/opfs";
import { useMirror } from "../store/mirror";
import { pushToast } from "../store/toasts";
import { SolarxyClient } from "./client";
import type { Command, EventBatch, GraphContext, NodeId, ParamSource, SaveExtra } from "./types";

// The import worker: created lazily on the first import job, then kept alive
// (idle-teardown is a later refinement). It runs `parse_model_job` in a
// second headless wasm instance and posts results back for the generation
// guard to accept or drop.
let importWorker: Worker | null = null;

interface WorkerResult {
  jobId: number;
  ctx: GraphContext;
  blob?: Uint8Array;
  error?: string;
}

function ensureImportWorker(): Worker {
  if (!importWorker) {
    importWorker = new Worker(new URL("./importWorker.ts", import.meta.url), { type: "module" });
    importWorker.onmessage = (e: MessageEvent<WorkerResult>) => onWorkerResult(e.data);
  }
  return importWorker;
}

function onWorkerResult(data: WorkerResult): void {
  if (!client) return;
  const c = getClient();
  const batch =
    data.error !== undefined
      ? c.submitParseError(data.ctx, data.jobId, data.error)
      : data.blob
        ? c.submitParsedModel(data.ctx, data.jobId, data.blob)
        : null;
  if (!batch) return;
  applyToMirror(batch);
  refreshStale();
}

/** Drains the import jobs the last cook spawned and posts each to the worker
 * with its (and its sidecars') bytes pulled fresh from the engine and
 * transferred. Runs every frame; usually a no-op. */
function pumpImportJobs(): void {
  if (!client) return;
  const jobs = getClient().takeImportJobs();
  if (jobs.length === 0) return;
  const worker = ensureImportWorker();
  for (const job of jobs) {
    const files: { name: string; bytes: Uint8Array }[] = [];
    const primary = getClient().assetBytes(job.hash);
    if (primary) files.push({ name: job.name, bytes: primary });
    for (const s of job.sidecars) {
      const b = getClient().assetBytes(s.hash);
      if (b) files.push({ name: s.name, bytes: b });
    }
    if (files.length === 0) {
      applyToMirror(getClient().submitParseError(job.ctx, job.jobId, "asset bytes not staged"));
      continue;
    }
    worker.postMessage(
      {
        jobId: job.jobId,
        ctx: job.ctx,
        format: job.format,
        optionsJson: JSON.stringify(job.options),
        files,
      },
      files.map((f) => f.bytes.buffer),
    );
  }
}

let client: SolarxyClient | null = null;
let booting: Promise<void> | null = null;
// An autosave found at boot (from a prior session), offered as recovery.
let pendingRecovery: { bytes: Uint8Array; when: number } | null = null;

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
export function takeRecovery(): { bytes: Uint8Array; when: number } | null {
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

/** The host `extra` for a `.slxy` save: generator + timestamps. The camera
 * comes from the app itself; canvas viewports and richer metadata are a
 * later refinement. */
function buildSaveExtra(): SaveExtra {
  const now = new Date().toISOString();
  return {
    generator: "solarxy-web 0.6.0",
    canvasViewports: {},
    meta: { name: "", description: "", projectId: "", created: now, modified: now },
  };
}

function doAutosave(): void {
  debounceTimer = null;
  lastSaveAt = Date.now();
  try {
    void writeAutosave(getClient().saveSlxy(buildSaveExtra()));
  } catch {
    /* not booted / serialize issue */
  }
}

/** Explicit save to a `.slxy` file; clears the dirty flag and autosave ring. */
export async function explicitSave(): Promise<void> {
  await saveToFile(getClient().saveSlxy(buildSaveExtra()), "scene.slxy");
  useMirror.getState().setDirty(false);
  await clearAutosaves();
}

/** Applies a loaded `.slxy` (recovery, open, or drop) to the mirror; the
 * camera is restored Rust-side. */
function applyLoadedScene(bytes: Uint8Array): void {
  const result = getClient().loadSlxy(bytes);
  applyToMirror(result.batch);
  useMirror.getState().setCurrent("root");
  useMirror.getState().setDirty(false);
  refreshStale();
  if (result.warnings.length > 0) {
    pushToast(`Scene loaded with ${result.warnings.length} warning(s).`, "warn");
  }
}

/** Restores a document from autosave `.slxy` bytes (recovery). */
export function restoreDocument(bytes: Uint8Array): void {
  applyLoadedScene(bytes);
}

/** Opens a `.slxy` from disk (the Open button / File System Access API). */
export async function openScene(): Promise<void> {
  const picked = await openSceneFile();
  if (picked) applyLoadedScene(picked.bytes);
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

/** Reads a File and stages its bytes into the engine (content-addressed),
 * returning the asset hash and original name. The JS-side SHA-256 matches
 * the engine's recomputed content id, so re-staging is idempotent. */
export async function stageFile(file: File): Promise<{ hash: string; name: string }> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  const sha256 = Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  const hash = getClient().stageAsset(file.name, file.type, sha256, bytes);
  return { hash, name: file.name };
}

const IMPORT_NODE: Record<string, string> = {
  obj: "import_obj",
  gltf: "import_gltf",
  glb: "import_gltf",
  stl: "import_stl",
  ply: "import_ply",
};

function extOf(name: string): string {
  return name.split(".").pop()?.toLowerCase() ?? "";
}

/** Stages dropped files and creates the matching import node referencing the
 * primary model. Companion files (mtl/bin/textures) are staged too and
 * resolved by name at parse time, so a multi-file `.gltf` just works. When
 * the current context is the root, a Geo container is created and entered
 * first (import nodes live in subflows). */
export async function importDroppedFiles(files: File[]): Promise<void> {
  // A dropped `.slxy` opens the scene rather than importing a model.
  if (files.length === 1 && extOf(files[0].name) === "slxy") {
    applyLoadedScene(new Uint8Array(await files[0].arrayBuffer()));
    return;
  }
  const primaryIdx = files.findIndex((f) => IMPORT_NODE[extOf(f.name)]);
  if (primaryIdx < 0) {
    pushToast("No model file in the drop (.obj / .gltf / .glb / .stl / .ply).", "warn");
    return;
  }
  const staged = await Promise.all(files.map(stageFile));
  const nodeType = IMPORT_NODE[extOf(files[primaryIdx].name)];

  let ctx = useMirror.getState().current;
  if (ctx === "root") {
    const geoBatch = dispatch({ type: "addNode", ctx: "root", nodeType: "geo", position: [40, 40] });
    const geoEv = geoBatch.events.find((e) => e.type === "nodeAdded");
    if (!geoEv || geoEv.type !== "nodeAdded") return;
    ctx = { subflow: geoEv.node.id };
    useMirror.getState().setCurrent(ctx);
  }

  const batch = dispatch({ type: "addNode", ctx, nodeType, position: [80, 80] });
  const added = batch.events.find((e) => e.type === "nodeAdded");
  if (!added || added.type !== "nodeAdded") return;
  dispatch({
    type: "setParam",
    ctx,
    node: added.node.id,
    key: "file",
    value: { kind: "literal", type: "asset", value: staged[primaryIdx].hash },
  });
  pushToast(`Importing ${files[primaryIdx].name}…`, "info");
}

/** Runs one cook+render frame and mirrors the cook batch. In manual mode,
 * refresh the stale set each frame so a just-cooked node drops its badge. */
export function runFrame(dtMs: number): void {
  applyToMirror(getClient().frame(dtMs));
  pumpImportJobs();
  if (useMirror.getState().cookMode === "manual") refreshStale();
}
