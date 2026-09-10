// The shared per-node action vocabulary: one dispatch site behind the
// hover radial's wedges and the list view's row buttons, so the two
// surfaces cannot drift apart. Pure mirror/command calls; UI concerns
// (closing the ring, the info modal's position) stay at the call sites.

import { dispatch } from "../engine/session";
import type { GraphContext, NodeMirror, NodeTypeSnapshot } from "../engine/types";
import { selectGraph, useMirror } from "../store/mirror";
import { useRadial } from "../store/radial";
import { useUi } from "../store/ui";

/** Start the inline rename (the list view and canvas both listen for it). */
export function requestRename(nodeId: number): void {
  useUi.getState().setRenameRequest(nodeId);
}

/** Subflow contexts: the display flag is a radio over the container's
 * output, so picking a node is absolute, not a toggle. */
export function setDisplayFlag(ctx: GraphContext, nodeId: number): void {
  dispatch({ type: "setActiveOutput", ctx, node: nodeId });
}

/** Root context: the additive per-node `visible` param. Callers gate on
 * the node's `declaresVisibility`; dispatching without one is a no-op warning in
 * the engine, not a crash. */
export function toggleVisibility(ctx: GraphContext, node: NodeMirror): void {
  dispatch({
    type: "setParam",
    ctx,
    node: node.id,
    key: "visible",
    value: { kind: "literal", type: "bool", value: !node.visible },
  });
}

export function diveIntoSubflow(nodeId: number): void {
  useMirror.getState().setCurrent({ subflow: nodeId });
}

/** Open the modeless node-info card at a screen position. */
export function openNodeInfo(nodeId: number, ctx: GraphContext, x: number, y: number): void {
  useRadial.getState().openInfo(nodeId, ctx, x, y);
}

export function toggleBypass(ctx: GraphContext, node: NodeMirror): void {
  dispatch({ type: "setBypass", ctx, node: node.id, bypassed: !node.bypassed });
}

export function removeNode(ctx: GraphContext, nodeId: number): void {
  dispatch({ type: "removeNodes", ctx, ids: [nodeId] });
}

/** The node's display path for the clipboard: `/name` for a root node,
 * `/container/name` inside a subflow, using the same labels the UI shows
 * everywhere (the `name` param when renamed, else the type name). */
export function nodePathOf(ctx: GraphContext, node: NodeMirror): string {
  const s = useMirror.getState();
  const seg = (n: NodeMirror) => n.label;
  if (ctx === "root") return `/${seg(node)}`;
  const container = selectGraph(s, "root").nodes.find((n) => n.id === ctx.subflow);
  return container ? `/${seg(container)}/${seg(node)}` : `/${seg(node)}`;
}

/** The same gates the canvas node uses when opening the radial.
 *
 * Asks what the node OPENS rather than what silhouette it wears. The two
 * agree today and the question is a semantic one: diving in needs a
 * network to dive into, and a type that declares none has nothing to
 * show whatever it is drawn as. Reading the silhouette for this was the
 * last place a presentation answer stood in for an engine one. */
export function isContainerType(desc: NodeTypeSnapshot | undefined): boolean {
  return desc?.opens != null;
}

export function isBypassable(desc: NodeTypeSnapshot | undefined): boolean {
  return desc?.bypass.mode !== "notBypassable";
}
