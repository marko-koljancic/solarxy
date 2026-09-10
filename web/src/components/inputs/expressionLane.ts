// What is left of the expression lane after the shared rules moved into
// `solarxy-studio`.
//
// PARKED TEXT is the substantial survivor, and it is here for a reason
// worth stating rather than for convenience. An expression switched off
// and switched back on in the same sitting comes back verbatim, which
// means the text has to live somewhere between those two clicks. It does
// not live in the DOCUMENT, because the scene schema is frozen and a
// per-session convenience is not worth a schema version; so it is
// interface memory belonging to whichever shell is holding it, and the
// desktop will keep its own. A rule it is not: nothing about it would
// read differently on another surface.
//
// `paramExpression` stays for the same reason `descriptorFor` does. It
// reads one entry out of a mirror the caller is already holding, and the
// Rust twin `solarxy_studio::expression::param_expression` is what the
// desktop asks instead.
//
// Gone: which types accept an expression (it rides `ParamSnapshot` now,
// derived by the engine from the param type, which is what retired the
// drift test that used to hold a browser array against it), the seed text
// a fresh field opens on, and the readout's rounding.

import {
  ctxKey,
  type GraphContext,
  type NodeId,
  type NodeMirror,
  type ParamSnapshot,
} from "../../engine/types";

/** The expression driving a param, if one is. A literal and an unset
 * param answer the same way: the lane asks whether an expression is in
 * charge, and neither is. */
export function paramExpression(node: NodeMirror, spec: ParamSnapshot): string | null {
  const src = node.params[spec.key];
  return src && src.kind === "expression" ? src.expr : null;
}

const parked = new Map<string, string>();

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

export function clearParkedExpressions(): void {
  parked.clear();
}
