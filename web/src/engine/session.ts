// The engine session: one `SolarxyClient` plus the mirror-sync glue. Every
// component mutates the document by calling `dispatch(command)`; the returned
// batch (and every frame's cook batch) is applied to the mirror store, with
// a full resnapshot on desync.

import uvCheckerUrl from "../assets/uv-checker_1k.png";
import { reportWorkerError } from "../telemetry";
import {
  clearAutosaves,
  openSceneFile,
  readLatestAutosave,
  saveToFile,
  writeAutosave,
} from "../persistence/opfs";
import { nodeLabel } from "../flow/nodeLabel";
import { descriptorFor } from "../registry/datatypes";
import { syncCanvasSize } from "./canvas";
import { useMirror } from "../store/mirror";
import { usePrefs, type DisplayPrefs } from "../store/prefs";
import { useUi } from "../store/ui";
import { useRadial } from "../store/radial";
import { useReview } from "../store/review";
import { pushToast, useToasts } from "../store/toasts";
import { useViewState } from "../store/viewState";
import { SolarxyClient } from "./client";
import { useAttrPinStats } from "./attrPins";
import { applyMarkerPositions, hideAllMarkers } from "./markers";
import { hasMissing, missingSidecars, referencedSidecars } from "./sidecars";
import { ctxKey } from "./types";
import type {
  AttrVizState,
  CameraCommand,
  Command,
  DisplaySettingsDto,
  EventBatch,
  GraphContext,
  ImportOptions,
  NodeId,
  PaneDisplaySettings,
  ParamSource,
  PickDetail,
  ReviewAnchor,
  ReviewCategory,
  SaveExtra,
  ToolMode,
  ViewLayout,
} from "./types";

// The import worker: created lazily on the first import job, then kept alive
// (idle-teardown is a later refinement). It runs `parse_model_job` and
// `validate_geometry_job` in a second headless wasm instance and posts
// results back for the generation guard to accept or drop.
let importWorker: Worker | null = null;

interface WorkerResult {
  kind: "parse" | "validate" | "hdri" | "decodeImage";
  jobId: number;
  ctx: GraphContext;
  blob?: Uint8Array;
  /** JSON `ValidationResult`: the implicit load validation beside a parse,
   * or the whole result of a validate job. */
  validation?: string;
  /** The packed `PreparedHdri` blob (hdri kind). */
  prepared?: Uint8Array;
  /** Decoded RGBA8 (decodeImage kind). */
  width?: number;
  height?: number;
  pixels?: Uint8Array;
  error?: string;
  /** True when `error` is a wasm trap (our bug) rather than a bad input file
   * (the user's). The worker distinguishes them; only the former is reported. */
  fatal?: boolean;
  stack?: string;
}

// HDRI preparations are host view-state work, not engine jobs: they ride
// the same worker but resolve through a local promise map keyed by a
// negative token (engine job ids are non-negative).
let hdriToken = -1;
const hdriWaiters = new Map<number, (r: WorkerResult) => void>();

// Asset-preview parses are host-orchestrated the same way: a parse posted
// with a distinct negative token, resolved through this map rather than the
// engine's generation guard. Routed by token, so a superseded preview never
// commits to the document.
let previewToken = -1_000_000;
const previewWaiters = new Map<
  number,
  { resolve: (blob: Uint8Array) => void; reject: (e: Error) => void }
>();

/** Parses a staged model in the import worker (off the main thread) and
 * resolves with its geometry transfer blob, for the live asset preview. Hands
 * the worker the primary bytes plus every other staged file as a candidate
 * companion, since OBJ / glTF resolve textures and buffers by name. */
export async function previewParseModel(hash: string, name: string): Promise<Uint8Array> {
  // `async` so a synchronous throw in this prologue (getClient/assetBytes/
  // assetManifest/ensureImportWorker) surfaces as a promise rejection the
  // caller's `.catch` can handle, rather than an uncaught error in the effect.
  const c = getClient();
  const primary = c.assetBytes(hash);
  if (!primary) throw new Error("asset is not staged");
  const format = name.split(".").pop()?.toLowerCase() ?? "";
  const files: { name: string; bytes: Uint8Array }[] = [{ name, bytes: primary }];
  if (format === "obj" || format === "gltf" || format === "glb") {
    for (const ref of c.assetManifest()) {
      if (ref.hash === hash) continue;
      const b = c.assetBytes(ref.hash);
      if (b) files.push({ name: ref.name, bytes: b });
    }
  }
  const options: ImportOptions = {
    scale: 1.0,
    centerToOrigin: false,
    recomputeNormals: null,
    preserveMaterials: null,
    vertexColors: null,
  };
  const worker = ensureImportWorker();
  const token = previewToken;
  previewToken -= 1;
  return new Promise<Uint8Array>((resolve, reject) => {
    previewWaiters.set(token, { resolve, reject });
    try {
      worker.postMessage(
        {
          kind: "parse",
          jobId: token,
          ctx: "root",
          format,
          optionsJson: JSON.stringify(options),
          files,
        },
        files.map((f) => f.bytes.buffer),
      );
    } catch (e) {
      // A failed post never yields a worker reply; drop the waiter so it
      // does not leak, and reject so the caller sees the error.
      previewWaiters.delete(token);
      reject(e instanceof Error ? e : new Error(String(e)));
    }
  });
}

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
    // The fourth crash surface. The worker runs a SECOND wasm instance in its own
    // realm: the React error boundary cannot see it, and neither can the main
    // thread's `window.onerror`. Anything that escapes the worker's own try/catch
    // arrives here or nowhere.
    importWorker.onerror = (e: ErrorEvent) => {
      reportWorkerError(e.message || "import worker crashed", undefined);
    };
  }
  return importWorker;
}

function onWorkerResult(data: WorkerResult): void {
  // A wasm trap in the worker is our bug and must be reported. A bad model file
  // is not: it already becomes a toast, and reporting it would fill crash
  // reporting with other people's broken glTFs. The worker tells us which.
  if (data.fatal === true && data.error !== undefined) {
    reportWorkerError(`import worker: ${data.error}`, data.stack);
  }
  if (data.kind === "hdri") {
    hdriWaiters.get(data.jobId)?.(data);
    hdriWaiters.delete(data.jobId);
    return;
  }
  // Preview parses resolve their own promise (keyed by the negative token)
  // and never touch the document, so route them before the engine handling.
  if (data.kind === "parse" && previewWaiters.has(data.jobId)) {
    const waiter = previewWaiters.get(data.jobId);
    previewWaiters.delete(data.jobId);
    if (waiter) {
      if (data.error !== undefined || !data.blob) {
        waiter.reject(new Error(data.error ?? "parse produced no geometry"));
      } else {
        waiter.resolve(data.blob);
      }
    }
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
    if (data.error !== undefined) {
      pushToast(`Validation failed: ${data.error}`, "error");
    }
  } else if (data.kind === "decodeImage") {
    batch =
      data.error !== undefined
        ? c.submitImageError(data.ctx, data.jobId, data.error)
        : data.pixels && data.width !== undefined && data.height !== undefined
          ? c.submitDecodedImage(data.ctx, data.jobId, data.width, data.height, data.pixels)
          : null;
    if (data.error !== undefined) {
      pushToast(`Image decode failed: ${data.error}`, "error");
    }
  } else {
    batch =
      data.error !== undefined
        ? c.submitParseError(data.ctx, data.jobId, data.error)
        : data.blob
          ? c.submitParsedModel(data.ctx, data.jobId, data.blob, data.validation)
          : null;
    // A buried hover badge is not enough for a failed import (the node
    // often lives inside an auto-created subflow); say it out loud.
    if (data.error !== undefined) {
      pushToast(`Import failed: ${data.error}`, "error");
    }
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
  const imageJobs = getClient().takeImageJobs();
  const hdriJobs = getClient().takeHdriJobs();
  if (
    jobs.length === 0 &&
    validateJobs.length === 0 &&
    imageJobs.length === 0 &&
    hdriJobs.length === 0
  )
    return;
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
  for (const job of imageJobs) {
    const bytes = getClient().assetBytes(job.hash);
    if (!bytes) {
      applyToMirror(getClient().submitImageError(job.ctx, job.jobId, "asset bytes not staged"));
      continue;
    }
    worker.postMessage(
      { kind: "decodeImage", jobId: job.jobId, ctx: job.ctx, name: job.name, bytes },
      [bytes.buffer],
    );
  }
  for (const job of hdriJobs) {
    const bytes = getClient().assetBytes(job.hash);
    if (!bytes) {
      applyToMirror(getClient().submitHdriError(job.ctx, job.jobId, "asset bytes not staged"));
      continue;
    }
    // Reuses the worker's existing HDRI entry point, which decodes AND
    // runs the CPU lighting stages. A lean decode would leave the
    // irradiance convolution on the main thread, where it is a visible
    // stall on a large equirect.
    const ext = job.name.includes(".") ? extOf(job.name) : "";
    prepareHdriInWorker(bytes, ext)
      .then((prepared) =>
        applyToMirror(getClient().submitDecodedHdri(job.ctx, job.jobId, prepared)),
      )
      .catch((err) =>
        applyToMirror(
          getClient().submitHdriError(
            job.ctx,
            job.jobId,
            err instanceof Error ? err.message : String(err),
          ),
        ),
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
    // The host caches the gizmo's snap steps and orientation; push the current
    // prefs in once, then keep it in step for the rest of the session.
    pushGizmoSettings();
    pushSelectionHighlight();
    pushLabelColors();
    // Boot push (no prev): both pane-seeded display fields apply. This runs
    // before any recovery load, so a restored scene's saved panes still win.
    pushDisplayDefaults();
    usePrefs.subscribe((state, prev) => {
      if (state.prefs.viewport !== prev.prefs.viewport) pushGizmoSettings();
      if (state.prefs.selection !== prev.prefs.selection) pushSelectionHighlight();
      if (state.prefs.display !== prev.prefs.display) pushDisplayDefaults(prev.prefs.display);
      // Body classes flip synchronously in the prefs store before this
      // subscriber runs, so the tokens read fresh.
      if (state.resolvedTheme !== prev.resolvedTheme) pushLabelColors();
    });
    if (import.meta.env.DEV) {
      // Dev-only introspection hook (Chrome-automation verification).
      (window as unknown as Record<string, unknown>).__solarxy = {
        client: c,
        useViewState,
        useMirror,
        useReview,
        useToasts,
        useRadial,
        dispatch,
        // Chrome freezes rAF in a background tab, so the verification harness
        // needs to drive frames itself rather than wait for the render loop.
        runFrame,
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
  // The review mirror re-reads on any annotation/staleness change and on a
  // full document replace (scene load, structural undo).
  if (batch.events.some((e) => e.type === "reviewChanged" || e.type === "documentReplaced")) {
    refreshReview();
  }
}

/** Re-reads the annotation set (with runtime staleness) into the review
 * store. */
export function refreshReview(): void {
  useReview.getState().setAnnotations(getClient().reviewAnnotations());
}

/** Refreshes the stale (dirty) node set into the mirror store. */
export function refreshStale(): void {
  useMirror.getState().setStale(getClient().staleNodes());
}

/** Dispatches a command and mirrors the result. */
export function dispatch(cmd: Command): EventBatch {
  const batch = getClient().dispatch(cmd);
  applyToMirror(batch);
  toastShadowHandoff(cmd, batch);
  refreshStale();
  syncSceneSelection();
  markDirtyAndAutosave();
  return batch;
}

/** Applies a batch produced by a HOST-side interaction (a gizmo drag), through
 * exactly the same tail as `dispatch`: the mirror, the stale set, the scene
 * selection, and the dirty/autosave flag. A gizmo commit must dirty the document
 * like any other edit, or the user could lose it. */
export function applyViewportBatch(batch: EventBatch | null): void {
  if (!batch || batch.events.length === 0) return;
  applyToMirror(batch);
  refreshStale();
  syncSceneSelection();
  markDirtyAndAutosave();
}

/** Selects the viewport tool (Q/W/E/R, or the tool column).
 *
 * Switching mid-drag abandons that drag, and the host rolls it back rather than
 * dropping it, so the returned batch has to reach the mirror. */
export function setTool(tool: ToolMode): void {
  if (!client) return;
  applyViewportBatch(getClient().setTool(tool));
  useViewState.getState().setToolMode(tool);
}

/** Replaces the host's attribute-visualization state (the right strip's
 * toggles and lane pick) and mirrors the returned view state. */
export function setAttrViz(state: AttrVizState): void {
  if (!client) return;
  useViewState.getState().setView(getClient().setAttrViz(state));
}

/** Pushes the gizmo's drag ergonomics into the host. Called on boot and on any
 * prefs change; the drag loop runs in Rust and never crosses back to ask. */
export function pushGizmoSettings(): void {
  if (!client) return;
  getClient().setGizmoSettings(usePrefs.getState().prefs.viewport);
}

/** Pushes the selection-highlight preference into the host.
 * Called on boot and on any prefs change, like the gizmo ergonomics. */
export function pushSelectionHighlight(): void {
  if (!client) return;
  getClient().setSelectionHighlight(usePrefs.getState().prefs.selection);
}

/** Pushes the GPU attribute-label colors from the live theme tokens (the
 * same tokens the DOM overlay used to consume via CSS). Called at boot and
 * whenever the resolved theme flips. */
export function pushLabelColors(): void {
  if (!client) return;
  const tokens = getComputedStyle(document.body);
  const read = (name: string, fallback: string) => tokens.getPropertyValue(name).trim() || fallback;
  getClient().setLabelColors(
    read("--text-primary", "#e6e1cf"),
    read("--background-secondary", "#1f2430"),
    read("--accent-primary", "#ff9e21"),
  );
}

/** Pushes the display defaults (wireframe weight, background, turntable
 * rpm) into the host. At boot both pane-seeded fields apply to every pane
 * (before any scene load, so a restored scene's saved panes still win); a
 * mid-session preference save applies only the fields that changed, so
 * per-pane Display-menu overrides survive unrelated edits. The changed
 * panes come back through a viewChanged host event. */
export function pushDisplayDefaults(prev?: DisplayPrefs): void {
  if (!client) return;
  const d = usePrefs.getState().prefs.display;
  const applyWireframe = prev === undefined || prev.wireframeWeight !== d.wireframeWeight;
  const applyBackground = prev === undefined || prev.background !== d.background;
  getClient().setDisplayDefaults(d, applyWireframe, applyBackground);
  pushLabelDefaults(prev);
}

/** Seeds the session's attribute-label appearance from the saved defaults.
 *
 * The same "defaults seed, the live control overrides" shape wireframe
 * weight and background already have, one level down: these four live in
 * the host's session `AttrVizState` (never saved, never in undo), and the
 * Preferences Display tab is where the value a fresh session starts from
 * lives. A mid-session preference change applies only the fields that
 * actually moved, so it cannot stamp on a gear override of an unrelated
 * one. */
function pushLabelDefaults(prev?: DisplayPrefs): void {
  const d = usePrefs.getState().prefs.display;
  const viz = useViewState.getState().view?.attrViz;
  if (!viz) return;
  const next = { ...viz };
  let moved = false;
  if (prev === undefined || prev.labelSize !== d.labelSize) {
    next.labelSize = d.labelSize;
    moved = true;
  }
  if (prev === undefined || prev.labelBackground !== d.labelBackground) {
    next.labelBackground = d.labelBackground;
    moved = true;
  }
  if (prev === undefined || prev.labelOpacity !== d.labelOpacity) {
    next.labelOpacity = d.labelOpacity;
    moved = true;
  }
  if (prev === undefined || prev.labelDecimals !== d.labelDecimals) {
    next.labelDecimals = d.labelDecimals;
    moved = true;
  }
  if (moved) setAttrViz(next);
}

/** Escape during a gizmo drag: rolls it back. */
export function cancelGizmoDrag(): void {
  if (!client) return;
  applyViewportBatch(getClient().cancelGizmoDrag());
}

/** The exclusive-shadow-caster rule must be self-explanatory at the moment
 * it acts: when granting cast_shadow cascades a release onto
 * another light, toast the name of the light that lost it. */
function toastShadowHandoff(cmd: Command, batch: EventBatch): void {
  if (cmd.type !== "setParam" || cmd.key !== "cast_shadow") return;
  const released = batch.events.filter(
    (e) =>
      e.type === "paramChanged" &&
      e.key === "cast_shadow" &&
      e.node !== cmd.node &&
      e.value.kind === "literal" &&
      e.value.type === "bool" &&
      e.value.value === false,
  );
  if (released.length === 0) return;
  const m = useMirror.getState();
  const registry = m.registry;
  const nameOf = (id: number) => {
    const node = m.contexts["root"]?.nodes.find((n) => n.id === id);
    if (!node) return `light ${id}`;
    return nodeLabel(node, registry ? descriptorFor(registry, node.typeId) : undefined);
  };
  const granted = nameOf(cmd.node);
  const names = released
    .map((e) => (e.type === "paramChanged" ? nameOf(e.node) : ""))
    .filter(Boolean)
    .join(", ");
  pushToast(`${granted} now casts the shadow — ${names} released it`, "info");
}

/** Pushes the root-context selection into the host so the picked object
 * gets its viewport tint. */
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
  // Mirror the returned view state like every other view mutator: a view
  // preset changes the pane's projection, and the Persp/Ortho toolbar label
  // reads the mirror.
  useViewState.getState().setView(getClient().cameraCommand(pane, cmd));
}

/** Binds a pane to look through a camera node (or -1 to clear to free view). */
export function setPaneCamera(pane: number, camera: number): void {
  useViewState.getState().setView(getClient().setPaneCamera(pane, camera));
}

/** Toggles lock-camera-to-view for a look-through pane. */
export function setPaneCameraLock(pane: number, locked: boolean): void {
  useViewState.getState().setView(getClient().setPaneCameraLock(pane, locked));
}

/** Jumps a pane's free view to a camera's saved pose (bookmark). */
export function jumpToCamera(pane: number, camera: number): void {
  useViewState.getState().setView(getClient().jumpToCamera(pane, camera));
}

/** Authors a new camera node framed on the pane's current view, then looks
 * through it (unlocked). The host exposes the pose; the node itself is created
 * and framed with ordinary commands so the mirror stays the source of truth. */
export function createCameraFromView(pane: number): void {
  const pose = getClient().paneCameraPose(pane);
  dispatch({ type: "beginTransaction", label: "Add Camera" });
  const batch = dispatch({ type: "addNode", ctx: "root", nodeType: "camera", position: [0, 0] });
  const added = batch.events.find((e) => e.type === "nodeAdded");
  if (added && added.type === "nodeAdded") {
    const id = added.node.id;
    dispatch({
      type: "setParam",
      ctx: "root",
      node: id,
      key: "position",
      value: { kind: "literal", type: "vec3", value: pose.position },
    });
    dispatch({
      type: "setParam",
      ctx: "root",
      node: id,
      key: "target",
      value: { kind: "literal", type: "vec3", value: pose.target },
    });
    dispatch({ type: "endTransaction" });
    useViewState.getState().setView(getClient().setPaneCamera(pane, id));
  } else {
    dispatch({ type: "endTransaction" });
  }
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

// ---- review actions (host fills author + timestamps; the engine fills
// anchor hashes and inherits reply anchors) ----

/** The annotation author from preferences ("" = anonymous, opt-in). */
function reviewAuthor(): string | undefined {
  const author = usePrefs.getState().prefs.review.author.trim();
  return author.length > 0 ? author : undefined;
}

function anchorFromPick(pick: PickDetail): ReviewAnchor {
  return {
    ctx: "root",
    node: pick.node,
    mesh: pick.mesh,
    face: pick.face,
    barycentric: pick.barycentric,
    worldFallback: pick.worldPos,
  };
}

export function addAnnotation(pick: PickDetail, text: string, category: ReviewCategory): void {
  dispatch({
    type: "addAnnotation",
    anchor: anchorFromPick(pick),
    text,
    category,
    author: reviewAuthor(),
    createdAt: new Date().toISOString(),
  });
}

/** Adds a reply under `parent` (the engine inherits the parent's anchor;
 * the node here is a placeholder the engine ignores). */
export function replyToAnnotation(parent: number, text: string, category: ReviewCategory): void {
  dispatch({
    type: "addAnnotation",
    anchor: { ctx: "root", node: 0 },
    text,
    category,
    author: reviewAuthor(),
    createdAt: new Date().toISOString(),
    replyTo: parent,
  });
}

export function editAnnotation(id: number, text: string, category: ReviewCategory): void {
  dispatch({ type: "editAnnotation", id, text, category, updatedAt: new Date().toISOString() });
}

export function resolveAnnotation(id: number, resolved: boolean): void {
  dispatch({ type: "resolveAnnotation", id, resolved, updatedAt: new Date().toISOString() });
}

export function deleteAnnotation(id: number): void {
  dispatch({ type: "deleteAnnotation", id });
}

export function reanchorAnnotation(id: number, pick: PickDetail): void {
  dispatch({
    type: "reanchorAnnotation",
    id,
    anchor: anchorFromPick(pick),
    updatedAt: new Date().toISOString(),
  });
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

// Autosave: debounced after the last mutation (cadence from preferences,
// default 2s), forced at most every 15s while edits keep arriving.
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let lastSaveAt = 0;

/** The delay before the next autosave write, or null when disabled.
 * Pure so the gating logic is unit-testable. */
export function autosaveDelayMs(
  enabled: boolean,
  debounceSec: number,
  now: number,
  lastSave: number,
): number | null {
  if (!enabled) return null;
  const debounce = Math.max(500, debounceSec * 1000);
  const forceIn = Math.max(0, 15000 - (now - lastSave));
  return Math.min(debounce, forceIn);
}

function markDirtyAndAutosave(): void {
  useMirror.getState().setDirty(true);
  const { enabled, debounceSec } = usePrefs.getState().prefs.autosave;
  if (debounceTimer) clearTimeout(debounceTimer);
  const delay = autosaveDelayMs(enabled, debounceSec, Date.now(), lastSaveAt);
  if (delay === null) return;
  debounceTimer = setTimeout(doAutosave, delay);
}

/** The host `extra` for a `.slxy` save: generator + timestamps. The camera
 * comes from the app itself; canvas viewports and richer metadata are a
 * later refinement. */
/** The host sidecar every save carries (canvas viewports, meta timestamps).
 * Exported so the web-bundle export writes the same shape a normal save
 * does: a published scene must be the scene, not a thinner copy of it. */
export function buildSaveExtra(): SaveExtra {
  const now = new Date().toISOString();
  return {
    generator: `solarxy-web ${__APP_VERSION__}`,
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
  // The node wins: when the document carries an environment node, its
  // cook emits the environment and restoring the scene file's own section
  // as well would race it, with whichever finished last taking the
  // viewport. The section stays the fallback for pre-node documents.
  if (!result.environment.fromNode) {
    void restoreEnvironment(result.environment.hdriHash);
  }
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

/** Opens a bundled sample scene by filename (fetched from the site's
 * `samples/` directory; guarded by a Rust fixture test that cooks every
 * committed sample). Callers own the dirty-check confirm. */
export async function openSampleScene(file: string): Promise<void> {
  try {
    const res = await fetch(`${import.meta.env.BASE_URL}samples/${file}`);
    if (!res.ok) {
      pushToast(`Could not load the sample scene (${res.status}).`, "error");
      return;
    }
    applyLoadedScene(new Uint8Array(await res.arrayBuffer()));
  } catch (e) {
    pushToast(e instanceof Error ? e.message : "Could not load the sample scene.", "error");
  }
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

/** Pastes the clipboard fragment into the current context; context-illegal
 * nodes are skipped with a toast naming the count. */
export function paste(): void {
  if (!clipboard) return;
  const s = useMirror.getState();
  const wanted = Array.isArray((clipboard as { nodes?: unknown[] }).nodes)
    ? (clipboard as { nodes: unknown[] }).nodes.length
    : null;
  const batch = dispatch({
    type: "pasteNodes",
    ctx: s.current,
    fragment: clipboard,
    position: [30, 30],
  });
  if (wanted !== null) {
    const added = batch.events.filter((e) => e.type === "nodeAdded").length;
    const skipped = wanted - added;
    if (skipped > 0) {
      pushToast(
        `${skipped} node(s) skipped: not allowed in this context`,
        "warn",
      );
    }
  }
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

/** Hash-to-filename map for assets staged this session (the engine keeps
 * names too, but does not expose a lookup; assets restored from a .slxy
 * simply miss here and callers fall back to the hash prefix). */
const stagedAssetNames = new Map<string, string>();

/** The original filename of a staged asset, when known this session. */
export function assetDisplayName(hash: string): string | undefined {
  return stagedAssetNames.get(hash);
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
  stagedAssetNames.set(hash, file.name);
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

/** Every staged asset's name from the engine manifest (the sidecar
 * preflight's authoritative staged set; a JS-side cache would go cold on
 * reloads and `.slxy` restores). */
export function stagedManifestNames(): string[] {
  return getClient()
    .assetManifest()
    .map((a) => a.name);
}

/** Stages dropped files and creates the matching import node referencing the
 * primary model. Companion files (mtl/bin/textures) are staged too and
 * resolved by name at parse time, so a multi-file `.gltf` just works. When
 * the primary references companions that are still missing (a lone `.gltf`
 * was dropped), the missing-sidecars dialog opens instead and node creation
 * defers to its completion. */
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
  const primary = files[primaryIdx];
  const staged = await Promise.all(files.map(stageFile));

  const refs = referencedSidecars(primary.name, new Uint8Array(await primary.arrayBuffer()));
  const missing = missingSidecars(refs, stagedManifestNames());
  if (hasMissing(missing)) {
    useUi.getState().setSidecarPrompt({
      primaryName: primary.name,
      primaryHash: staged[primaryIdx].hash,
      missing,
      complete: { kind: "createImportNode" },
    });
    return;
  }
  completeModelImport(staged[primaryIdx].hash, primary.name);
}

/** The import-node creation tail of [`importDroppedFiles`], also run by the
 * missing-sidecars dialog on completion: creates the matching import node
 * (inside a fresh Geo container when at root) and points its `file` param
 * at the staged primary. */
export function completeModelImport(primaryHash: string, primaryName: string): void {
  const nodeType = IMPORT_NODE[extOf(primaryName)];
  if (!nodeType) return;

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
    value: { kind: "literal", type: "asset", value: primaryHash },
  });
  pushToast(`Importing ${primaryName}…`, "info");
}

/** Runs one cook+render frame, mirrors the cook batch, and drains host
 * events (pane-rect changes). In manual mode, refresh the stale set each
 * frame so a just-cooked node drops its badge. */
export function runFrame(dtMs: number): void {
  // The surface must match the canvas BEFORE the frame is cooked and rendered:
  // dockview can re-parent or resize the canvas at any time.
  syncCanvasSize((w, h, dpr) => getClient().resize(w, h, dpr));
  applyToMirror(getClient().frame(dtMs));
  pumpImportJobs();
  for (const ev of getClient().takeHostEvents()) {
    if (ev.type === "paneRects") useViewState.getState().setPaneRects(ev.rects);
    else if (ev.type === "activePane") useViewState.getState().setActivePaneMirror(ev.pane);
    else if (ev.type === "uvOverlap") useViewState.getState().setUvOverlap(ev.pct, ev.pending);
    else if (ev.type === "viewChanged") refreshViewState();
    else if (ev.type === "attrPinStats") useAttrPinStats.getState().set(ev.capacity, ev.total);
  }
  // The gizmo's live delta. Polled here rather than pushed from `pointerMove`,
  // which stays void so the drag keeps costing zero boundary crossings; the
  // frame loop is already crossing anyway for the cook.
  useViewState.getState().setGizmoReadout(getClient().gizmoReadout());

  // Marker pins track the cameras imperatively (no React re-render).
  const review = useReview.getState();
  if (review.annotations.length > 0 && !review.markersHidden) {
    applyMarkerPositions(getClient().reviewMarkers());
  } else {
    hideAllMarkers();
  }
  // Attribute labels draw in the GPU pass now; their sampling stats
  // arrive as attrPinStats host events above, so nothing to pump here.
  // Both modes: manual drives the amber stale tags + header count, auto
  // drives the transient pending tint on queued-dirty nodes.
  refreshStale();
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
