// The node editor canvas: a React Flow view of the current context's graph,
// rewired to the mirror-and-command model. React Flow gestures dispatch
// Commands; the mirror updates and re-seeds the local RF state. React never
// owns document truth - it is a display mirror.

import { useCallback, useRef, useEffect, useMemo } from "react";
import {
  Background,
  ConnectionLineType,
  Controls,
  MiniMap,
  ReactFlow,
  addEdge,
  useEdgesState,
  useNodesState,
  type Connection,
  type Edge,
  type Node,
  type NodeChange,
  type EdgeChange,
  useReactFlow,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { dispatch } from "../engine/session";
import type { GraphContext, GraphMirror, RegistrySnapshot } from "../engine/types";
import { portDataType, connectionLegal, descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { pushToast } from "../store/toasts";
import { usePrefs } from "../store/prefs";
import { useUi, type EdgeStyle } from "../store/ui";
import { useViewState } from "../store/viewState";
import { FlowNode, type FlowNodeData } from "./FlowNode";
import { setFlowProjection } from "./flowProjection";
import { NoteNode } from "./NoteNode";
import { RadialMenu } from "./RadialMenu";

const NODE_TYPES = { solarxy: FlowNode, note: NoteNode };

/** The background dot-grid pitch; snap-to-grid locks drags to the
 * same lattice so the two stay one concept. */
const GRID_GAP = 18;

/** Minimap tint: each node carries its category pastel so the
 * overview map reads as a scaled-down graph, not uniform blocks. */
function minimapNodeColor(registry: RegistrySnapshot | null) {
  return (n: Node): string => {
    if (n.type === "note") return "var(--background-tertiary)";
    const data = n.data as FlowNodeData | undefined;
    const desc = data ? descriptorFor(registry, data.node.typeId) : undefined;
    return desc ? `var(--node-cat-${desc.category})` : "var(--geometry-node-background)";
  };
}

/** Connection styles map onto xyflow's built-in edge types, so the typed
 * color classes (edge-type-*, edge-lossy) apply to every style unchanged.
 * The same ids drive the drag-preview connectionLineType. */
const RF_EDGE_TYPE: Record<EdgeStyle, ConnectionLineType> = {
  bezier: ConnectionLineType.Bezier,
  straight: ConnectionLineType.Straight,
  simpleBezier: ConnectionLineType.SimpleBezier,
  smoothStep: ConnectionLineType.SmoothStep,
};

/** Maps a mirror graph to React Flow nodes/edges, styling each edge by its
 * coercion kind (plain for same/lossless, warning for lossy). */
function toRf(
  graph: GraphMirror,
  registry: RegistrySnapshot | null,
  edgeStyle: EdgeStyle,
): { nodes: Node<FlowNodeData>[]; edges: Edge[] } {
  const nodes = graph.nodes.map((n) => ({
    id: String(n.id),
    // The note node is the one bespoke component (on-canvas sticky);
    // everything else renders through the generic registry interpreter.
    type: n.typeId === "note" ? "note" : "solarxy",
    position: { x: n.position[0], y: n.position[1] },
    selected: graph.selection.includes(n.id),
    data: { node: n, isDisplay: graph.activeOutput === n.id },
  }));
  const edges = graph.edges.map((e) => {
    const from = graph.nodes.find((n) => n.id === e.from);
    const to = graph.nodes.find((n) => n.id === e.to);
    // Houdini-style typed wires: the stroke carries the SOURCE port's
    // data-type color (the same palette as the handles); a lossy coercion
    // keeps its dashed overlay on top of the type color.
    const classes: string[] = [];
    if (from) {
      const dt = portDataType(registry, from.typeId, e.fromPort, "output");
      if (dt) classes.push(`edge-type-${dt}`);
    }
    if (from && to) {
      const { kind } = connectionLegal(registry, from.typeId, e.fromPort, to.typeId, e.toPort);
      if (kind === "lossy") classes.push("edge-lossy");
      else if (kind === "lossless") classes.push("edge-coerced");
    }
    return {
      id: String(e.id),
      source: String(e.from),
      target: String(e.to),
      sourceHandle: e.fromPort,
      targetHandle: e.toPort,
      type: RF_EDGE_TYPE[edgeStyle],
      className: classes.join(" "),
      reconnectable: true,
    };
  });
  return { nodes, edges };
}

export function NodeCanvas() {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const resolvedTheme = usePrefs((s) => s.resolvedTheme);
  const showFlowGrid = useUi((s) => s.showFlowGrid);
  const showMinimap = useUi((s) => s.showMinimap);
  const showFlowControls = useUi((s) => s.showFlowControls);
  const snapToGrid = useUi((s) => s.snapToGrid);
  const edgeStyle = useUi((s) => s.edgeStyle);
  const { fitView, screenToFlowPosition } = useReactFlow();

  // Auto-layout completion fits the view (the layout module emits this
  // after its moveNodes command lands).
  useEffect(() => {
    const onFit = () => void fitView({ padding: 0.2, duration: 300 });
    window.addEventListener("solarxy:fitView", onFit);
    return () => window.removeEventListener("solarxy:fitView", onFit);
  }, [fitView]);

  // Publish the projection for NodePalette, which renders outside this
  // provider (it must survive list view) and so cannot call useReactFlow.
  // Nulled on unmount, so a palette opened in list view falls back instead of
  // projecting through a stale transform.
  useEffect(() => {
    setFlowProjection((p) => screenToFlowPosition(p));
    return () => setFlowProjection(null);
  }, [screenToFlowPosition]);

  const seed = useMemo(() => toRf(graph, registry, edgeStyle), [graph, registry, edgeStyle]);
  const [nodes, setNodes, onNodesChange] = useNodesState(seed.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(seed.edges);

  // Re-seed local RF state whenever the mirror graph (or context) changes.
  // Structural/param edits and undo change the graph reference; cook-only
  // batches do not, so this does not fire every frame or clobber a drag.
  useEffect(() => {
    setNodes(seed.nodes);
    setEdges(seed.edges);
  }, [seed, setNodes, setEdges]);

  const ctx: GraphContext = current;

  // React Flow node changes -> local apply + selective commands.
  const handleNodesChange = useCallback(
    (changes: NodeChange<Node<FlowNodeData>>[]) => {
      onNodesChange(changes);
      const removed = changes.filter((c) => c.type === "remove").map((c) => Number(c.id));
      if (removed.length) dispatch({ type: "removeNodes", ctx, ids: removed });
      const selected = changes.filter(
        (c): c is Extract<NodeChange, { type: "select" }> => c.type === "select",
      );
      if (selected.length) {
        // Compute the new selection set from the current nodes + changes.
        const sel = new Set<number>(
          nodes.filter((n) => n.selected).map((n) => Number(n.id)),
        );
        for (const c of selected) {
          if (c.selected) sel.add(Number(c.id));
          else sel.delete(Number(c.id));
        }
        dispatch({ type: "setSelection", ctx, ids: [...sel] });
      }
    },
    [onNodesChange, nodes, ctx],
  );

  const handleEdgesChange = useCallback(
    (changes: EdgeChange[]) => {
      onEdgesChange(changes);
      for (const c of changes) {
        if (c.type === "remove") dispatch({ type: "disconnect", ctx, edge: Number(c.id) });
      }
    },
    [onEdgesChange, ctx],
  );

  const onConnect = useCallback(
    (conn: Connection) => {
      if (!conn.source || !conn.target || !conn.sourceHandle || !conn.targetHandle) return;
      const src = graph.nodes.find((n) => n.id === Number(conn.source));
      const dst = graph.nodes.find((n) => n.id === Number(conn.target));
      if (src && dst) {
        const { kind } = connectionLegal(registry, src.typeId, conn.sourceHandle, dst.typeId, conn.targetHandle);
        if (kind === "lossy") pushToast("Lossy connection (value narrowed)", "warn");
      }
      dispatch({
        type: "connect",
        ctx,
        from: { node: Number(conn.source), port: conn.sourceHandle },
        to: { node: Number(conn.target), port: conn.targetHandle },
      });
      // The mirror re-seed adds the edge; keep RF responsive meanwhile.
      setEdges((es) => addEdge({ ...conn, type: RF_EDGE_TYPE[edgeStyle] }, es));
    },
    [ctx, setEdges, graph, registry, edgeStyle],
  );

  // C3: dragging an existing edge's endpoint re-routes it (one undo step:
  // disconnect + connect inside a transaction); releasing it over empty
  // space or an invalid handle DISCONNECTS (Houdini-style drop-to-void).
  const reconnectedRef = useRef(false);

  const onReconnect = useCallback(
    (oldEdge: Edge, conn: Connection) => {
      if (!conn.source || !conn.target || !conn.sourceHandle || !conn.targetHandle) return;
      reconnectedRef.current = true;
      dispatch({ type: "beginTransaction", label: "reconnect" });
      dispatch({ type: "disconnect", ctx, edge: Number(oldEdge.id) });
      dispatch({
        type: "connect",
        ctx,
        from: { node: Number(conn.source), port: conn.sourceHandle },
        to: { node: Number(conn.target), port: conn.targetHandle },
      });
      dispatch({ type: "endTransaction" });
    },
    [ctx],
  );

  const onReconnectStart = useCallback(() => {
    reconnectedRef.current = false;
  }, []);

  const onReconnectEnd = useCallback(
    (_e: MouseEvent | TouchEvent, edge: Edge) => {
      if (reconnectedRef.current) return;
      // No valid re-target happened: the drag ended on empty space (or an
      // illegal handle) -- disconnect the wire.
      dispatch({ type: "disconnect", ctx, edge: Number(edge.id) });
      pushToast("Disconnected", "info");
    },
    [ctx],
  );

  // A drop on an incompatible handle rejects with a toast NAMING BOTH
  // TYPES (section 6: "Cannot connect Geometry to Float"); the
  // wire also snaps back, React Flow's built-in visual feedback.
  const onConnectEnd = useCallback(
    (
      _e: MouseEvent | TouchEvent,
      state: {
        isValid: boolean | null;
        fromHandle: { nodeId?: string; id?: string | null } | null;
        toHandle: { nodeId?: string; id?: string | null } | null;
      },
    ) => {
      if (!state.toHandle || state.isValid !== false) return;
      const typeName = (h: { nodeId?: string; id?: string | null } | null, dir: "output" | "input") => {
        const node = graph.nodes.find((n) => n.id === Number(h?.nodeId));
        if (!node || !h?.id) return null;
        const dt = portDataType(registry, node.typeId, h.id, dir);
        return dt ? dt[0].toUpperCase() + dt.slice(1) : null;
      };
      const from = typeName(state.fromHandle, "output");
      const to = typeName(state.toHandle, "input");
      pushToast(
        from && to ? `Cannot connect ${from} to ${to}` : "Incompatible connection rejected",
        "error",
      );
    },
    [graph, registry],
  );

  // Only allow legal connections (coercion matrix); enforced server-side too.
  const isValidConnection = useCallback(
    (conn: Connection | Edge) => {
      if (!conn.source || !conn.target || !conn.sourceHandle || !conn.targetHandle) return false;
      const src = graph.nodes.find((n) => n.id === Number(conn.source));
      const dst = graph.nodes.find((n) => n.id === Number(conn.target));
      if (!src || !dst) return false;
      return connectionLegal(registry, src.typeId, conn.sourceHandle, dst.typeId, conn.targetHandle)
        .legal;
    },
    [graph, registry],
  );

  const onNodeDragStop = useCallback(
    (_e: MouseEvent | TouchEvent, _node: Node<FlowNodeData>, dragged: Node<FlowNodeData>[]) => {
      const moves = dragged.map(
        (n) => [Number(n.id), [n.position.x, n.position.y]] as [number, [number, number]],
      );
      if (moves.length) dispatch({ type: "moveNodes", ctx, moves });
    },
    [ctx],
  );

  // Double-click a container to enter its child network. Ownership is the
  // descriptor's `opens` (any network kind), never a category or type id.
  const onNodeDoubleClick = useCallback(
    (_e: React.MouseEvent, node: Node<FlowNodeData>) => {
      const mirror = graph.nodes.find((n) => n.id === Number(node.id));
      if (mirror && descriptorFor(registry, mirror.typeId)?.opens != null) {
        useMirror.getState().setCurrent({ subflow: mirror.id });
      }
    },
    [graph, registry],
  );

  return (
    <div
      className="flow-canvas-track"
      onPointerEnter={() => useViewState.getState().setPointerOverCanvas(true)}
      onPointerLeave={() => useViewState.getState().setPointerOverCanvas(false)}
    >
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={NODE_TYPES}
      onNodesChange={handleNodesChange}
      onEdgesChange={handleEdgesChange}
      onConnect={onConnect}
      onConnectEnd={onConnectEnd}
      onReconnect={onReconnect}
      onReconnectStart={onReconnectStart}
      onReconnectEnd={onReconnectEnd}
      edgesReconnectable
      isValidConnection={isValidConnection}
      onNodeDragStop={onNodeDragStop}
      onNodeDoubleClick={onNodeDoubleClick}
      deleteKeyCode={["Backspace", "Delete"]}
      multiSelectionKeyCode={["Meta", "Control"]}
      connectionLineType={RF_EDGE_TYPE[edgeStyle]}
      snapToGrid={snapToGrid}
      snapGrid={[GRID_GAP, GRID_GAP]}
      zoomOnDoubleClick={false}
      fitView
      proOptions={{ hideAttribution: true }}
      colorMode={resolvedTheme}
    >
      {showFlowGrid && (
        <Background gap={GRID_GAP} color={resolvedTheme === "dark" ? "#3c3c3c" : "#d8d8d8"} />
      )}
      {graph.nodes.length === 0 && (
        // The first-session teaching hint (; revamp 08-02).
        <div className="canvas-empty-hint">
          <kbd className="key-chip">Tab</kbd>
          <span className="empty-title">Press Tab to add a node</span>
          <span className="empty-sub">
            {current === "root"
              ? "or drop a model file to import"
              : "or drag one in from the palette"}
          </span>
        </div>
      )}
      {graph.nodes.length > 0 && graph.activeOutput === null && current !== "root" && (
        // The cleared-display ghost chip (sec. 10; revamp 08-02):
        // this subflow currently renders nothing.
        <div className="ghost-chip-row">
          <span className="ghost-chip">
            <span className="ghost-dot" />
            no display node
          </span>
          <span className="empty-sub">Set a node's display flag to render it</span>
        </div>
      )}
      {showMinimap && (
        <MiniMap
          pannable
          zoomable
          className="flow-minimap"
          nodeColor={minimapNodeColor(registry)}
        />
      )}
      {showFlowControls && <Controls showInteractive={false} />}
    </ReactFlow>
    {/* The hover radial lives INSIDE the flow so it can subscribe to the
        viewport transform and stay glued to its node during pan and zoom.
        It still portals to the body, so its stacking context is unchanged. */}
    <RadialMenu />
    </div>
  );
}
