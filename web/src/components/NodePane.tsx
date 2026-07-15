// The self-contained node pane (Phase 9, a Phase 10 docking prerequisite):
// a Blender-style menu-bar row (Add + View), the breadcrumb on its own row,
// the graph/list canvas host, and the always-mounted palette. The bottom
// properties drawer arrives through the children slot so this pane never
// depends on properties internals.

import { ReactFlowProvider } from "@xyflow/react";
import { ctxKey } from "../engine/types";
import { FlowListView } from "../flow/FlowListView";
import { NodeCanvas } from "../flow/NodeCanvas";
import { nodeLabel } from "../flow/nodeLabel";
import { IconGraphView, IconListView } from "../icons";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { useUi } from "../store/ui";
import { NodesMenu } from "./menu/NodesMenu";
import { NodePaneViewMenu } from "./menu/NodePaneViewMenu";
import { NodePalette } from "./NodePalette";

function Breadcrumb() {
  const current = useMirror((s) => s.current);
  const registry = useMirror((s) => s.registry);
  const root = useMirror((s) => selectGraph(s, "root"));
  const setCurrent = useMirror((s) => s.setCurrent);

  if (current === "root") return <div className="breadcrumb">Scene</div>;
  const owner = root.nodes.find((n) => n.id === current.subflow);
  const name = owner ? nodeLabel(owner, descriptorFor(registry, owner.typeId)) : "subflow";
  return (
    <div className="breadcrumb">
      <button className="crumb-link" onClick={() => setCurrent("root")}>
        Scene
      </button>
      <span className="crumb-sep">›</span>
      <span>{name}</span>
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
