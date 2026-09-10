// What is left of the registry helpers after the shared rules moved into
// `solarxy-studio`.
//
// The PRESENTATION of a data type is gone: the wire colour, the handle
// shape that is its second channel, and the category order the palette
// draws in. Those were three tables restating rules the engine also holds,
// and the colour table had drifted into hex literals under a docstring
// calling them theme-owned tokens. They arrive on the presentation tables
// now, read once at boot beside the registry snapshot.
//
// Four things stay, in two groups, and each is named rather than left to
// look like an oversight.
//
// `descriptorFor` and `portDataType` are index helpers over a snapshot the
// caller is already holding. Crossing the WebAssembly boundary to look up
// an array entry would be a trade nobody would make twice.
//
// `coercionKind` and `connectionLegal` are the same shape one level up.
// The matrix is the ENGINE's: `reg.coercions` arrives on the registry
// snapshot, and "legal" is "the matrix named a kind". What is here is the
// lookup, not a second opinion about which conversions exist. The Rust
// twin is `solarxy_studio::types::connection_verdict`, which the desktop
// reads; the browser does not call it because `connectionLegal` runs on
// every hover of a connection drag, and crossing per hover to re-derive
// two port types the caller already holds is not worth buying.
//
// `compareCategories` is the third of that kind: the order is the
// engine's, on the presentation tables, and what is left here is the
// lookup plus what to do with a category this build has not heard of.
//
// `SUPPORTED_PARAM_TYPES` is the one thing here that is genuinely this
// shell's own: which param types this panel draws a widget for. The engine
// has no opinion about that, and the desktop's answer is its own.

import type {
  CoercionKind,
  DataType,
  NodeTypeSnapshot,
  PresentationTables,
  RegistrySnapshot,
} from "../engine/types";

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

/** The param types this panel draws a widget for.
 *
 * A fact about THIS shell, not about the engine: a node using only these
 * needs zero frontend changes, and a new `ParamType` is a deliberate
 * frontend addition. It was three entries stale until 0.10.0 -- the panel
 * had rendered `action`, `assetRef` and `multilineText` for releases while
 * this list said otherwise -- because its only reader was a test over a
 * fabricated node whose params all happened to be listed.
 * `every_declared_param_type_has_a_widget` now reads the real registry. */
export const SUPPORTED_PARAM_TYPES = [
  "float",
  "int",
  "bool",
  "text",
  "multilineText",
  "vec2",
  "vec3",
  "vec4",
  "color",
  "enum",
  "assetRef",
  "action",
  "nodePath",
  "attributeName",
  "snippet",
] as const;

export function isSupportedParamType(t: string): boolean {
  return (SUPPORTED_PARAM_TYPES as readonly string[]).includes(t);
}

/** Orders two category ids for the palette and the Add menu.
 *
 * The order is the ENGINE's: `categories[id].order` arrives on the
 * presentation tables, and it is the `Category` declaration order. The
 * browser kept a curated list of its own until 0.10.0 and it had always
 * been the same sequence, so it was never a second opinion. What is here
 * is the lookup and the degradation: a category this build has not heard
 * of sorts after the known ones, alphabetically, rather than scattering.
 */
export function compareCategories(
  tables: PresentationTables | null,
  a: string,
  b: string,
): number {
  const ia = tables?.categories[a]?.order;
  const ib = tables?.categories[b]?.order;
  if (ia === undefined && ib === undefined) return a.localeCompare(b);
  if (ia === undefined) return 1;
  if (ib === undefined) return -1;
  return ia - ib;
}

/** The coercion verdict for a wire (null = forbidden), read off the
 * engine's own matrix. */
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
