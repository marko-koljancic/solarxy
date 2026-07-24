// The scene tree derivation for the Tree panel: a pure fold over the
// mirror's contexts map (the whole document is already mirrored, root plus
// every subflow), kept separate from the component so the shape and the
// search are unit-testable without a DOM.
//
// Rows keep mirror (insertion) order, matching the canvas and the list
// view; containers recurse into the context their descriptor `opens`
// (the breadcrumb's owner-walk inverted, parent-down).

import { ctxKey } from "../engine/types";
import type {
  ContextKind,
  GraphContext,
  GraphMirror,
  NodeMirror,
  NodeTypeSnapshot,
  RegistrySnapshot,
} from "../engine/types";
import { nodeLabel } from "../flow/nodeLabel";
import { descriptorFor } from "../registry/datatypes";

/** A malformed contexts map (a node id recurring down its own subtree)
 * would recurse forever; real documents are a few levels deep. */
const MAX_DEPTH = 64;

export interface TreeRow {
  /** `${ctxKey}:${nodeId}`, stable across renames. */
  key: string;
  /** The context the node LIVES in (where selection dispatches). */
  ctx: GraphContext;
  node: NodeMirror;
  desc: NodeTypeSnapshot | undefined;
  label: string;
  typeId: string;
  /** The child-network kind, containers only. */
  opens: ContextKind | null;
  /** Whether this node holds its context's display flag. */
  isDisplay: boolean;
  children: TreeRow[];
  depth: number;
}

/** Folds the mirrored contexts into the scene tree, rooted at "root". A
 * container whose sub-context is not (yet) mirrored renders as a leaf. */
export function buildSceneTree(
  registry: RegistrySnapshot | null,
  contexts: Record<string, GraphMirror>,
): TreeRow[] {
  const build = (ctx: GraphContext, graph: GraphMirror | undefined, depth: number): TreeRow[] => {
    if (!graph || depth >= MAX_DEPTH) return [];
    return graph.nodes.map((node) => {
      const desc = descriptorFor(registry, node.typeId);
      const opens = desc?.opens ?? null;
      const children =
        opens === null
          ? []
          : build({ subflow: node.id }, contexts[`sub:${node.id}`], depth + 1);
      return {
        key: `${ctxKey(ctx)}:${node.id}`,
        ctx,
        node,
        desc,
        label: nodeLabel(node, desc),
        typeId: node.typeId,
        opens,
        isDisplay: graph.activeOutput === node.id,
        children,
        depth,
      };
    });
  };
  return build("root", contexts.root, 0);
}

/** Case-insensitive substring search over labels and type ids. Returns the
 * matched row keys plus the ancestor keys to force-expand so every match
 * is reachable. An empty (or whitespace) query returns empty sets, and the
 * caller falls back to manual expansion. */
export function searchTree(
  rows: TreeRow[],
  query: string,
): { matches: Set<string>; expand: Set<string> } {
  const matches = new Set<string>();
  const expand = new Set<string>();
  const q = query.trim().toLowerCase();
  if (!q) return { matches, expand };
  const walk = (row: TreeRow, ancestors: readonly string[]): void => {
    if (row.label.toLowerCase().includes(q) || row.typeId.toLowerCase().includes(q)) {
      matches.add(row.key);
      for (const a of ancestors) expand.add(a);
    }
    const next = [...ancestors, row.key];
    for (const child of row.children) walk(child, next);
  };
  for (const row of rows) walk(row, []);
  return { matches, expand };
}
