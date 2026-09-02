// Hand-authored TypeScript mirror of the frozen Rust serde boundary shapes
// (solarxy-graph). These are the wasm boundary contract; they are pinned on
// the Rust side by `command_boundary_json_shape_is_camelcase` and exercised
// live. A generated .d.ts via tsify is a documented follow-up; until then
// keep this file in lockstep with the Rust `Command`/`EngineEvent`/snapshot
// definitions (all camelCase).

export type NodeId = number;
export type EdgeId = number;

/** `GraphContext`: `"root"` or `{ subflow: <geo nodeId> }`. */
export type GraphContext = "root" | { subflow: NodeId };

// --- Parameter values (ParamValue is adjacently tagged; ParamSource wraps
// it with a `kind`). For a literal, serde flattens to `{kind:"literal",
// type:<t>, value:<v>}`. ---

export type ParamValue =
  | { type: "float"; value: number }
  | { type: "int"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "text"; value: string }
  | { type: "vec2"; value: [number, number] }
  | { type: "vec3"; value: [number, number, number] }
  | { type: "vec4"; value: [number, number, number, number] }
  | { type: "color"; value: [number, number, number, number] }
  | { type: "enum"; value: string }
  | { type: "asset"; value: string }
  /** A cross-context node reference: the target's stable node id, or
 * null when unset. */
  | { type: "nodeRef"; value: NodeId | null };

export type ParamSource =
  | ({ kind: "literal" } & ParamValue)
  | { kind: "expression"; expr: string };

/** The parameter panel's per-row readout: the current value, or the
 * message explaining why an expression has none. */
export type ResolvedParam =
  | { ok: true; value: ParamValue }
  | { ok: false; error: string };

// --- Mirror types (Rust -> JS, serialize-only) ---

export interface NodeMirror {
  id: NodeId;
  typeId: string;
  typeVersion: number;
  params: Record<string, ParamSource>;
  position: [number, number];
  bypassed: boolean;
}

export interface EdgeMirror {
  id: EdgeId;
  from: NodeId;
  fromPort: string;
  to: NodeId;
  toPort: string;
}

export interface GraphMirror {
  nodes: NodeMirror[];
  edges: EdgeMirror[];
  activeOutput: NodeId | null;
  selection: NodeId[];
}

export interface DocumentSnapshot {
  root: GraphMirror;
  /** keyed by owning geo node id (string). */
  subflows: Record<string, GraphMirror>;
  annotations: Annotation[];
}

export type ReviewCategory = "info" | "warning" | "question" | "change";

/** Where an annotation pins: always a node; 3D pins add mesh/face/bary with
 * a world fallback and the engine-filled staleness hash. */
export interface ReviewAnchor {
  ctx: GraphContext;
  node: NodeId;
  mesh?: number | null;
  face?: number | null;
  barycentric?: [number, number, number] | null;
  worldFallback?: [number, number, number] | null;
  geometryHash?: number | null;
}

/** One annotation as the mirror sees it (`AnnotationSnapshot`: the document
 * annotation flattened together with the runtime staleness flag). */
export interface Annotation {
  id: number;
  anchor: ReviewAnchor;
  text: string;
  category: ReviewCategory;
  resolved: boolean;
  author?: string | null;
  createdAt: string;
  updatedAt: string;
  /** Parent id for replies (flat threading; replies share the parent pin). */
  replyTo?: number | null;
  /** Runtime-derived: the anchored geometry changed; re-place the pin. */
  needsReanchor: boolean;
}

/** A detailed pick (review anchor source), from `pick_detailed`. */
export interface PickDetail {
  node: NodeId;
  mesh: number;
  face: number;
  barycentric: [number, number, number];
  worldPos: [number, number, number];
  distance: number;
  pane: number;
}

/** One marker pin position (PANE-relative CSS px) in one pane, from
 * `review_markers`; structure rides `review_annotations`. */
export interface MarkerScreen {
  id: number;
  pane: number;
  x: number;
  y: number;
}

/** Which element axis an attribute lane describes. */
export type AttrDomain = "point" | "primitive";

/** One named attribute lane in one domain, from `attribute_summary`. */
export interface AttrLane {
  name: string;
  ty: "float" | "vec2" | "vec3" | "vec4";
  len: number;
}

/** The lane inventory of a node's cooked geometry (both domains), from
 * `attribute_summary`; `undefined` while nothing is committed. */
export interface AttributeSummary {
  points: number;
  /** Triangle count (cook-stats convention; lines and points count zero). */
  prims: number;
  /** Primitive-domain element count (topology-aware): the primitive
   * table's row extent. */
  primitiveElements: number;
  meshes: number;
  point: AttrLane[];
  primitive: AttrLane[];
}

/** One value column of the paged attribute table: `P` (positions) leads
 * the point domain, lanes follow in name order. */
export interface AttrColumn {
  key: string;
  ty: AttrLane["ty"];
  components: number;
}

/** One window of attribute rows from `attribute_table`. A row
 * concatenates every column's components; `null` marks a lane missing
 * (or type-conflicted) on that element's mesh. */
export interface AttributePage {
  total: number;
  offset: number;
  columns: AttrColumn[];
  rows: (number | null)[][];
}

/** Host-owned attribute-visualization state (session-only, scene-wide):
 * the right strip's toggles, the picked point-lane name, and the pin
 * budget (0 = the host default). Mirrored through ViewStateDto like the
 * tool mode; never saved, never in undo. */
export interface AttrVizState {
  labels: boolean;
  vectors: boolean;
  points: boolean;
  name: string | null;
  cap: number;
  /** Multiplier on the bounds-derived arrow length; 1.0 is the default. */
  vectorScale: number;
  /** Unit-length directions before scaling. */
  normalize: boolean;
  colorMode: "uniform" | "ramp";
  /** The uniform arrow color, linear RGB 0..1. */
  color: [number, number, number];
  /** Which curated ramp the ramp mode draws. Pinned to
   * `solarxy_web::attr_viz::RampPreset` (serde camelCase). */
  rampPreset: RampPreset;
  /** Label text size. The renderer scales one SDF bake, so these are
   * presets rather than a free number. */
  labelSize: LabelSize;
  /** What a label draws behind its text. `none` is text plus the anchor
   * dot, which is quieter but gives up guaranteed contrast. */
  labelBackground: LabelBackground;
  /** Overall label opacity, 0..1. The chip keeps its own 82% underneath. */
  labelOpacity: number;
  /** Decimal places in a label's value text (0..4). */
  labelDecimals: number;
}

/** The on-demand half of the node info card. Pinned to
 * `solarxy_graph::engine::NodeReport` (serde camelCase).
 *
 * Not part of the mirror on purpose: every field here moves on each cook of
 * a time-dependent node, so mirroring them would put one event per node per
 * frame back on the wire during playback. */
export interface NodeReport {
  /** World bounds as `[minX, minY, minZ, maxX, maxY, maxZ]`, or null for a
   * node with no geometry output. */
  bounds: [number, number, number, number, number, number] | null;
  /** The last cook's wall time. Microseconds, because a fast node reads
   * `0.0 ms` and `340 us` and only one of those tells you anything. */
  lastCookUs: number;
  /** Cooks this session and their summed duration; both reset on load. */
  cookCount: number;
  totalCookUs: number;
  /** Why the node loaded as a non-cooking placeholder, when it did. */
  placeholder: string | null;
  /** Unix milliseconds, or null for a scene saved before 0.8.1. Null means
   * "unknown" and must render as such, never as an epoch date. */
  createdMs: number | null;
  modifiedMs: number | null;
}

export type RampPreset = "coldWarm" | "ember" | "ocean" | "grayscale" | "signal";
export type LabelSize = "small" | "medium" | "large";
export type LabelBackground = "chip" | "none";

/** A screenshot request: capture resolution (physical px) + GPU overlay
 * toggles. */
export interface ScreenshotOpts {
  width: number;
  height: number;
  overlays: { grid: boolean; axes: boolean; validation: boolean };
}

/** A completed capture: tightly-packed RGBA8. */
export interface ScreenshotResult {
  width: number;
  height: number;
  pixels: Uint8Array;
}

// --- Cook status/stats ---

export type CookStatus =
  | { state: "pending" }
  | { state: "cooking" }
  | { state: "ok"; ms: number }
  | { state: "error"; message: string };

export type CookMode = "auto" | "manual";

// --- Runtime / playback ---

export type LoopMode = "once" | "loop" | "pingPong";

/** The persisted half of the scene clock. `playing` and the current frame are
 * session state and deliberately absent: a saved scene always reloads
 * stopped at its range start. */
export interface RuntimeSettings {
  fps: number;
  frameStart: number;
  frameEnd: number;
  loopMode: LoopMode;
  /** Only a published player acts on this; the editor stores and saves it. */
  autoplay: boolean;
}

// --- Events (Rust -> JS) ---

export type EngineEvent =
  | { type: "nodeAdded"; ctx: GraphContext; node: NodeMirror }
  | { type: "nodeRemoved"; ctx: GraphContext; id: NodeId }
  | { type: "paramChanged"; ctx: GraphContext; node: NodeId; key: string; value: ParamSource }
  | { type: "edgeAdded"; ctx: GraphContext; edge: EdgeMirror }
  | { type: "edgeRemoved"; ctx: GraphContext; id: EdgeId }
  | { type: "nodesMoved"; ctx: GraphContext; moves: [NodeId, [number, number]][] }
  | { type: "activeOutputChanged"; ctx: GraphContext; node: NodeId | null }
  | { type: "selectionChanged"; ctx: GraphContext; ids: NodeId[] }
  | { type: "bypassChanged"; ctx: GraphContext; node: NodeId; bypassed: boolean }
  | { type: "variadicReordered"; ctx: GraphContext; node: NodeId; port: string; order: EdgeId[] }
  | { type: "cookStatus"; node: NodeId; status: CookStatus }
  | {
      type: "nodeStats";
      node: NodeId;
      points: number;
      prims: number;
      meshes: number;
      /** `[width, height]` when the node's default output is an image
       * (geometry counts stay zero for those); null otherwise. */
      image: [number, number] | null;
    }
  | { type: "validationSummary"; node: NodeId; errors: number; warnings: number }
  | {
      type: "validationReport";
      node: NodeId;
      errors: number;
      warnings: number;
      truncated: boolean;
      issues: ValidationIssue[];
    }
  | { type: "cookModeChanged"; mode: CookMode }
  | { type: "playbackChanged"; playing: boolean }
  | { type: "frameChanged"; frame: number }
  | { type: "runtimeSettingsChanged"; settings: RuntimeSettings }
  // The node a gizmo drag will write to. Emitted on BOTH policy paths, because
  // the reuse path mints nothing and so carries no nodeAdded to read an id from.
  | { type: "transformTargetReady"; ctx: GraphContext; node: NodeId }
  | { type: "reviewChanged" }
  | { type: "documentReplaced" };

export interface EventBatch {
  revision: number;
  events: EngineEvent[];
}

// --- Commands (JS -> Rust) ---

export type Command =
  | { type: "addNode"; ctx: GraphContext; nodeType: string; position: [number, number] }
  | { type: "removeNodes"; ctx: GraphContext; ids: NodeId[] }
  | { type: "connect"; ctx: GraphContext; from: PortRef; to: PortRef }
  | { type: "disconnect"; ctx: GraphContext; edge: EdgeId }
  | { type: "setParam"; ctx: GraphContext; node: NodeId; key: string; value: ParamSource }
  // Removes stored overrides so the node falls back to descriptor
  // defaults: every param when keys is absent, else only the listed ones.
  // One undo step.
  | { type: "resetParams"; ctx: GraphContext; node: NodeId; keys?: string[] }
  | { type: "moveNodes"; ctx: GraphContext; moves: [NodeId, [number, number]][] }
  | { type: "setActiveOutput"; ctx: GraphContext; node: NodeId | null }
  | { type: "setSelection"; ctx: GraphContext; ids: NodeId[] }
  | { type: "setBypass"; ctx: GraphContext; node: NodeId; bypassed: boolean }
  | { type: "reorderVariadicInput"; ctx: GraphContext; node: NodeId; port: string; order: EdgeId[] }
  | { type: "setCookMode"; mode: CookMode }
  | { type: "cookNow" }
  | { type: "play" }
  | { type: "pause" }
  | { type: "stop" }
  | { type: "stepFrame"; delta: number }
  | { type: "setFrame"; frame: number }
  | { type: "setFrameRange"; start: number; end: number }
  | { type: "setFps"; fps: number }
  | { type: "setLoopMode"; mode: LoopMode }
  | { type: "setAutoplay"; autoplay: boolean }
  | { type: "pasteNodes"; ctx: GraphContext; fragment: unknown; position: [number, number] }
  | { type: "duplicateNodes"; ctx: GraphContext; ids: NodeId[] }
  | {
      type: "addAnnotation";
      anchor: ReviewAnchor;
      text: string;
      category: ReviewCategory;
      author?: string;
      createdAt: string;
      replyTo?: number;
    }
  | { type: "editAnnotation"; id: number; text: string; category: ReviewCategory; updatedAt: string }
  | { type: "resolveAnnotation"; id: number; resolved: boolean; updatedAt: string }
  | { type: "deleteAnnotation"; id: number }
  | { type: "reanchorAnnotation"; id: number; anchor: ReviewAnchor; updatedAt: string }
  | { type: "beginTransaction"; label: string }
  | { type: "endTransaction" }
  // Resolves (creating if needed) the node a gizmo drag writes to, inside the
  // geo's subflow. Issued inside the drag's transaction, so an appended
  // transform undoes together with the move.
  | { type: "ensureTransformTarget"; geo: NodeId }
  // Escape mid-drag: rolls the open transaction back and discards it, leaving
  // no document mutation AND no redo entry.
  | { type: "cancelTransaction" }
  | { type: "undo" }
  | { type: "redo" };

export interface PortRef {
  node: NodeId;
  port: string;
}

// --- Registry snapshot (drives palette + parameter panel) ---

export type DataType =
  | "geometry" | "light" | "report" | "float" | "int" | "bool"
  | "vec2" | "vec3" | "vec4" | "color" | "text" | "image" | "material";

export interface PortSnapshot {
  key: string;
  label: string;
  dataType: DataType;
  variadic: boolean;
  required: boolean;
  min: number;
  isDefault: boolean;
  doc: string;
}

export interface ParamSnapshot {
  key: string;
  label: string;
  group: string;
  /** A labelled division inside the group, rendered as a heading above the
   * run of params that share it. A group is a tab and the tab strip is a
   * single non-wrapping row, so a node with many related families needs a
   * level below the tab rather than a tab each. Absent on params that
   * declare none. */
  subgroup?: string;
  paramType: string;
  enumVariants: [string, string][];
  accept: string[];
  /** The picker constraint for `nodePath` params; absent otherwise. */
  nodePath?: { kind: "opens"; opens: ContextKind } | { kind: "typeIs"; typeIs: string };
  default: unknown;
  hard: [number, number] | null;
  soft: [number, number] | null;
  step: number | null;
  unit: "none" | "degrees" | "meters" | "normalized";
  /** Input-port key whose connection neutralizes this param (the panel
   * dims the row while that port is connected). */
  drivenByPort?: string | null;
  /** Conditional-visibility clauses (ANDed); absent means always visible.
   * Predicate values use the same plain encoding as `default`. */
  showIf?: ShowIfClause[];
  doc: string;
}

export interface ShowIfClause {
  param: string;
  pred: ShowIfPred;
}

export type ShowIfPred =
  | { kind: "truthy" }
  | { kind: "eq"; value: unknown }
  | { kind: "neq"; value: unknown }
  | { kind: "in"; values: unknown[] };

export type BypassSnapshot =
  | { mode: "passThrough"; input: string }
  | { mode: "mute" }
  | { mode: "notBypassable" };

/** The visual silhouette family a node renders with;
 * orthogonal to `category` (which picks the fill). A pure UI hint. */
export type NodeRole =
  | "standard"
  | "container"
  | "gather"
  | "branch"
  | "terminal"
  | "analyzer"
  | "imageSource"
  | "light"
  | "camera"
  | "text"
  | "note";

/** The network kinds of the typed-context model. The root
 * canvas is `obj`; a container's child canvas is whatever its descriptor
 * `opens`. */
export type ContextKind = "obj" | "geo" | "mat" | "tex";

export interface NodeTypeSnapshot {
  typeId: string;
  version: number;
  displayName: string;
  category:
    | "container"
    | "generators"
    | "attribute"
    | "transform"
    | "copy"
    | "topology"
    | "shaders"
    | "import"
    | "export"
    | "lights"
    | "cameras"
    | "utility"
    | "tex_generate"
    | "tex_adjust"
    | "tex_composite";
  /** Title Case label for the category; `category` stays the stable id. */
  categoryLabel: string;
  /** The network kinds this node may be placed in. Replaces the older
   * rootContext/subflowContext booleans, which could only describe two
   * kinds; the palette filters against the current canvas's kind. */
  contexts: ContextKind[];
  /** The child-network kind this node opens, for containers (`geo` opens
   * `"geo"`); null otherwise. A canvas's kind derives from its owner's
   * descriptor through this. */
  opens: ContextKind | null;
  inputs: PortSnapshot[];
  outputs: PortSnapshot[];
  params: ParamSnapshot[];
  bypass: BypassSnapshot;
  doc: string;
  searchAliases: string[];
  /** Stable icon key; an unknown key falls back to the
   * category glyph in `flow/nodeVisual.ts`. */
  glyph: string;
  /** Silhouette hint; a variant this frontend does not know
   * yet falls back by category in `flow/nodeVisual.ts`. */
  role: NodeRole;
}

export type CoercionKind = "same" | "lossless" | "lossy";

export interface CoercionEntry {
  from: DataType;
  to: DataType;
  kind: CoercionKind;
}

export interface RegistrySnapshot {
  nodes: NodeTypeSnapshot[];
  coercions: CoercionEntry[];
}

/** The per-import finishing options (camelCase; mirrors Rust ImportOptions).
 * A format-specific toggle is null for formats that do not declare it. */
export interface ImportOptions {
  scale: number;
  centerToOrigin: boolean;
  recomputeNormals: boolean | null;
  preserveMaterials: boolean | null;
  vertexColors: boolean | null;
}

/** A staged asset reference: its content hash and original file name. */
export interface AssetRef {
  hash: string;
  name: string;
}

/** One import parse job drained from the engine, to run in the worker. */
export interface ImportJob {
  ctx: GraphContext;
  jobId: number;
  hash: string;
  name: string;
  format: string;
  options: ImportOptions;
  /** Candidate companion files (mtl, bin, textures) the resolver matches by name. */
  sidecars: AssetRef[];
}

/** One image-decode job drained from the engine (`import_image`, Phase
 * 13): the frontend pulls the encoded bytes by hash and the worker
 * decodes them via `createImageBitmap`. */
export interface ImageJob {
  ctx: GraphContext;
  jobId: number;
  hash: string;
  name: string;
}

/** A drained HDRI-decode job. Same shape as an ImageJob; it routes to the
 * worker's HDRI entry point instead, because no browser codec reads
 * Radiance or OpenEXR. */
export interface HdriJob {
  ctx: GraphContext;
  jobId: number;
  hash: string;
  name: string;
}

/** The host's environment state (the Environment panel's mirror). */
export interface EnvironmentState {
  iblMode: string;
  hdriHash: string | null;
  hdriName: string | null;
  /** Whether the document holds an environment node. When it does, the
   * node is authoritative and the scene file's own environment section is
   * not restored: the section is the fallback for pre-node documents. */
  fromNode: boolean;
}

/** One stashed geometry-validation job for the worker (the validate node
 * above its inline triangle threshold). `blob` is the geometry transfer
 * blob; `config` a JSON `ValidationConfig`. */
export interface ValidateJob {
  ctx: GraphContext;
  jobId: number;
  blob: Uint8Array;
  config: string;
  budget?: number;
}

/** Validation issue severity (core `Severity`, camelCase). */
export type ValidationSeverity = "error" | "warning";

/** Where a validation issue lives (core `IssueScope`, externally tagged).
 * Mesh/face/edge indices are raw-model indices. */
export type ValidationScope =
  | "model"
  | { mesh: number }
  | { material: number }
  | { face: [number, number] }
  | { edge: { meshIndex: number; vertices: [number, number] } };

/** One row of a validation report (core `ValidationIssue`). `kind` is the
 * camelCase issue kind (e.g. "degenerateTriangles", "missingUvs"). */
export interface ValidationIssue {
  severity: ValidationSeverity;
  scope: ValidationScope;
  kind: string;
  message: string;
}

/** One context's canvas pan/zoom. */
export interface CanvasViewport {
  x: number;
  y: number;
  zoom: number;
}

/** Document metadata carried in a .slxy. */
export interface SceneMeta {
  name: string;
  description: string;
  projectId: string;
  created: string;
  modified: string;
}

/** The result of loading a .slxy: the replace batch plus the view state the
 * frontend restores (canvas viewports, metadata). The camera is applied
 * Rust-side. */
export interface SlxyLoadResult {
  batch: EventBatch;
  warnings: string[];
  canvasViewports: Record<string, CanvasViewport>;
  meta: SceneMeta;
  environment: EnvironmentState;
}

/** The host `extra` passed to save_slxy (camera comes from the app itself). */
export interface SaveExtra {
  generator: string;
  canvasViewports: Record<string, CanvasViewport>;
  meta: SceneMeta;
}

/** Whether two contexts refer to the same graph. */
export function ctxEq(a: GraphContext, b: GraphContext): boolean {
  if (a === "root" || b === "root") return a === b;
  return a.subflow === b.subflow;
}

/** A stable string key for a context (for maps). */
export function ctxKey(ctx: GraphContext): string {
  return ctx === "root" ? "root" : `sub:${ctx.subflow}`;
}

// --- Host-owned view state (panes, cameras, display settings).
// PaneDisplaySettings mirrors solarxy_core::view_config with camelCase
// fields; enum VALUES keep their Rust casing ("Shaded", "GRADIENT"-style
// BackgroundMode strings) because those serde shapes predate the boundary.

export type ViewLayout =
  | "single"
  | "splitVertical"
  | "splitHorizontal"
  | "quad"
  | "threeLeftBig";

export interface PaneDisplaySettings {
  viewMode: "Shaded" | "ShadedWireframe" | "WireframeOnly" | "Ghosted";
  prevNonGhostedMode: string;
  ghostedWireframe: boolean;
  normalsMode: "Off" | "Face" | "Vertex" | "FaceAndVertex";
  backgroundMode: unknown;
  uvMode: "Off" | "Gradient" | "Checker";
  boundsMode: "off" | "wholeModel" | "perMesh";
  /** Wireframe stroke weight in screen px: Light 1, Medium 2, Bold 3.
   * Pinned to `solarxy_core::preferences::LineWeight`. */
  lineWeight: "Light" | "Medium" | "Bold";
  showGrid: boolean;
  showAxisGizmo: boolean;
  showLocalAxes: boolean;
  inspectionMode: "Shaded" | "MaterialId" | "TexelDensity" | "Depth" | "Overdraw" | "AoPreview";
  materialOverride: "None" | "Clay" | "ClayDark" | "Chrome" | "Silhouette";
  texelDensityTarget: number;
  paneMode: "Scene3D" | "UvMap";
  uvBg: string;
  uvOffset: [number, number];
  uvZoom: number;
  showUvOverlap: boolean;
  showValidation: boolean;
  /** Live per-pane turntable spin; session-temporary, not persisted. */
  turntableActive: boolean;
  /** Which backend draws this pane's 3D content. A traced pane is still a
   * 3D pane (same navigation, picking and toolbar semantics); only the
   * encode differs. Pinned to `solarxy_core::view_config::PaneEngine`. */
  paneEngine: "raster" | "traced";
}

/** A free pane's own rendering intent, pinned to
 * `solarxy_core::view_config::PaneLook`.
 *
 * The scalar half of the look only. Lookup-table slots live on the camera
 * node, because a table is a staged document asset and a pane is not a
 * document object; a pane looking through a camera composites with that
 * camera's look instead of this one. */
export interface PaneLook {
  exposure: number;
  toneMode: "None" | "Linear" | "Reinhard" | "AcesFilmic";
  /** Added after tone mapping; neutral [0, 0, 0]. */
  lift: [number, number, number];
  /** Applied as a power; neutral [1, 1, 1]. */
  gamma: [number, number, number];
  /** Multiplied first; neutral [1, 1, 1]. */
  gain: [number, number, number];
}

export interface DisplaySettingsDto {
  turntableActive: boolean;
  turntableRpm: number;
  lightsLocked: boolean;
  layout: ViewLayout;
  splitRatio: number;
  roughnessScale: number;
  metallicScale: number;
  hdriRotation: number;
  hdriIntensity: number;
}

export interface PaneRectDto {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ViewStateDto {
  layout: ViewLayout;
  splitRatio: number;
  activePane: number;
  camerasLinked: boolean;
  paneSettings: PaneDisplaySettings[];
  /** Each pane's own look, used when the pane is a free view. */
  paneLooks: PaneLook[];
  display: DisplaySettingsDto;
  paneProjections: ("perspective" | "orthographic")[];
  paneRects: PaneRectDto[];
  /** The camera node each pane looks through (id), or null for a free view. */
  paneLookThrough: (number | null)[];
  /** Whether each look-through pane is locked (reframes the camera). */
  paneCameraLock: boolean[];
  /** The look-through camera's framing aspect per pane (for the gate), or null. */
  paneGateAspect: (number | null)[];
  /** The host-owned attribute-visualization state (the right strip). */
  attrViz: AttrVizState;
}

/** The current pose of a pane's camera (create-camera-from-view). */
export interface CameraPose {
  position: [number, number, number];
  target: [number, number, number];
}

/** Async host happenings drained once per frame. */
export type HostEvent =
  | { type: "paneRects"; rects: PaneRectDto[] }
  | { type: "activePane"; pane: number }
  | { type: "uvOverlap"; pct: number | null; pending: boolean }
  | { type: "viewChanged" }
  | { type: "attrPinStats"; capacity: number; total: number }
  | {
      type: "renderProgress";
      tile: number;
      tiles: number;
      sample: number;
      samples: number;
      done: boolean;
      /** How long the render has taken, in milliseconds. */
      elapsedMs: number;
      /** How much longer, or null while there is not enough to say: the first
       * chunks have no rate to extrapolate from, and after the last one the job
       * is still assembling. A confident wrong number is worse than a blank. */
      remainingMs: number | null;
    }
  | { type: "renderNotice"; message: string }
  | { type: "paneSamples"; pane: number; samples: number; target: number }
  | {
      type: "gpuFault";
      kind: "validation" | "outOfMemory" | "internal";
      message: string;
      count: number;
    };

/** What one render backend can do, pinned to the Rust `BackendCaps`
 * constants. `progressive` is what drives a sample counter: repeated
 * frames of an unchanged pane keep improving the image. */
export interface BackendCaps {
  progressive: boolean;
  supportsInstancing: boolean;
  writesAovs: boolean;
}

/** Both backends' capabilities, for menu gating. */
export interface BackendCapsSet {
  raster: BackendCaps;
  traced: BackendCaps;
}

/** One rectangle of a still render, as it crosses the boundary.
 *
 * Tiles cross one at a time and the frontend assembles them, which is what
 * keeps a 67-megapixel image out of the wasm heap and gives the modal its
 * live preview for free.
 *
 * The picture-so-far crosses in this same shape, from `takeStillPreview`, so
 * the modal paints both through one call. The difference is what they mean
 * rather than what they carry: a tile is the render's output and a preview is
 * an unfinished look at one, which is why only tiles reach the saved file. */
export interface StillTileDto {
  x: number;
  y: number;
  width: number;
  height: number;
  /** Always four bytes a pixel, whatever the render's own depth. A float
   * still's tiles are clamped and sRGB-encoded on the Rust side before they
   * cross, because the canvas is eight bits by construction and the floats
   * have no business here except inside the encoded file. */
  pixels: Uint8Array;
}

/** A pass a render window can show.
 *
 * The beauty is in the list because it is what a selector defaults to, and it
 * is not an auxiliary pass: it is the picture, and every render has one. */
export type StillPass = "beauty" | "albedo" | "normal" | "depth";

/** What the running render produces, and what its engine could produce.
 *
 * Two answers rather than one, because a selector says different things with
 * them: a pass nobody asked for is offered and disabled, naming the parameter
 * that would produce it, while a render whose engine writes none shows the
 * beauty alone, since no checkbox anywhere would have helped.
 *
 * `engineWritesAovs` is a capability, resolved from the backend's own constant.
 * Nothing here asks which engine is running. */
export interface StillPasses {
  albedo: boolean;
  normal: boolean;
  depth: boolean;
  engineWritesAovs: boolean;
}

/** What a still is saved as. Chosen before the render starts, because the
 * format decides what the tiles are and cannot change once they arrive. */
export type StillFormat = "png" | "exr";

/** Which floats a float still carries. The same two the headless command
 * offers, with the same default, so there is one vocabulary for the idea.
 *
 * `sceneLinear` is light with no exposure, tone map or grade applied, which is
 * what a compositing package expects to be handed. `display` is the finished
 * look without the quantization. Meaningless for a PNG, and ignored there. */
export type StillSpace = "sceneLinear" | "display";

/** What a `render` node is asking for, resolved by the engine.
 *
 * Read out of the engine rather than assembled here. The rule for turning
 * authored parameters into a render lived on this side until 0.9.0, which is
 * how the node's two bounce budgets came to be authored and read by nothing:
 * this shape had no field for them. A render is now asked for by naming the
 * node, and this travels one way, out, for the dialog to show. */
export interface RenderSettings {
  width: number;
  height: number;
  samples: number;
  engine: "raster" | "pathTraced";
  bounces: number;
  transmissiveBounces: number;
  denoise: boolean;
  /** The `camera` node to shoot through, or null for the active pane's view. */
  camera: number | null;
  /** The auxiliary passes the run writes beside the image.
   *
   * The production half of the pass model, and a property of the render node
   * rather than of the window: what a render makes is authored in the document
   * and what a window shows is chosen in the window. */
  aovAlbedo: boolean;
  aovNormal: boolean;
  aovDepth: boolean;
  /** Whether the render carries a matte. The window reads it for two things
   * the picture cannot say about itself: the checker shows only behind a
   * render that actually has transparency, and the eight-bit save routes
   * through the engine's own encoder, whose straight alpha a canvas round
   * trip would corrupt. */
  transparentBackground: boolean;
}

/** The viewport tool. Rotate and Scale select, draw and
 * grab nothing, which is why their buttons ship disabled rather than dead. */
export type ToolMode = "select" | "move" | "rotate" | "scale";

export type ViewAxis = "top" | "bottom" | "front" | "back" | "left" | "right";

export type CameraCommand =
  | { kind: "fit" }
  | { kind: "view"; axis: ViewAxis }
  | { kind: "projection"; mode: "perspective" | "orthographic" };
