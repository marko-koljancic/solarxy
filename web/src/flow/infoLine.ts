// The per-node one-line info text (Phase 7b C9): a muted line under the
// node label summarizing its key parameter(s). A pure interpreter of the
// registry snapshot + node params; new node types get a sensible line (or
// none) with zero frontend changes.

import type { NodeMirror, NodeTypeSnapshot, ParamSnapshot } from "../engine/types";

/** Well-known dimension keys, in display priority order, with the short
 * prefix shown before the value. */
const DIMENSION_ABBREV: [string, string][] = [
  ["size", "s"],
  ["width", "w"],
  ["height", "h"],
  ["depth", "d"],
  ["radius", "r"],
  ["radiusTop", "rt"],
  ["radiusBottom", "rb"],
  ["innerRadius", "ri"],
  ["outerRadius", "ro"],
];

/** Effective param value: explicit literal, else the registry default
 * (freshly added nodes carry a sparse params record); expressions yield
 * nothing (the line would mislead). */
function effectiveValue(node: NodeMirror, spec: ParamSnapshot): unknown {
  const src = node.params[spec.key];
  if (src) {
    if (src.kind !== "literal") return undefined;
    return (src as { value: unknown }).value;
  }
  return spec.default;
}

/** Trims a float for the info line: at most 3 decimals, no trailing zeros. */
export function fmtNumber(v: number): string {
  if (!Number.isFinite(v)) return String(v);
  return String(Number(v.toFixed(3)));
}

/**
 * The one-line node summary. Heuristic per category:
 * lights show intensity; primitives show their dimension params; imports
 * show the staged asset (name via the lookup, hash prefix otherwise);
 * everything else shows its first float/int/enum param, preferring params
 * outside the General group.
 */
export function nodeInfoLine(
  desc: NodeTypeSnapshot | undefined,
  node: NodeMirror,
  assetName?: (hash: string) => string | undefined,
): string | null {
  if (!desc) return null;
  const value = (spec: ParamSnapshot) => effectiveValue(node, spec);
  const byKey = (key: string) => desc.params.find((p) => p.key === key);

  if (desc.category === "lights") {
    const spec = byKey("intensity");
    const v = spec ? value(spec) : undefined;
    if (typeof v === "number") return `intensity ${fmtNumber(v)}`;
  }

  if (desc.category === "import") {
    const spec = desc.params.find((p) => p.paramType === "assetRef");
    if (spec) {
      const v = value(spec);
      const hash = typeof v === "string" ? v : "";
      if (!hash) return "no file";
      return assetName?.(hash) ?? `${hash.slice(0, 10)}…`;
    }
  }

  if (desc.category === "primitives") {
    const parts: string[] = [];
    for (const [key, abbrev] of DIMENSION_ABBREV) {
      const spec = desc.params.find(
        (p) => p.key === key && (p.paramType === "float" || p.paramType === "int"),
      );
      if (!spec) continue;
      const v = value(spec);
      if (typeof v === "number") parts.push(`${abbrev} ${fmtNumber(v)}`);
      if (parts.length === 3) break;
    }
    if (parts.length > 0) return parts.join("  ");
  }

  // Fallback: the first float/int/enum param, preferring non-general groups
  // (the general group tends to hold housekeeping like names and toggles).
  const candidates = desc.params.filter(
    (p) => p.paramType === "float" || p.paramType === "int" || p.paramType === "enum",
  );
  const pick = candidates.find((p) => p.group.toLowerCase() !== "general") ?? candidates[0];
  if (!pick) return null;
  const v = value(pick);
  if (pick.paramType === "enum" && typeof v === "string") {
    return pick.enumVariants.find(([key]) => key === v)?.[1] ?? v;
  }
  if (typeof v === "number") {
    return `${pick.label.toLowerCase()} ${fmtNumber(v)}`;
  }
  return null;
}
