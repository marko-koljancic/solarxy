// The four dock panels. Each is a thin wrapper around a pane component that
// Phase 9 already made self-contained, which is exactly why this phase does not
// have to restructure any of them.

import {
  DockviewDefaultTab,
  type IDockviewPanelHeaderProps,
  type IDockviewPanelProps,
} from "dockview-react";
import { NodePane } from "../components/NodePane";
import { ParameterPanel } from "../components/ParameterPanel";
import { Viewport } from "../components/Viewport";
import { ReviewPanel } from "../components/review/ReviewPanel";
import { nodeLabel } from "../flow/nodeLabel";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";

/** The selected node's display label (its `name` param when renamed, else the
 * type display name), shown in the Properties header. */
function useSelectedNodeName(): string {
  const registry = useMirror((s) => s.registry);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const selected = graph.nodes.find((n) => n.id === graph.selection[0]);
  if (!selected) return "";
  return nodeLabel(selected, descriptorFor(registry, selected.typeId));
}

function ViewportPanel(_props: IDockviewPanelProps) {
  return <Viewport />;
}

function NodesPanel(_props: IDockviewPanelProps) {
  return <NodePane />;
}

/** The single Properties panel. It replaces the bottom drawer and the
 * right-docked column: dockview owns the docking, resizing and tabbing that
 * those two hand-rolled variants existed to provide. */
function PropertiesPanel(_props: IDockviewPanelProps) {
  const title = useSelectedNodeName();
  return (
    <div className="properties-panel">
      {title && <div className="properties-panel-context">{title}</div>}
      <div className="properties-panel-body">
        <ParameterPanel />
      </div>
    </div>
  );
}

function ReviewDockPanel(_props: IDockviewPanelProps) {
  return <ReviewPanel />;
}

export const DOCK_COMPONENTS = {
  viewport: ViewportPanel,
  nodes: NodesPanel,
  properties: PropertiesPanel,
  review: ReviewDockPanel,
};

/** The pinned tab: no close button. The other half of the pin (cancelling the
 * tab drag) lives in Dock.tsx; together they mirror the desktop, where the
 * Viewport dock tab is neither floatable nor closeable. */
function PinnedTab(props: IDockviewPanelHeaderProps) {
  return <DockviewDefaultTab {...props} hideClose />;
}

export const DOCK_TAB_COMPONENTS = {
  pinned: PinnedTab,
};
