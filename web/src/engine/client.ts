// The wasm boundary client: loads the wasm module, constructs the
// `SolarxyApp`, and exposes typed wrappers over its `any`-typed methods.
// The React app holds exactly one instance.

import init, { SolarxyApp, start } from "../wasm/pkg/solarxy_web.js";
import wasmUrl from "../wasm/pkg/solarxy_web_bg.wasm?url";
import type {
  CameraCommand,
  Command,
  DisplaySettingsDto,
  DocumentSnapshot,
  EnvironmentState,
  EventBatch,
  GraphContext,
  HostEvent,
  ImportJob,
  NodeId,
  PaneDisplaySettings,
  PaneRectDto,
  ParamSource,
  RegistrySnapshot,
  SaveExtra,
  SlxyLoadResult,
  ValidateJob,
  ViewLayout,
  ViewStateDto,
} from "./types";

export class SolarxyClient {
  private constructor(private readonly app: SolarxyApp) {}

  /** Loads the wasm, installs the panic hook, and boots over a canvas.
   * `uvChecker` is the checker texture PNG the renderer's UV modes use. */
  static async create(canvas: HTMLCanvasElement, uvChecker: Uint8Array): Promise<SolarxyClient> {
    await init({ module_or_path: wasmUrl });
    start(); // panic hook -> console.error (idempotent)
    const app = await SolarxyApp.create(canvas, uvChecker);
    return new SolarxyClient(app);
  }

  /** Applies one command; returns the event batch for the mirror. */
  dispatch(cmd: Command): EventBatch {
    return this.app.dispatch(cmd) as EventBatch;
  }

  /** Cooks + renders one frame; returns the cook event batch. */
  frame(dtMs: number): EventBatch {
    return this.app.frame(dtMs) as EventBatch;
  }

  previewParam(ctx: GraphContext, node: NodeId, key: string, value: ParamSource): void {
    this.app.preview_param(ctx, node, key, value);
  }

  /** Rust-side pane-aware pick: canvas CSS pixel -> producing geo node id. */
  pick(x: number, y: number): NodeId | undefined {
    return this.app.pick(x, y);
  }

  /** Marks the picked node's scene object as selected (viewport tint). */
  setSceneSelection(node: NodeId | undefined): void {
    this.app.set_scene_selection(node);
  }

  snapshot(): DocumentSnapshot {
    return this.app.snapshot() as DocumentSnapshot;
  }

  registrySnapshot(): RegistrySnapshot {
    return this.app.registry_snapshot() as RegistrySnapshot;
  }

  copyNodes(ctx: GraphContext, ids: NodeId[]): unknown {
    return this.app.copy_nodes(ctx, new Float64Array(ids));
  }

  resize(width: number, height: number): void {
    this.app.resize(width, height);
  }

  // ---- pointer routing (CSS px relative to the canvas) ----

  pointerDown(x: number, y: number, button: number): void {
    this.app.pointer_down(x, y, button);
  }

  pointerMove(x: number, y: number): void {
    this.app.pointer_move(x, y);
  }

  pointerUp(button: number): void {
    this.app.pointer_up(button);
  }

  /** Wheel zoom on the hovered pane; positive zooms in. */
  wheel(delta: number): void {
    this.app.wheel(delta);
  }

  // ---- host-owned view state ----

  viewState(): ViewStateDto {
    return this.app.view_state() as ViewStateDto;
  }

  setViewLayout(layout: ViewLayout): ViewStateDto {
    return this.app.set_view_layout(layout) as ViewStateDto;
  }

  setSplitRatio(ratio: number): ViewStateDto {
    return this.app.set_split_ratio(ratio) as ViewStateDto;
  }

  setActivePane(pane: number): ViewStateDto {
    return this.app.set_active_pane(pane) as ViewStateDto;
  }

  setPaneSettings(pane: number, settings: PaneDisplaySettings): ViewStateDto {
    return this.app.set_pane_settings(pane, settings) as ViewStateDto;
  }

  setDisplaySettings(settings: DisplaySettingsDto): ViewStateDto {
    return this.app.set_display_settings(settings) as ViewStateDto;
  }

  cameraCommand(pane: number, cmd: CameraCommand): void {
    this.app.camera_command(pane, cmd);
  }

  paneRects(): PaneRectDto[] {
    return this.app.pane_rects() as PaneRectDto[];
  }

  /** Mirrors the node canvas's current graph context to the host. */
  setCurrentContext(ctx: GraphContext): void {
    this.app.set_current_context(ctx);
  }

  takeHostEvents(): HostEvent[] {
    return this.app.take_host_events() as HostEvent[];
  }

  /** The ids of currently stale (dirty) nodes. */
  staleNodes(): number[] {
    return Array.from(this.app.stale_nodes() as Float64Array);
  }

  nodeTypeCount(): number {
    return this.app.node_type_count();
  }

  // ---- asset staging + the import-worker pump ----

  /** Stages asset bytes; returns the content id the import node references. */
  stageAsset(name: string, mime: string, sha256: string, bytes: Uint8Array): string {
    return this.app.stage_asset(name, mime, sha256, bytes) as string;
  }

  /** A fresh copy of the staged bytes for a hash (undefined if absent). */
  assetBytes(hash: string): Uint8Array | undefined {
    return this.app.asset_bytes(hash) as Uint8Array | undefined;
  }

  /** Drains the import jobs the last cook spawned (to run in the worker).
   * Also stashes any validate jobs for `takeValidateJobs`. */
  takeImportJobs(): ImportJob[] {
    return this.app.take_import_jobs() as ImportJob[];
  }

  /** Drains the stashed geometry-validation jobs (call after
   * `takeImportJobs`, which performs the engine drain). */
  takeValidateJobs(): ValidateJob[] {
    return this.app.take_validate_jobs() as ValidateJob[];
  }

  /** Commits a worker-parsed model (transfer blob + implicit validation
   * JSON) under the generation guard. */
  submitParsedModel(
    ctx: GraphContext,
    jobId: number,
    blob: Uint8Array,
    validationJson?: string,
  ): EventBatch {
    return this.app.submit_parsed_model(ctx, jobId, blob, validationJson) as EventBatch;
  }

  /** Reports a worker parse failure (the node badges the error). */
  submitParseError(ctx: GraphContext, jobId: number, message: string): EventBatch {
    return this.app.submit_parse_error(ctx, jobId, message) as EventBatch;
  }

  /** Commits a worker validation result (JSON `ValidationResult`). */
  submitValidationResult(ctx: GraphContext, jobId: number, resultJson: string): EventBatch {
    return this.app.submit_validation_result(ctx, jobId, resultJson) as EventBatch;
  }

  /** Reports a worker validation failure (the node badges the error). */
  submitValidationError(ctx: GraphContext, jobId: number, message: string): EventBatch {
    return this.app.submit_validation_error(ctx, jobId, message) as EventBatch;
  }

  /** Flies the active pane's camera to a validation issue's mesh and
   * enables that pane's validation overlay. `object` is the owning geo
   * node (= scene object id); `source` the node whose report is shown;
   * `issue` the report row index. */
  flyToIssue(object: NodeId, source: NodeId, issue: number): ViewStateDto {
    return this.app.fly_to_issue(object, source, issue) as ViewStateDto;
  }

  // ---- environment / HDRI ----

  /** Installs a worker-prepared HDRI (GPU finish + light rebind). */
  setEnvironmentPrepared(hash: string, name: string, prepared: Uint8Array): void {
    this.app.set_environment_prepared(hash, name, prepared);
  }

  /** Clears the HDRI back to the procedural sky. */
  clearEnvironment(): void {
    this.app.clear_environment();
  }

  /** Sets the IBL contribution mode ("off" | "diffuse" | "full"). */
  setIblMode(mode: string): void {
    this.app.set_ibl_mode(mode);
  }

  /** The current environment (IBL mode + loaded HDRI identity). */
  environmentState(): EnvironmentState {
    return this.app.environment_state() as EnvironmentState;
  }

  // ---- .slxy save / load ----

  /** Builds .slxy archive bytes from the document + assets + host extra. */
  saveSlxy(extra: SaveExtra): Uint8Array {
    return this.app.save_slxy(extra) as Uint8Array;
  }

  /** Replaces the document from .slxy bytes; returns batch + view state. */
  loadSlxy(bytes: Uint8Array): SlxyLoadResult {
    return this.app.load_slxy(bytes) as SlxyLoadResult;
  }
}
