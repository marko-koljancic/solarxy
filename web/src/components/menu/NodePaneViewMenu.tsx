// The node-pane View menu: canvas chrome toggles, the connection
// style radio, and auto-layout, relocated from the old global View menu so
// the pane owns its own chrome. The graph/list switch moved out to the
// toolbar's right-side icon command.

import { runLayout } from "../../flow/layout";
import { selectGraph, useMirror } from "../../store/mirror";
import { useRadial } from "../../store/radial";
import { EDGE_STYLES, EDGE_STYLE_LABELS, useUi } from "../../store/ui";
import { MenuItem, type MenuEntry } from "./MenuItem";

export function NodePaneViewMenu() {
  const current = useMirror((s) => s.current);
  const selection = useMirror((s) => selectGraph(s, s.current).selection);
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
    { divider: true },
    {
      // The third way in, after the hover radial and the I key. The modal
      // documents what a node does, so it should not itself require inside
      // knowledge to find.
      label: "Node Info",
      shortcut: "I",
      disabled: !selection.length,
      onClick: () => {
        const id = selection[0];
        const el = document.querySelector(`.react-flow__node[data-id="${id}"]`);
        const host = document.querySelector(".node-canvas-host")?.getBoundingClientRect();
        const r = el?.getBoundingClientRect();
        const at = r
          ? { x: r.right + 16, y: r.top }
          : { x: (host?.left ?? 0) + 40, y: (host?.top ?? 0) + 80 };
        useRadial.getState().openInfo(id, current, at.x, at.y);
      },
    },
  ];

  return <MenuItem title="View" entries={entries} />;
}
