// Screen -> flow coordinate projection, published imperatively by NodeCanvas.
//
// `NodePalette` needs to turn a pointer position into a graph position, but it
// renders as a SIBLING of the ReactFlowProvider (it must keep working in list
// view, where no ReactFlow exists), so it cannot call `useReactFlow()`.
//
// Rather than restructure the tree, NodeCanvas publishes its projection here
// while it is mounted. Same shape as the marker registry: a module-level ref,
// no React state, so publishing costs no re-render. Null whenever the graph
// view is not mounted, which callers must handle.

export interface XY {
  x: number;
  y: number;
}

type Project = (screen: XY) => XY;

let project: Project | null = null;

/** Called by NodeCanvas on mount; pass null on unmount. */
export function setFlowProjection(fn: Project | null): void {
  project = fn;
}

/**
 * A viewport (client) CSS px point in graph coordinates, or null when the
 * graph view is not mounted (list view, or before the first render).
 */
export function screenToFlow(screen: XY): XY | null {
  return project ? project(screen) : null;
}
