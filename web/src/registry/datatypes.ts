// Registry-snapshot-derived helpers: typed-handle presentation (color +
// shape by DataType) and connection legality from the
// coercion matrix. All data-driven, so a new node reusing existing types
// needs zero changes here.

import type {
  CoercionKind,
  DataType,
  NodeTypeSnapshot,
  RegistrySnapshot,
} from "../engine/types";

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
  // A copper hue unused by the twelve
  // existing types; the hexagon groups it with Image as a resource
  // handle (dual encoding still holds: hue differs).
  material: "#c96f4a",
};

/** Handle shape channel for color-blind safety.
 *
 * Among the vectors the shape counts components: a bar is two, a triangle
 * three, a square four. That is what makes the channel legible rather than
 * arbitrary, and it is why `vec4` and `color` share the square -- a color
 * is an RGBA four-vector and the two convert both ways without loss.
 *
 * Until 0.10.0 all three vectors were round, so they were identical on
 * both channels while converting to each other in no direction at all:
 * three handles that looked the same and refused to connect. `image` and
 * `material` shared the hexagon for the same reason and were separated at
 * the same time.
 *
 * `round` is the only shape that makes no claim. Every other one names a
 * type, or a set of types that convert to each other freely. */
export type HandleShape =
  | "round"
  | "diamond"
  | "bar"
  | "triangle"
  | "square"
  | "hexagon"
  | "ring";

export function dataTypeShape(dt: DataType): HandleShape {
  if (dt === "int") return "diamond";
  if (dt === "vec2") return "bar";
  if (dt === "vec3") return "triangle";
  if (dt === "vec4" || dt === "color") return "square";
  if (dt === "image") return "hexagon";
  // A hollow circle: a material port names a shading network rather than
  // carrying pixels. It shared the hexagon with image until 0.10.0, and
  // the two convert in neither direction, so the shared mark said two
  // things to anyone reading shape rather than hue.
  if (dt === "material") return "ring";
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

/** The curated palette/menu order of the node categories (the registry
 * snapshot lists nodes alphabetically by type id, so presentation order
 * is a frontend concern). A category id not listed here sorts after the
 * known ones, alphabetically, so a future Rust category degrades to a
 * sensible spot instead of crashing or scattering. */
export const CATEGORY_ORDER: readonly string[] = [
  "container",
  "generators",
  "attribute",
  "transform",
  "copy",
  "topology",
  "shaders",
  "import",
  "export",
  "lights",
  // Beside Lights, not beside Utility: both are scene elements you place in
  // the root graph and both shape what the render sees.
  "cameras",
  "utility",
  "cop_generate",
  "cop_adjust",
  "cop_composite",
];

/** Comparator over category ids in [`CATEGORY_ORDER`]. */
export function compareCategories(a: string, b: string): number {
  const ia = CATEGORY_ORDER.indexOf(a);
  const ib = CATEGORY_ORDER.indexOf(b);
  if (ia === -1 && ib === -1) return a.localeCompare(b);
  if (ia === -1) return 1;
  if (ib === -1) return -1;
  return ia - ib;
}

/** The param types the parameter panel renders a widget for (a new node
 * using only these needs zero frontend changes; a new ParamType is a
 * deliberate frontend addition). `assetRef` lands with imports;
 * `nodePath` is the cross-context reference picker. */
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
  "nodePath",
  "attributeName",
  "snippet",
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
