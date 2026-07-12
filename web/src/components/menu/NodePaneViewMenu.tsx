// The node-pane View menu (Phase 9): canvas chrome toggles, the connection
// style radio, auto-layout, and the graph/list switch, relocated from the
// old global View menu so the pane owns its own chrome.

import { runLayout } from "../../flow/layout";
import { ctxKey } from "../../engine/types";
import { useMirror } from "../../store/mirror";
import { EDGE_STYLES, EDGE_STYLE_LABELS, useUi } from "../../store/ui";
import { MenuItem, type MenuEntry } from "./MenuItem";

export function NodePaneViewMenu() {
  const current = useMirror((s) => s.current);
  const showFlowGrid = useUi((s) => s.showFlowGrid);
  const showMinimap = useUi((s) => s.showMinimap);
  const showFlowControls = useUi((s) => s.showFlowControls);
  const edgeStyle = useUi((s) => s.edgeStyle);
  const flowView = useUi((s) => s.flowView[ctxKey(current)] ?? "graph");

  const entries: MenuEntry[] = [
    {
      label: "Canvas Grid",
      shortcut: "G",
      checked: showFlowGrid,
      onClick: () => useUi.getState().toggleFlowChrome("showFlowGrid"),
    },
    {
      label: "Minimap",
      shortcut: "M",
      checked: showMinimap,
      onClick: () => useUi.getState().toggleFlowChrome("showMinimap"),
    },
    {
      label: "Canvas Controls",
      shortcut: "C",
      checked: showFlowControls,
      onClick: () => useUi.getState().toggleFlowChrome("showFlowControls"),
    },
    { divider: true },
    {
      label: "Connection Style",
      submenu: EDGE_STYLES.map((style) => ({
        label: EDGE_STYLE_LABELS[style],
        checked: edgeStyle === style,
        onClick: () => useUi.getState().setEdgeStyle(style),
      })),
    },
    { divider: true },
    { label: "Auto-Layout (Dagre)", shortcut: "L", onClick: () => runLayout("dagre") },
    { label: "Auto-Layout (ELK)", onClick: () => runLayout("elk") },
    { divider: true },
    {
      label: "List View",
      checked: flowView === "list",
      onClick: () =>
        useUi.getState().setFlowView(ctxKey(current), flowView === "list" ? "graph" : "list"),
    },
  ];

  return <MenuItem title="View" entries={entries} />;
}
