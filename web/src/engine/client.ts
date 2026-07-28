// The wasm boundary client: loads the wasm module, constructs the
// `SolarxyApp`, and exposes typed wrappers over its `any`-typed methods.
// The React app holds exactly one instance.

import init, { SolarxyApp, start } from "../wasm/pkg/solarxy_web.js";
import wasmUrl from "../wasm/pkg/solarxy_web_bg.wasm?url";
import type { DisplayPrefs, GizmoPrefs, SelectionPrefs } from "../store/prefs";
import type {
  Annotation,
  AssetRef,
  AttrDomain,
  AttrVizState,
  AttributePage,
  AttributeSummary,
  CameraCommand,
  CameraPose,
  Command,
  DisplaySettingsDto,
  DocumentSnapshot,
  EnvironmentState,
  EventBatch,
  GraphContext,
  HostEvent,
  ImageJob,
  ImportJob,
  MarkerScreen,
  NodeId,
  PaneDisplaySettings,
  PaneRectDto,
  ParamSource,
  PickDetail,
  RegistrySnapshot,
  ResolvedParam,
  ScreenshotOpts,
  ScreenshotResult,
  SaveExtra,
  SlxyLoadResult,
  ValidateJob,
  ToolMode,
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

  /** `pick` with the full hit detail (mesh/face/barycentric/world/pane):
   * the anchor source for creating and re-placing review annotations. */
  pickDetailed(x: number, y: number): PickDetail | undefined {
    return (this.app.pick_detailed(x, y) ?? undefined) as PickDetail | undefined;
  }

  /** The annotation set with runtime staleness; re-read on `reviewChanged`. */
  reviewAnnotations(): Annotation[] {
    return this.app.review_annotations() as Annotation[];
  }

  /** Marker pin positions (canvas CSS px, per visible marker x 3D pane);
   * polled once per animation frame while markers are shown. */
  reviewMarkers(): MarkerScreen[] {
    return this.app.review_markers() as MarkerScreen[];
  }

  /** The lane inventory of a node's cooked geometry (both domains);
   * undefined while nothing is committed. Fetched on demand (picker open,
   * pane refresh), never polled. */
  attributeSummary(node: NodeId): AttributeSummary | undefined {
    return (this.app.attribute_summary(node) ?? undefined) as AttributeSummary | undefined;
  }

  /** The last completed cook's warnings for one node (empty when quiet);
   * fetched when the info card opens or the cook status changes. */
  cookWarnings(node: NodeId): string[] {
    return this.app.cook_warnings(node) as string[];
  }

  /** One window of a node's cooked attribute values; only the page
   * crosses the boundary. */
  attributeTable(
    node: NodeId,
    domain: AttrDomain,
    offset: number,
    limit: number,
  ): AttributePage | undefined {
    return (this.app.attribute_table(node, domain, offset, limit) ?? undefined) as
      | AttributePage
      | undefined;
  }

  /** Replaces the host-owned attribute-visualization state; returns the
   * refreshed view state (the mutator convention). */
  setAttrViz(state: AttrVizState): ViewStateDto {
    return this.app.set_attr_viz(state) as ViewStateDto;
  }

  /** Pushes the GPU attribute-label theme colors (CSS hex tokens in,
   * linear RGB across the boundary, the selection-highlight convention):
   * text, background chip, anchor dot. */
  setLabelColors(textHex: string, chipHex: string, dotHex: string): void {
    const [tr, tg, tb] = hexToLinearRgb(textHex);
    const [cr, cg, cb] = hexToLinearRgb(chipHex);
    const [dr, dg, db] = hexToLinearRgb(dotHex);
    this.app.set_label_colors(tr, tg, tb, cr, cg, cb, dr, dg, db);
  }

  /** Requests an active-pane screenshot (rendered at frame end). */
  requestScreenshot(opts: ScreenshotOpts): void {
    this.app.request_screenshot(opts);
  }

  /** Requests one turntable-export frame: `pane` rendered through its
   * render-through camera rotated by `azimuthDeg`. Uses the same capture slot
   * as the screenshot; poll with `pollScreenshot`. */
  requestTurntableFrame(pane: number, azimuthDeg: number, opts: ScreenshotOpts): void {
    this.app.request_turntable_frame(pane, azimuthDeg, opts);
  }

  /** Polls the in-flight capture; undefined while pending. */
  pollScreenshot(): ScreenshotResult | undefined {
    return (this.app.poll_screenshot() ?? undefined) as ScreenshotResult | undefined;
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

  /** Resizes the surface. `dpr` is the live device pixel ratio (browser zoom
   * and monitor changes move it), which the host scales every pointer
   * coordinate and pane rect by. */
  resize(width: number, height: number, dpr: number): void {
    this.app.resize(width, height, dpr);
  }

  // ---- pointer routing (CSS px relative to the canvas) ----

  /** Selects the viewport tool. Returns a rollback batch when the switch
   * abandoned a live drag, else null. */
  setTool(tool: ToolMode): EventBatch | null {
    return this.app.set_tool(tool) as EventBatch | null;
  }

  /** The gizmo's drag ergonomics, pushed from the prefs store. Pushed rather
   * than polled: the drag loop never crosses back into JS to ask. */
  setGizmoSettings(s: GizmoPrefs): void {
    this.app.set_gizmo_settings(
      s.orientation,
      s.snapTranslate,
      s.snapRotate,
      s.snapScale,
    );
  }

  /** The selection-highlight preference. The hex color is
   * sRGB; the rim draws into an sRGB swapchain view, so the shader wants
   * linear components (the hardware re-encodes on write). */
  setSelectionHighlight(s: SelectionPrefs): void {
    const [r, g, b] = hexToLinearRgb(s.color);
    this.app.set_selection_highlight(s.style, r, g, b, 1.0, s.width);
  }

  /** The display defaults preference (wireframe weight, background,
   * turntable rpm). The apply flags say which pane-seeded fields should
   * repaint every pane now: both at boot, only the changed ones on a
   * mid-session preference save (so per-pane Display-menu overrides
   * survive unrelated edits). */
  setDisplayDefaults(d: DisplayPrefs, applyWireframe: boolean, applyBackground: boolean): void {
    this.app.set_display_defaults(
      d.wireframeWeight,
      d.background,
      d.turntableRpm,
      d.pointSize,
      applyWireframe,
      applyBackground,
    );
  }

  /** Enters player mode: no manipulator, no picking, no review markers, and
   * the layout locked to a single pane. Set BEFORE loading a scene so no
   * frame is ever drawn with editing chrome on it. */
  setPlayerMode(on: boolean): void {
    this.app.set_player_mode(on);
  }

  /** The clock's current frame. Polled, not pushed: see `gizmoReadout`. */
  clockFrame(): number {
    return this.app.clock_frame();
  }

  /** Whether the clock is running. Polled, not tracked: a `once` range stops
   * itself at the end, so a caller's own boolean would go stale. */
  clockPlaying(): boolean {
    return this.app.clock_playing();
  }

  /** Whether the loaded document asks to start playing. A document setting,
   * not an export one, so it means the same thing in the editor (which
   * stores it and does not act on it) and in a player (which does). */
  autoplay(): boolean {
    return this.app.autoplay();
  }

  /** The displayed image of a texture network, or null when
   * it publishes nothing. Pull-based; the viewer fetches on cook changes,
   * so cooked pixels never ride the event stream. */
  texturePreview(
    owner: number,
  ): { width: number; height: number; pixels: Uint8ClampedArray } | null {
    return (this.app.texture_preview(owner) ?? null) as {
      width: number;
      height: number;
      pixels: Uint8ClampedArray;
    } | null;
  }

  /** One param's value as the panel displays it, or why it has none.
   *
   * Pulled per row rather than pushed: under playback a resolved value
   * pushed per cook would be one event per expression per frame. */
  resolvedParam(ctx: GraphContext, node: number, key: string): ResolvedParam {
    return this.app.resolved_param(ctx, node, key) as ResolvedParam;
  }

  /** Executes an export node's Action param; the returned
   * bytes go to the save path. Throws with the engine's message when the
   * action cannot run (nothing cooked, unsupported). */
  invokeAction(
    ctx: GraphContext,
    node: number,
    key: string,
  ): { filename: string; mime: string; bytes: Uint8Array } {
    return this.app.invoke_action(ctx, node, key) as {
      filename: string;
      mime: string;
      bytes: Uint8Array;
    };
  }

  /** The live drag's delta text ("X +1.250 m"), or null when nothing is
   * dragging. POLLED once per frame, so `pointerMove` can stay void. */
  gizmoReadout(): string | null {
    return this.app.gizmo_readout() ?? null;
  }

  /** Escape during a gizmo drag: rolls it back. Returns the rollback batch, or
   * null when no drag was in flight. */
  cancelGizmoDrag(): EventBatch | null {
    return this.app.cancel_gizmo_drag() as EventBatch | null;
  }

  /** Returns an event batch when the press STARTED a gizmo drag that mutated
   * the document (the append path mints a transform node), else null. */
  pointerDown(x: number, y: number, button: number): EventBatch | null {
    return this.app.pointer_down(x, y, button) as EventBatch | null;
  }

  /** Void on purpose: this is the hot path, and a live gizmo drag streams
   * straight into the engine's preview lane without crossing back into JS.
   *
   * `mods` is a bitfield (bit 0 = snap / Ctrl), not a bool, so shift-for-
   * precision can land later without changing the wasm signature. */
  pointerMove(x: number, y: number, mods: number): void {
    this.app.pointer_move(x, y, mods);
  }

  /** Returns the commit batch when it ended a gizmo drag, else null. */
  pointerUp(button: number): EventBatch | null {
    return this.app.pointer_up(button) as EventBatch | null;
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

  cameraCommand(pane: number, cmd: CameraCommand): ViewStateDto {
    return this.app.camera_command(pane, cmd) as ViewStateDto;
  }

  /** Binds a pane to look through a camera node, or -1 to clear to free view. */
  setPaneCamera(pane: number, camera: number): ViewStateDto {
    return this.app.set_pane_camera(pane, camera) as ViewStateDto;
  }

  setPaneCameraLock(pane: number, locked: boolean): ViewStateDto {
    return this.app.set_pane_camera_lock(pane, locked) as ViewStateDto;
  }

  jumpToCamera(pane: number, camera: number): ViewStateDto {
    return this.app.jump_to_camera(pane, camera) as ViewStateDto;
  }

  paneCameraPose(pane: number): CameraPose {
    return this.app.pane_camera_pose(pane) as CameraPose;
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

  /** Every staged asset as `{hash, name}` (the sidecar preflight's
   * authoritative staged-name source). */
  assetManifest(): AssetRef[] {
    return this.app.asset_manifest() as AssetRef[];
  }

  /** Opens (or replaces) the live model preview on a canvas. */
  previewOpen(canvas: HTMLCanvasElement, hash: string, name: string): void {
    this.app.preview_open(canvas, hash, name);
  }

  /** Opens the live model preview from a geometry blob the import worker
   * parsed off the main thread, so a large model no longer hitches the UI on
   * open. */
  previewOpenParsed(canvas: HTMLCanvasElement, blob: Uint8Array): void {
    this.app.preview_open_parsed(canvas, blob);
  }

  previewOrbit(dx: number, dy: number): void {
    this.app.preview_orbit(dx, dy);
  }

  previewZoom(delta: number): void {
    this.app.preview_zoom(delta);
  }

  previewResize(width: number, height: number): void {
    this.app.preview_resize(width, height);
  }

  previewClose(): void {
    this.app.preview_close();
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

  /** Drains the stashed image-decode jobs (call after `takeImportJobs`,
   * which performs the engine drain). */
  takeImageJobs(): ImageJob[] {
    return this.app.take_image_jobs() as ImageJob[];
  }

  /** Commits a worker-decoded image (raw RGBA8 + dimensions) under the
   * generation guard; the content hash is stamped engine-side. */
  submitDecodedImage(
    ctx: GraphContext,
    jobId: number,
    width: number,
    height: number,
    pixels: Uint8Array,
  ): EventBatch {
    return this.app.submit_decoded_image(ctx, jobId, width, height, pixels) as EventBatch;
  }

  /** Reports a worker image-decode failure (the node badges the error). */
  submitImageError(ctx: GraphContext, jobId: number, message: string): EventBatch {
    return this.app.submit_image_error(ctx, jobId, message) as EventBatch;
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

/** Parses "#rrggbb" and converts each sRGB channel to linear. */
function hexToLinearRgb(hex: string): [number, number, number] {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  const n = m ? parseInt(m[1], 16) : 0xff9e21;
  const toLinear = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return [toLinear((n >> 16) & 0xff), toLinear((n >> 8) & 0xff), toLinear(n & 0xff)];
}
