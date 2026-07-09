// The node editor canvas: a React Flow view of the current context's graph,
// rewired to the mirror-and-command model. React Flow gestures dispatch
// Commands; the mirror updates and re-seeds the local RF state. React never
// owns document truth - it is a display mirror.

import { useCallback, useEffect, useMemo } from "react";
import {
  Background,
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
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { dispatch } from "../engine/session";
import type { GraphContext, GraphMirror, RegistrySnapshot } from "../engine/types";
import { connectionLegal, descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { pushToast } from "../store/toasts";
import { useUi } from "../store/ui";
import { FlowNode, type FlowNodeData } from "./FlowNode";

const NODE_TYPES = { solarxy: FlowNode };

/** Maps a mirror graph to React Flow nodes/edges, styling each edge by its
 * coercion kind (plain for same/lossless, warning for lossy). */
function toRf(
  graph: GraphMirror,
  registry: RegistrySnapshot | null,
): { nodes: Node<FlowNodeData>[]; edges: Edge[] } {
  const nodes = graph.nodes.map((n) => ({
    id: String(n.id),
    type: "solarxy",
    position: { x: n.position[0], y: n.position[1] },
    selected: graph.selection.includes(n.id),
    data: { node: n, isDisplay: graph.activeOutput === n.id },
  }));
  const edges = graph.edges.map((e) => {
    const from = graph.nodes.find((n) => n.id === e.from);
    const to = graph.nodes.find((n) => n.id === e.to);
    let cls = "";
    if (from && to) {
      const { kind } = connectionLegal(registry, from.typeId, e.fromPort, to.typeId, e.toPort);
      if (kind === "lossy") cls = "edge-lossy";
      else if (kind === "lossless") cls = "edge-coerced";
    }
    return {
      id: String(e.id),
      source: String(e.from),
      target: String(e.to),
      sourceHandle: e.fromPort,
      targetHandle: e.toPort,
      type: "default",
      className: cls,
    };
  });
  return { nodes, edges };
}

export function NodeCanvas() {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const resolvedTheme = useUi((s) => s.resolvedTheme);

  const seed = useMemo(() => toRf(graph, registry), [graph, registry]);
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
      setEdges((es) => addEdge({ ...conn, type: "default" }, es));
    },
    [ctx, setEdges, graph, registry],
  );

  // A drop on an incompatible handle rejects with a toast (the wire also
  // snaps back, React Flow's built-in visual feedback).
  const onConnectEnd = useCallback(
    (_e: MouseEvent | TouchEvent, state: { isValid: boolean | null; toHandle: { nodeId?: string } | null }) => {
      if (state.toHandle && state.isValid === false) {
        pushToast("Incompatible connection rejected", "error");
      }
    },
    [],
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

  // Double-click a geo container to enter its subflow.
  const onNodeDoubleClick = useCallback(
    (_e: React.MouseEvent, node: Node<FlowNodeData>) => {
      const mirror = graph.nodes.find((n) => n.id === Number(node.id));
      if (mirror && descriptorFor(registry, mirror.typeId)?.category === "container") {
        useMirror.getState().setCurrent({ subflow: mirror.id });
      }
    },
    [graph, registry],
  );

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={NODE_TYPES}
      onNodesChange={handleNodesChange}
      onEdgesChange={handleEdgesChange}
      onConnect={onConnect}
      onConnectEnd={onConnectEnd}
      isValidConnection={isValidConnection}
      onNodeDragStop={onNodeDragStop}
      onNodeDoubleClick={onNodeDoubleClick}
      deleteKeyCode={["Backspace", "Delete"]}
      multiSelectionKeyCode={["Meta", "Control"]}
      fitView
      proOptions={{ hideAttribution: true }}
      colorMode={resolvedTheme}
    >
      <Background gap={18} color={resolvedTheme === "dark" ? "#3c3c3c" : "#d8d8d8"} />
      <MiniMap pannable zoomable className="flow-minimap" />
      <Controls showInteractive={false} />
    </ReactFlow>
  );
}
