// The expression lane's pure logic, split out from the components so it
// is testable without a renderer (the house convention: every web test is
// pure logic, there is no jsdom).

import type { NodeMirror, ParamSnapshot, ParamValue } from "../../engine/types";

/** The param types an expression may drive.
 *
 * Mirrors `ParamType::accepts_expression` in
 * `crates/solarxy-graph/src/registry/param_spec.rs` (decision M-3), and is
 * held to it by `expression_types_match_the_frontend` in
 * `crates/solarxy-core/tests/tokens_drift.rs`. There is no string type in
 * the expression value lattice, so text, menu, file and node-reference
 * params show no affordance at all. */
export const EXPRESSION_TYPES = [
  "float",
  "int",
  "bool",
  "vec2",
  "vec3",
  "vec4",
  "color",
] as const;

export function acceptsExpression(paramType: string): boolean {
  return (EXPRESSION_TYPES as readonly string[]).includes(paramType);
}

/** The stored expression on a param, if it has one. */
export function paramExpression(
  node: NodeMirror,
  spec: ParamSnapshot,
): string | null {
  const src = node.params[spec.key];
  return src && src.kind === "expression" ? src.expr : null;
}

/** The expression text a freshly opened field starts from: the value the
 * param already had, spelled the way the grammar spells it.
 *
 * Seeding rather than opening blank means the field starts on something
 * that resolves. A blank expression is a parse error, which would badge
 * the node the instant the user clicked `=`. */
export function seedExpression(value: unknown): string {
  const num = (n: unknown) =>
    typeof n === "number" && Number.isFinite(n) ? String(n) : "0";
  if (Array.isArray(value)) return `set(${value.map(num).join(", ")})`;
  // There are no boolean literals in the grammar, so a comparison is how
  // a constant true is spelled.
  if (typeof value === "boolean") return value ? "1 > 0" : "0 > 1";
  return num(value);
}

/** Formats a resolved value for the readout under the field. */
export function formatResolved(v: ParamValue): string {
  const round = (n: number) => {
    if (!Number.isFinite(n)) return String(n);
    // Six places shows a change without turning the readout into noise;
    // trailing zeros are dropped.
    return String(Number(n.toFixed(6)));
  };
  switch (v.type) {
    case "float":
    case "int":
      return round(v.value);
    case "bool":
      return v.value ? "true" : "false";
    case "vec2":
    case "vec3":
    case "vec4":
    case "color":
      return (v.value as number[]).map(round).join(", ");
    default:
      return String((v as { value: unknown }).value);
  }
}
