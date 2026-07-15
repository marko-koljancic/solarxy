// The node-pane View menu (Phase 9): canvas chrome toggles, the connection
// style radio, and auto-layout, relocated from the old global View menu so
// the pane owns its own chrome. The graph/list switch moved out to the
// toolbar's right-side icon command (D-24).

import { runLayout } from "../../flow/layout";
import { EDGE_STYLES, EDGE_STYLE_LABELS, useUi } from "../../store/ui";
import { MenuItem, type MenuEntry } from "./MenuItem";

export function NodePaneViewMenu() {
  const showFlowGrid = useUi((s) => s.showFlowGrid);
  const showMinimap = useUi((s) => s.showMinimap);
  const showFlowControls = useUi((s) => s.showFlowControls);
  const snapToGrid = useUi((s) => s.snapToGrid);
  const edgeStyle = useUi((s) => s.edgeStyle);

  const entries: MenuEntry[] = [
    {
      label: "Canvas Grid",
      shortcut: "G",
      checked: showFlowGrid,
      onClick: () => useUi.getState().toggleFlowChrome("showFlowGrid"),
    },
    {
      label: "Snap to Grid",
      checked: snapToGrid,
      onClick: () => useUi.getState().toggleFlowChrome("snapToGrid"),
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
  ];

  return <MenuItem title="View" entries={entries} />;
}
