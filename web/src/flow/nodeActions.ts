// The shared per-node action vocabulary: one dispatch site behind the
// hover radial's wedges and the list view's row buttons, so the two
// surfaces cannot drift apart. Pure mirror/command calls; UI concerns
// (closing the ring, the info modal's position) stay at the call sites.

import { dispatch } from "../engine/session";
import type { GraphContext, NodeMirror, NodeTypeSnapshot } from "../engine/types";
import { useMirror } from "../store/mirror";
import { useRadial } from "../store/radial";
import { useUi } from "../store/ui";
import { nodeRole } from "./nodeVisual";
import { nodeVisible } from "./visibility";

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
 * `hasVisibleParam(desc)`; dispatching without one is a no-op warning in
 * the engine, not a crash. */
export function toggleVisibility(ctx: GraphContext, node: NodeMirror): void {
  dispatch({
    type: "setParam",
    ctx,
    node: node.id,
    key: "visible",
    value: { kind: "literal", type: "bool", value: !nodeVisible(node) },
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

/** The same gates the canvas node uses when opening the radial. */
export function isContainerType(desc: NodeTypeSnapshot | undefined): boolean {
  return nodeRole(desc) === "container";
}

export function isBypassable(desc: NodeTypeSnapshot | undefined): boolean {
  return desc?.bypass.mode !== "notBypassable";
}
