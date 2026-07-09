// The engine session: one `SolarxyClient` plus the mirror-sync glue. Every
// component mutates the document by calling `dispatch(command)`; the returned
// batch (and every frame's cook batch) is applied to the mirror store, with
// a full resnapshot on desync.

import uvCheckerUrl from "../assets/uv-checker_1k.png";
import {
  clearAutosaves,
  openSceneFile,
  readLatestAutosave,
  saveToFile,
  writeAutosave,
} from "../persistence/opfs";
import { useMirror } from "../store/mirror";
import { pushToast } from "../store/toasts";
import { useViewState } from "../store/viewState";
import { SolarxyClient } from "./client";
import { ctxKey } from "./types";
import type {
  CameraCommand,
  Command,
  DisplaySettingsDto,
  EventBatch,
  GraphContext,
  NodeId,
  PaneDisplaySettings,
  ParamSource,
  SaveExtra,
  ViewLayout,
} from "./types";

// The import worker: created lazily on the first import job, then kept alive
// (idle-teardown is a later refinement). It runs `parse_model_job` and
// `validate_geometry_job` in a second headless wasm instance and posts
// results back for the generation guard to accept or drop.
let importWorker: Worker | null = null;

interface WorkerResult {
  kind: "parse" | "validate" | "hdri";
  jobId: number;
  ctx: GraphContext;
  blob?: Uint8Array;
  /** JSON `ValidationResult`: the implicit load validation beside a parse,
   * or the whole result of a validate job. */
  validation?: string;
  /** The packed `PreparedHdri` blob (hdri kind). */
  prepared?: Uint8Array;
  error?: string;
}

// HDRI preparations are host view-state work, not engine jobs: they ride
// the same worker but resolve through a local promise map keyed by a
// negative token (engine job ids are non-negative).
let hdriToken = -1;
const hdriWaiters = new Map<number, (r: WorkerResult) => void>();

/** Runs the CPU IBL stages in the worker; resolves with the packed
 * `PreparedHdri` blob. */
function prepareHdriInWorker(bytes: Uint8Array, format: string): Promise<Uint8Array> {
  const worker = ensureImportWorker();
  const token = hdriToken;
  hdriToken -= 1;
  return new Promise((resolve, reject) => {
    hdriWaiters.set(token, (r) => {
      if (r.error !== undefined || !r.prepared) reject(new Error(r.error ?? "prepare failed"));
      else resolve(r.prepared);
    });
    worker.postMessage({ kind: "hdri", jobId: token, ctx: "root", bytes, format }, [bytes.buffer]);
  });
}

function ensureImportWorker(): Worker {
  if (!importWorker) {
    importWorker = new Worker(new URL("./importWorker.ts", import.meta.url), { type: "module" });
    importWorker.onmessage = (e: MessageEvent<WorkerResult>) => onWorkerResult(e.data);
  }
  return importWorker;
}

function onWorkerResult(data: WorkerResult): void {
  if (data.kind === "hdri") {
    hdriWaiters.get(data.jobId)?.(data);
    hdriWaiters.delete(data.jobId);
    return;
  }
  if (!client) return;
  const c = getClient();
  let batch = null;
  if (data.kind === "validate") {
    batch =
      data.error !== undefined
        ? c.submitValidationError(data.ctx, data.jobId, data.error)
        : data.validation
          ? c.submitValidationResult(data.ctx, data.jobId, data.validation)
          : null;
  } else {
    batch =
      data.error !== undefined
        ? c.submitParseError(data.ctx, data.jobId, data.error)
        : data.blob
          ? c.submitParsedModel(data.ctx, data.jobId, data.blob, data.validation)
          : null;
  }
  if (!batch) return;
  applyToMirror(batch);
  refreshStale();
}

/** Drains the import and validate jobs the last cook spawned and posts each
 * to the worker (import bytes pulled fresh from the engine and transferred;
 * validate geometry pre-packed by the host). Runs every frame; usually a
 * no-op. */
function pumpImportJobs(): void {
  if (!client) return;
  const jobs = getClient().takeImportJobs();
  const validateJobs = getClient().takeValidateJobs();
  if (jobs.length === 0 && validateJobs.length === 0) return;
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
        kind: "parse",
        jobId: job.jobId,
        ctx: job.ctx,
        format: job.format,
        optionsJson: JSON.stringify(job.options),
        files,
      },
      files.map((f) => f.bytes.buffer),
    );
  }
  for (const job of validateJobs) {
    worker.postMessage(
      {
        kind: "validate",
        jobId: job.jobId,
        ctx: job.ctx,
        blob: job.blob,
        configJson: job.config,
        budget: job.budget,
      },
      [job.blob.buffer],
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
  booting = (async () => {
    // The UV-checker texture ships as a Vite asset (not baked into the
    // wasm) so the payload stays flat.
    const checker = new Uint8Array(await (await fetch(uvCheckerUrl)).arrayBuffer());
    const c = await SolarxyClient.create(canvas, checker);
    // Capture any prior autosave BEFORE flipping the boot flag: the
    // recovery prompt polls isBooted() and takes the record exactly once,
    // so the flag must never be visible with the capture still pending.
    pendingRecovery = await readLatestAutosave();
    client = c;
    useMirror.getState().setRegistry(c.registrySnapshot());
    useViewState.getState().setView(c.viewState());
    if (import.meta.env.DEV) {
      // Dev-only introspection hook (Chrome-automation verification).
      (window as unknown as Record<string, unknown>).__solarxy = {
        client: c,
        useViewState,
        useMirror,
      };
    }
  })();
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
  syncSceneSelection();
  markDirtyAndAutosave();
  return batch;
}

/** Pushes the root-context selection into the host so the picked object
 * gets its viewport tint (decision 24, node-to-viewport direction). */
export function syncSceneSelection(): void {
  if (!client) return;
  const root = useMirror.getState().contexts["root"];
  getClient().setSceneSelection(root?.selection[0]);
}

// ---- host-owned view state: actions mirror the returned DTO ----

export function setViewLayout(layout: ViewLayout): void {
  useViewState.getState().setView(getClient().setViewLayout(layout));
}

export function setSplitRatio(ratio: number): void {
  useViewState.getState().setView(getClient().setSplitRatio(ratio));
}

export function setActivePane(pane: number): void {
  useViewState.getState().setView(getClient().setActivePane(pane));
}

export function setPaneSettings(pane: number, settings: PaneDisplaySettings): void {
  useViewState.getState().setView(getClient().setPaneSettings(pane, settings));
}

export function setDisplaySettings(settings: DisplaySettingsDto): void {
  useViewState.getState().setView(getClient().setDisplaySettings(settings));
}

export function cameraCommand(pane: number, cmd: CameraCommand): void {
  getClient().cameraCommand(pane, cmd);
}

/** Refreshes the whole view-state mirror from the host. */
export function refreshViewState(): void {
  useViewState.getState().setView(getClient().viewState());
}

/** Flies the active pane's camera to a validation issue's mesh (report
 * panel row click) and mirrors the resulting view state (the host also
 * enables that pane's validation overlay). */
export function flyToIssue(objectNode: number, sourceNode: number, issue: number): void {
  useViewState.getState().setView(getClient().flyToIssue(objectNode, sourceNode, issue));
}

/** Stages an HDRI file, runs the CPU IBL stages in the worker, and installs
 * the environment (GPU finish + light rebind + skybox). */
export async function loadHdri(file: File): Promise<void> {
  const { hash, name } = await stageFile(file);
  const ext = extOf(file.name);
  const bytes = getClient().assetBytes(hash);
  if (!bytes) throw new Error("HDRI bytes not staged");
  const prepared = await prepareHdriInWorker(bytes, ext);
  getClient().setEnvironmentPrepared(hash, name, prepared);
  useViewState.getState().setEnvironment(getClient().environmentState());
  markDirtyAndAutosave();
  pushToast(`Environment: ${name}`, "info");
}

/** Clears the HDRI back to the procedural sky. */
export function clearEnvironment(): void {
  getClient().clearEnvironment();
  useViewState.getState().setEnvironment(getClient().environmentState());
  markDirtyAndAutosave();
  pushToast("Environment cleared", "info");
}

/** Sets the IBL contribution mode ("off" | "diffuse" | "full"). */
export function setIblMode(mode: string): void {
  getClient().setIblMode(mode);
  useViewState.getState().setEnvironment(getClient().environmentState());
  markDirtyAndAutosave();
}

/** Re-prepares a restored scene's HDRI from its embedded asset bytes
 * (async; the sky pops in when the worker finishes). */
async function restoreEnvironment(hdriHash: string | null): Promise<void> {
  useViewState.getState().setEnvironment(getClient().environmentState());
  if (!hdriHash) return;
  const bytes = getClient().assetBytes(hdriHash);
  if (!bytes) return;
  const prepared = await prepareHdriInWorker(bytes, "");
  getClient().setEnvironmentPrepared(hdriHash, "", prepared);
  useViewState.getState().setEnvironment(getClient().environmentState());
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
  refreshViewState();
  syncSceneSelection();
  void restoreEnvironment(result.environment.hdriHash);
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

/** Runs one cook+render frame, mirrors the cook batch, and drains host
 * events (pane-rect changes). In manual mode, refresh the stale set each
 * frame so a just-cooked node drops its badge. */
export function runFrame(dtMs: number): void {
  applyToMirror(getClient().frame(dtMs));
  pumpImportJobs();
  for (const ev of getClient().takeHostEvents()) {
    if (ev.type === "paneRects") useViewState.getState().setPaneRects(ev.rects);
    else if (ev.type === "activePane") useViewState.getState().setActivePaneMirror(ev.pane);
    else if (ev.type === "uvOverlap") useViewState.getState().setUvOverlap(ev.pct, ev.pending);
    else if (ev.type === "viewChanged") refreshViewState();
  }
  if (useMirror.getState().cookMode === "manual") refreshStale();
}

// Mirror the node canvas's current graph context to the host (the UV
// pane's selected-node source resolves against it). Subscribed once at
// module scope; deduped by context key.
let lastCtxKey = "root";
useMirror.subscribe((s) => {
  const key = ctxKey(s.current);
  if (key !== lastCtxKey && client) {
    lastCtxKey = key;
    getClient().setCurrentContext(s.current);
  }
});

// The engine session cannot survive hot-module replacement: the canvas
// already holds a WebGPU context owned by the old instance. Force a full
// reload instead of letting a zombie session linger (dev-only). The
// beforeunload dirty guard must not block this programmatic reload, or
// the swapped module graph boots a second app on the same canvas while
// the old page keeps running; suppress it for the reload (the autosave
// ring already holds the state).
if (import.meta.hot) {
  import.meta.hot.accept(() => {
    window.addEventListener("beforeunload", (e) => e.stopImmediatePropagation(), true);
    window.location.reload();
  });
}
