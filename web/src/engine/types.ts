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
  | { type: "asset"; value: string };

export type ParamSource =
  | ({ kind: "literal" } & ParamValue)
  | { kind: "expression"; expr: string };

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
  | { type: "nodeStats"; node: NodeId; points: number; prims: number; meshes: number }
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
  | { type: "moveNodes"; ctx: GraphContext; moves: [NodeId, [number, number]][] }
  | { type: "setActiveOutput"; ctx: GraphContext; node: NodeId | null }
  | { type: "setSelection"; ctx: GraphContext; ids: NodeId[] }
  | { type: "setBypass"; ctx: GraphContext; node: NodeId; bypassed: boolean }
  | { type: "reorderVariadicInput"; ctx: GraphContext; node: NodeId; port: string; order: EdgeId[] }
  | { type: "setCookMode"; mode: CookMode }
  | { type: "cookNow" }
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
  | { type: "undo" }
  | { type: "redo" };

export interface PortRef {
  node: NodeId;
  port: string;
}

// --- Registry snapshot (drives palette + parameter panel) ---

export type DataType =
  | "geometry" | "light" | "report" | "float" | "int" | "bool"
  | "vec2" | "vec3" | "vec4" | "color" | "text";

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
  paramType: string;
  enumVariants: [string, string][];
  accept: string[];
  default: unknown;
  hard: [number, number] | null;
  soft: [number, number] | null;
  step: number | null;
  unit: "none" | "degrees" | "meters" | "normalized";
  doc: string;
}

export type BypassSnapshot =
  | { mode: "passThrough"; input: string }
  | { mode: "mute" }
  | { mode: "notBypassable" };

export interface NodeTypeSnapshot {
  typeId: string;
  version: number;
  displayName: string;
  category: "container" | "primitives" | "modifiers" | "import" | "lights" | "utility";
  /** Title Case label for the category; `category` stays the stable id. */
  categoryLabel: string;
  rootContext: boolean;
  subflowContext: boolean;
  inputs: PortSnapshot[];
  outputs: PortSnapshot[];
  params: ParamSnapshot[];
  bypass: BypassSnapshot;
  doc: string;
  searchAliases: string[];
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

/** The host's environment state (the Environment panel's mirror). */
export interface EnvironmentState {
  iblMode: string;
  hdriHash: string | null;
  hdriName: string | null;
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

// --- Phase 6: host-owned view state (panes, cameras, display settings).
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
  lineWeight: string;
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
  display: DisplaySettingsDto;
  paneProjections: ("perspective" | "orthographic")[];
  paneRects: PaneRectDto[];
}

/** Async host happenings drained once per frame. */
export type HostEvent =
  | { type: "paneRects"; rects: PaneRectDto[] }
  | { type: "activePane"; pane: number }
  | { type: "uvOverlap"; pct: number | null; pending: boolean }
  | { type: "viewChanged" };

export type ViewAxis = "top" | "bottom" | "front" | "back" | "left" | "right";

export type CameraCommand =
  | { kind: "fit" }
  | { kind: "view"; axis: ViewAxis }
  | { kind: "projection"; mode: "perspective" | "orthographic" };
