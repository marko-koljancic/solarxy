// The Nodes menu: registry-driven node creation grouped by category (a
// pure interpreter of the snapshot, so a node added in Rust appears with
// zero changes here). Relocated from the top menu bar into the node-pane
// header (Phase 7b): node management lives beside the canvas it acts on.

import { dispatch } from "../../engine/session";
import { selectGraph, useMirror } from "../../store/mirror";
import { MenuItem, type MenuEntry } from "./MenuItem";

export function NodesMenu() {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));

  const inRoot = current === "root";
  const byCat = new Map<string, { label: string; typeId: string }[]>();
  for (const n of registry?.nodes ?? []) {
    if (inRoot ? !n.rootContext : !n.subflowContext) continue;
    const g = byCat.get(n.category) ?? [];
    g.push({ label: n.displayName, typeId: n.typeId });
    byCat.set(n.category, g);
  }
  const addNode = (typeId: string) => {
    const n = graph.nodes.length;
    const position: [number, number] = [80 + (n % 5) * 44, 80 + Math.floor(n / 5) * 90];
    dispatch({ type: "addNode", ctx: current, nodeType: typeId, position });
  };
  const entries: MenuEntry[] = [...byCat.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([cat, list]) => ({
      label: cat,
      submenu: list.map((t) => ({ label: t.label, onClick: () => addNode(t.typeId) })),
    }));

  return (
    <nav className="menu-bar node-pane-menu">
      <MenuItem title="Nodes" entries={entries} />
    </nav>
  );
}
