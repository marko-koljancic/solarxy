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

export interface Annotation {
  id: number;
  anchor: { ctx: GraphContext; node: NodeId; face?: number | null; barycentric?: [number, number, number] | null };
  text: string;
  category: "info" | "issue" | "question" | "suggestion";
  resolved: boolean;
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

/** Whether two contexts refer to the same graph. */
export function ctxEq(a: GraphContext, b: GraphContext): boolean {
  if (a === "root" || b === "root") return a === b;
  return a.subflow === b.subflow;
}

/** A stable string key for a context (for maps). */
export function ctxKey(ctx: GraphContext): string {
  return ctx === "root" ? "root" : `sub:${ctx.subflow}`;
}
