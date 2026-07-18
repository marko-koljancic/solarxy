// The one node-label rule (rename chain): the per-node `name`
// param when set and non-empty, else the type display name, else the type
// id. Every label surface (FlowNode title, FlowListView cell, parameter
// panel header, breadcrumb) goes through this helper so a rename is
// reflected everywhere at once.

import type { NodeMirror, NodeTypeSnapshot } from "../engine/types";

/**
 * The node's display label. `name` is an override-only param: freshly
 * added nodes carry no entry (the registry default IS the display name),
 * and a cleared or whitespace-only name falls back the same way. An
 * expression-valued name is not resolved here and also falls back.
 */
export function nodeLabel(node: NodeMirror, desc: NodeTypeSnapshot | undefined): string {
  const src = node.params["name"];
  if (src && src.kind === "literal" && src.type === "text") {
    const name = src.value.trim();
    if (name !== "") return name;
  }
  return desc?.displayName ?? node.typeId;
}
