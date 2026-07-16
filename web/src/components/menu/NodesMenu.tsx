// The Add menu: registry-driven node creation grouped by category (a
// pure interpreter of the snapshot, so a node added in Rust appears with
// zero changes here), led by a palette-opening search entry. Lives in the
// node-pane menu bar (Phase 9); node management sits beside the canvas it
// acts on.

import { dispatch } from "../../engine/session";
import { contextKind } from "../../registry/datatypes";
import { selectGraph, useMirror } from "../../store/mirror";
import { useUi } from "../../store/ui";
import { MenuItem, type MenuEntry } from "./MenuItem";

export function NodesMenu() {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const rootNodes = useMirror((s) => selectGraph(s, "root").nodes);

  const kind = contextKind(registry, current, rootNodes);
  const byCat = new Map<string, { label: string; typeId: string }[]>();
  const catLabels = new Map<string, string>();
  for (const n of registry?.nodes ?? []) {
    if (!n.contexts.includes(kind)) continue;
    const g = byCat.get(n.category) ?? [];
    g.push({ label: n.displayName, typeId: n.typeId });
    byCat.set(n.category, g);
    catLabels.set(n.category, n.categoryLabel);
  }
  const addNode = (typeId: string) => {
    const n = graph.nodes.length;
    const position: [number, number] = [80 + (n % 5) * 44, 80 + Math.floor(n / 5) * 90];
    dispatch({ type: "addNode", ctx: current, nodeType: typeId, position });
  };
  const entries: MenuEntry[] = [
    {
      label: "Search Nodes...",
      shortcut: "Tab",
      onClick: () => useUi.getState().setPaletteOpen(true),
    },
    { divider: true },
    ...[...byCat.entries()].sort((a, b) => a[0].localeCompare(b[0])).map(([cat, list]) => ({
      label: catLabels.get(cat) ?? cat,
      submenu: list.map((t) => ({ label: t.label, onClick: () => addNode(t.typeId) })),
    })),
  ];

  return <MenuItem title="Add" entries={entries} />;
}
