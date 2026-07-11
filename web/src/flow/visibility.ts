// Root visibility affordance helpers (Phase 8): the registry-driven
// predicate (the descriptor declares a `visible` param; the note node gets
// no eye by construction, future root types get one for free) and the
// current value with the default-true fallback (params are override-only,
// so a fresh node carries no entry). Root visibility is distinct from the
// subflow display flag: separate storage, separate command, separate
// icon-in-context.

import type { NodeMirror, NodeTypeSnapshot } from "../engine/types";

/** Whether the descriptor declares a root `visible` param (geo, lights). */
export function hasVisibleParam(desc: NodeTypeSnapshot | undefined): boolean {
  return desc?.params.some((p) => p.key === "visible") ?? false;
}

/** The node's current `visible` value; anything but an explicit literal
 * `false` (including an expression) reads as visible. */
export function nodeVisible(node: NodeMirror): boolean {
  const src = node.params["visible"];
  return !(src && src.kind === "literal" && src.type === "bool" && src.value === false);
}
