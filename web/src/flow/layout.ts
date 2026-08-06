// Auto-layout over the current context (Minimystix parity): dagre
// (layered, top-to-bottom) and ELK (layered/DOWN) both wired; `L` cycles
// between them, the View menu invokes each explicitly. Positions are
// measured from the live DOM (React Flow data-id nodes) and the result
// lands as ONE moveNodes command, so a layout is a single undo step.
// Note nodes are excluded (they are annotations, not flow).

import dagre from "@dagrejs/dagre";
import { dispatch } from "../engine/session";
import type { GraphContext, GraphMirror } from "../engine/types";
import { selectGraph, useMirror } from "../store/mirror";
import { NODE_BOX } from "./nodeVisual";

export type LayoutAlgorithm = "dagre" | "elk";

/** An unmeasured node (not yet in the DOM) is assumed to be the layout
 * box. This is a FALLBACK, not a floor: a measured size is used as-is, so
 * a role's real box drives the layout instead of a phantom minimum. */
function nodeDims(id: number): { width: number; height: number } {
  const el = document.querySelector(`[data-id="${id}"]`);
  if (!(el instanceof HTMLElement)) return { width: NODE_BOX.w, height: NODE_BOX.h };
  // offsetWidth/offsetHeight are layout pixels, untouched by the canvas
  // zoom transform; getBoundingClientRect() shrinks with zoom-out, which
  // made the result depend on how far out the user happened to be.
  return {
    width: el.offsetWidth || NODE_BOX.w,
    height: el.offsetHeight || NODE_BOX.h,
  };
}

export interface LayoutNode {
  id: number;
  width: number;
  height: number;
}

export type Measure = (id: number) => { width: number; height: number };

/** Layoutable nodes (notes excluded) with measured dims + the edges among
 * them. Pure given a measure function; the DOM measurer is the default. */
export function layoutInputs(
  graph: GraphMirror,
  measure: Measure = nodeDims,
): { nodes: LayoutNode[]; edges: [number, number][] } {
  const nodes = graph.nodes
    .filter((n) => n.typeId !== "note")
    .map((n) => ({ id: n.id, ...measure(n.id) }));
  const ids = new Set(nodes.map((n) => n.id));
  const edges = graph.edges
    .filter((e) => ids.has(e.from) && ids.has(e.to))
    .map((e) => [e.from, e.to] as [number, number]);
  return { nodes, edges };
}

/** Dagre position mapping (rankdir TB, Minimystix options). Pure: returns
 * React Flow top-left moves (dagre yields centers). */
export function computeDagreLayout(
  nodes: LayoutNode[],
  edges: [number, number][],
): [number, [number, number]][] {
  const g = new dagre.graphlib.Graph();
  // Spacing retuned for the real 112x32 box (the old values were tuned
  // against 120x60 phantom floors, whose padding the constants now carry
  // explicitly): nodesep 58 keeps the old 8px-padded sibling distance and
  // clears the label stack's start; ranksep 128 keeps the old rank
  // distance (100 between 60px-tall phantoms) so wires clear the handle
  // overhang and the gather dome.
  g.setGraph({ rankdir: "TB", align: "UL", nodesep: 58, edgesep: 10, ranksep: 128 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const n of nodes) g.setNode(String(n.id), { width: n.width, height: n.height });
  for (const [from, to] of edges) g.setEdge(String(from), String(to));
  dagre.layout(g);
  return nodes.map((n) => {
    const p = g.node(String(n.id));
    return [n.id, [p.x - n.width / 2, p.y - n.height / 2]];
  });
}

/** ELK position mapping (layered/DOWN, Minimystix options). Pure/async;
 * ELK already yields top-left coordinates.
 *
 * ELK is loaded on FIRST USE, not at module load. `elk.bundled.js` is ~1.6 MB
 * minified -- about two thirds of what the entry chunk used to be -- and it is
 * only reachable from here: dagre is the default algorithm and ELK is behind the
 * `L` cycle and an explicit View-menu item. A static import made every visitor
 * download a layout engine most of them never invoke. */
export async function computeElkLayout(
  nodes: LayoutNode[],
  edges: [number, number][],
): Promise<[number, [number, number]][]> {
  const { default: ELK } = await import("elkjs/lib/elk.bundled.js");
  const elk = new ELK();
  const result = await elk.layout({
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "DOWN",
      // Retuned like the dagre constants above: +8 sibling / +28 layer
      // spacing absorbs the padding the 120x60 phantom floors used to add.
      "elk.spacing.nodeNode": "88",
      "elk.layered.spacing.nodeNodeBetweenLayers": "128",
      "elk.layered.spacing.edgeNodeBetweenLayers": "50",
      "elk.layered.nodePlacement.strategy": "SIMPLE",
    },
    children: nodes.map((n) => ({ id: String(n.id), width: n.width, height: n.height })),
    edges: edges.map(([from, to], i) => ({
      id: `e${i}`,
      sources: [String(from)],
      targets: [String(to)],
    })),
  });
  const moves: [number, [number, number]][] = [];
  for (const child of result.children ?? []) {
    moves.push([Number(child.id), [child.x ?? 0, child.y ?? 0]]);
  }
  return moves;
}

function applyPositions(ctx: GraphContext, moves: [number, [number, number]][]): void {
  if (moves.length === 0) return;
  dispatch({ type: "moveNodes", ctx, moves });
}

/** Dagre: synchronous layered layout, dispatched as one moveNodes. */
export function applyDagreLayout(ctx: GraphContext, graph: GraphMirror): void {
  const { nodes, edges } = layoutInputs(graph);
  if (nodes.length === 0) return;
  applyPositions(ctx, computeDagreLayout(nodes, edges));
}

/** ELK: layered/DOWN (async), dispatched as one moveNodes. */
export async function applyElkLayout(ctx: GraphContext, graph: GraphMirror): Promise<void> {
  const { nodes, edges } = layoutInputs(graph);
  if (nodes.length === 0) return;
  applyPositions(ctx, await computeElkLayout(nodes, edges));
}

/** Menu entry point: lays out the current context and fits the view when
 * done (the canvas listens for the fitView event). No-op on empty graphs. */
export function runLayout(algo: LayoutAlgorithm): void {
  const current = useMirror.getState().current;
  const g = selectGraph(useMirror.getState(), current);
  if (g.nodes.length === 0) return;
  const done = () => window.dispatchEvent(new Event("solarxy:fitView"));
  if (algo === "dagre") {
    applyDagreLayout(current, g);
    done();
  } else {
    void applyElkLayout(current, g).then(done);
  }
}

// The L-key cycle alternates algorithms per press (Minimystix behavior).
let cycle: LayoutAlgorithm = "dagre";

/** Runs the next algorithm in the cycle; returns which one ran. */
export async function cycleAutoLayout(
  ctx: GraphContext,
  graph: GraphMirror,
): Promise<LayoutAlgorithm> {
  const algo = cycle;
  cycle = cycle === "dagre" ? "elk" : "dagre";
  if (algo === "dagre") applyDagreLayout(ctx, graph);
  else await applyElkLayout(ctx, graph);
  return algo;
}
