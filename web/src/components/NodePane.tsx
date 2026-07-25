// The self-contained node pane:
// a Blender-style menu-bar row (Add + View), the breadcrumb on its own row,
// the graph/list canvas host, and the always-mounted palette. The bottom
// properties drawer arrives through the children slot so this pane never
// depends on properties internals.

import { ReactFlowProvider } from "@xyflow/react";
import React from "react";
import { ctxKey, type GraphContext, type NodeTypeSnapshot } from "../engine/types";
import { FlowListView } from "../flow/FlowListView";
import { NodeCanvas } from "../flow/NodeCanvas";
import { nodeLabel } from "../flow/nodeLabel";
import { IconGraphView, IconListView } from "../icons";
import { descriptorFor } from "../registry/datatypes";
import { useMirror } from "../store/mirror";
import { useUi } from "../store/ui";
import { NodesMenu } from "./menu/NodesMenu";
import { NodePaneViewMenu } from "./menu/NodePaneViewMenu";
import { NodeGlyph } from "./NodeGlyph";
import { NodePalette } from "./NodePalette";

function Breadcrumb() {
  const current = useMirror((s) => s.current);
  const registry = useMirror((s) => s.registry);
  const contexts = useMirror((s) => s.contexts);
  const setCurrent = useMirror((s) => s.setCurrent);

  if (current === "root") return <div className="breadcrumb">Scene</div>;

  // Walk the owner chain up to the root (containers nest):
  // each child context's owner node lives in some enclosing graph; that
  // graph is the next crumb out.
  const chain: { ctx: GraphContext; label: string; desc?: NodeTypeSnapshot }[] = [];
  let cursor: GraphContext = current;
  let guard = 0;
  while (cursor !== "root" && guard < 64) {
    guard += 1;
    const ownerId = cursor.subflow;
    let holder: GraphContext = "root";
    let ownerLabel = "subflow";
    let ownerDesc: NodeTypeSnapshot | undefined;
    for (const [key, g] of Object.entries(contexts)) {
      const owner = g.nodes.find((n) => n.id === ownerId);
      if (owner) {
        ownerDesc = descriptorFor(registry, owner.typeId);
        ownerLabel = nodeLabel(owner, ownerDesc);
        holder = key === "root" ? "root" : { subflow: Number(key.slice(4)) };
        break;
      }
    }
    chain.unshift({ ctx: cursor, label: ownerLabel, desc: ownerDesc });
    cursor = holder;
  }

  // Crumbs and separators render as direct flex children (no wrapper
  // spans), so the row's align-items and gap govern every piece and the
  // separators cannot drift off the text's centerline.
  return (
    <div className="breadcrumb">
      <button className="crumb-link" onClick={() => setCurrent("root")}>
        Scene
      </button>
      {chain.map((crumb, i) => {
        const last = i === chain.length - 1;
        return (
          <React.Fragment key={ctxKey(crumb.ctx)}>
            <span className="crumb-sep">›</span>
            {last ? (
              <span className="crumb-current">
                {crumb.desc && <NodeGlyph desc={crumb.desc} size={13} />}
                {crumb.label}
              </span>
            ) : (
              <button className="crumb-link" onClick={() => setCurrent(crumb.ctx)}>
                {crumb.desc && <NodeGlyph desc={crumb.desc} size={13} />}
                {crumb.label}
              </button>
            )}
          </React.Fragment>
        );
      })}
    </div>
  );
}

export function NodePane({ children }: { children?: React.ReactNode }) {
  const current = useMirror((s) => s.current);
  const flowView = useUi((s) => s.flowView[ctxKey(current)] ?? "graph");

  return (
    <div className="node-pane">
      <nav className="menu-bar node-pane-menu node-toolbar">
        <NodesMenu />
        <NodePaneViewMenu />
        {/* The graph/list switch (D-24): a right-side icon command; the
            icon advertises the view a click switches TO. */}
        <button
          className="tbtn icon flow-view-toggle"
          title={flowView === "list" ? "Graph view" : "List view"}
          onClick={() =>
            useUi.getState().setFlowView(ctxKey(current), flowView === "list" ? "graph" : "list")
          }
        >
          {flowView === "list" ? <IconGraphView /> : <IconListView />}
        </button>
      </nav>
      <div className="breadcrumb-row">
        <Breadcrumb />
      </div>
      <div className="node-canvas-host">
        {flowView === "list" ? (
          <FlowListView />
        ) : (
          <ReactFlowProvider>
            <NodeCanvas />
          </ReactFlowProvider>
        )}
      </div>
      <NodePalette />
      {children}
    </div>
  );
}
