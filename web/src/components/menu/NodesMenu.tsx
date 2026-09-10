// The Add menu: registry-driven node creation grouped by category (a
// pure interpreter of the snapshot, so a node added in Rust appears with
// zero changes here), led by a palette-opening search entry. Lives in the
// node-pane menu bar; node management sits beside the canvas it
// acts on.

import { dispatch } from "../../engine/session";
import type { NodeTypeSnapshot } from "../../engine/types";
import { compareCategories } from "../../registry/datatypes";
import { selectGraph, useMirror } from "../../store/mirror";
import { useUi } from "../../store/ui";
import { NodeGlyph } from "../NodeGlyph";
import { MenuItem, type MenuEntry } from "./MenuItem";

export function NodesMenu() {
  const registry = useMirror((s) => s.registry);
  const tables = useMirror((s) => s.presentation);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));

  const kind = useMirror((s) => selectGraph(s, s.current).kind);
  const byCat = new Map<string, NodeTypeSnapshot[]>();
  const catLabels = new Map<string, string>();
  for (const n of registry?.nodes ?? []) {
    if (!n.contexts.includes(kind)) continue;
    const g = byCat.get(n.category) ?? [];
    g.push(n);
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
    // The snapshot lists nodes alphabetically by type id; the engine's
    // category order decides submenu order.
    ...[...byCat.entries()]
      .sort(([a], [b]) => compareCategories(tables, a, b))
      .map(([cat, list]) => ({
      label: catLabels.get(cat) ?? cat,
      submenu: list.map((t) => ({
        label: t.displayName,
        icon: <NodeGlyph desc={t} size={13} />,
        onClick: () => addNode(t.typeId),
      })),
    })),
  ];

  return <MenuItem title="Add" entries={entries} />;
}
