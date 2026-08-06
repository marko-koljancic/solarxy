// The expression lane's pure logic, split out from the components so it
// is testable without a renderer (the house convention: every web test is
// pure logic, there is no jsdom).

import { ctxKey } from "../../engine/types";
import type {
  GraphContext,
  NodeId,
  NodeMirror,
  ParamSnapshot,
  ParamValue,
} from "../../engine/types";

/** The param types an expression may drive.
 *
 * Mirrors `ParamType::accepts_expression` in
 * `crates/solarxy-graph/src/registry/param_spec.rs`, and is held to it by
 * `expression_types_match_the_frontend` in
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

/** Parked expression text, keyed by the parameter it came off.
 *
 * The `=` affordance is a round trip, not a delete: switching a parameter
 * back to its literal keeps the expression here so switching forward
 * restores exactly what was written, rather than reseeding from the value
 * it happened to resolve to. Only the field's clear control discards.
 *
 * Session scoped, deliberately. The natural home is the parameter itself,
 * but the scene schema is frozen for this release, so parking in the
 * document is not available and the text is lost on reload. Worth
 * revisiting when the schema can move again. */
const parked = new Map<string, string>();

/** The park key. Node ids are only unique within a context, so the context
 * has to be part of it. */
function parkKey(ctx: GraphContext, node: NodeId, paramKey: string): string {
  return `${ctxKey(ctx)} ${node} ${paramKey}`;
}

export function parkExpression(
  ctx: GraphContext,
  node: NodeId,
  paramKey: string,
  expr: string,
): void {
  parked.set(parkKey(ctx, node, paramKey), expr);
}

/** The parked text, or `null` when this parameter has never had one
 * switched off in this session. */
export function parkedExpression(
  ctx: GraphContext,
  node: NodeId,
  paramKey: string,
): string | null {
  return parked.get(parkKey(ctx, node, paramKey)) ?? null;
}

export function discardParkedExpression(
  ctx: GraphContext,
  node: NodeId,
  paramKey: string,
): void {
  parked.delete(parkKey(ctx, node, paramKey));
}

/** Drops every parked expression. Called when a document is loaded.
 *
 * Not defensive tidying: node ids are reused across documents, so without
 * this an unrelated node in the newly loaded scene inherits the previous
 * scene's expression the first time someone clicks its `=`. The frontend
 * mirror already carries one recorded case of state outliving a load and
 * attaching itself to the wrong node; this is the same mistake, and it is
 * not being made twice. */
export function clearParkedExpressions(): void {
  parked.clear();
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
