// Registry-snapshot-derived helpers: typed-handle presentation (color +
// shape by DataType, UX spec section 6) and connection legality from the
// coercion matrix. All data-driven, so a new node reusing existing types
// needs zero changes here.

import type { CoercionKind, DataType, NodeTypeSnapshot, RegistrySnapshot } from "../engine/types";

/** Handle color by DataType family (theme-owned tokens). */
export const DATA_TYPE_COLOR: Record<DataType, string> = {
  geometry: "#5aa0ff",
  light: "#ffcc66",
  report: "#4dd0c8",
  float: "#7fd962",
  int: "#7fd962",
  bool: "#ff8a80",
  vec2: "#b39ddb",
  vec3: "#b39ddb",
  vec4: "#b39ddb",
  color: "#f5a623",
  text: "#9aa0a6",
  image: "#e879c8",
};

/** Handle shape channel for color-blind safety. */
export type HandleShape = "round" | "diamond" | "square" | "hexagon";

export function dataTypeShape(dt: DataType): HandleShape {
  if (dt === "int") return "diamond";
  if (dt === "color") return "square";
  if (dt === "image") return "hexagon";
  return "round";
}

/** The descriptor for a node type, if present in the registry. */
export function descriptorFor(
  reg: RegistrySnapshot | null,
  typeId: string,
): NodeTypeSnapshot | undefined {
  return reg?.nodes.find((n) => n.typeId === typeId);
}

/** A port's DataType, resolved from the descriptor (null if unknown). */
export function portDataType(
  reg: RegistrySnapshot | null,
  typeId: string,
  portKey: string,
  dir: "input" | "output",
): DataType | null {
  const desc = descriptorFor(reg, typeId);
  if (!desc) return null;
  const ports = dir === "input" ? desc.inputs : desc.outputs;
  return ports.find((p) => p.key === portKey)?.dataType ?? null;
}

/** The param types the parameter panel renders a widget for (a new node
 * using only these needs zero frontend changes; a new ParamType is a
 * deliberate frontend addition). `assetRef` lands with imports (Phase 5). */
export const SUPPORTED_PARAM_TYPES = [
  "float",
  "int",
  "bool",
  "text",
  "vec2",
  "vec3",
  "vec4",
  "color",
  "enum",
] as const;

export function isSupportedParamType(t: string): boolean {
  return (SUPPORTED_PARAM_TYPES as readonly string[]).includes(t);
}

/** The coercion verdict for a wire (null = forbidden). */
export function coercionKind(
  reg: RegistrySnapshot | null,
  from: DataType,
  to: DataType,
): CoercionKind | null {
  if (!reg) return null;
  return reg.coercions.find((c) => c.from === from && c.to === to)?.kind ?? null;
}

/** Whether a connection between two ports is legal (same or a coercion). */
export function connectionLegal(
  reg: RegistrySnapshot | null,
  fromType: string,
  fromPort: string,
  toType: string,
  toPort: string,
): { legal: boolean; kind: CoercionKind | null } {
  const a = portDataType(reg, fromType, fromPort, "output");
  const b = portDataType(reg, toType, toPort, "input");
  if (!a || !b) return { legal: false, kind: null };
  const kind = coercionKind(reg, a, b);
  return { legal: kind !== null, kind };
}
