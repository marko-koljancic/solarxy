// The application menu bar (File / Edit / Nodes / View / Help), carried
// from Minimystix's Header. Every item dispatches through the same session
// functions the keyboard uses; the Nodes menu is a pure interpreter of the
// registry snapshot (categories to node types), so a node added in Rust
// appears with zero changes here.

import { useRef, useState } from "react";
import {
  copySelection,
  dispatch,
  duplicateSelection,
  explicitSave,
  importDroppedFiles,
  openScene,
  paste,
} from "../../engine/session";
import { clearAutosaves } from "../../persistence/opfs";
import { EnvironmentModal } from "../EnvironmentModal";
import { selectGraph, useMirror } from "../../store/mirror";
import { pushToast } from "../../store/toasts";
import { useUi, type ThemeChoice } from "../../store/ui";
import { MenuItem, type MenuEntry } from "./MenuItem";

const MOD = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";

async function newScene(): Promise<void> {
  if (useMirror.getState().dirty) {
    const ok = window.confirm("Discard unsaved changes and start a new scene?");
    if (!ok) return;
    // Confirmed once here; drop the dirty flag so the beforeunload guard
    // does not double-prompt on the reload.
    useMirror.getState().setDirty(false);
  }
  await clearAutosaves();
  window.location.reload();
}

export function MenuBar() {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const theme = useUi((s) => s.theme);
  const viewportMaximized = useUi((s) => s.viewportMaximized);
  const drawerCollapsed = useUi((s) => s.drawerCollapsed);
  const importRef = useRef<HTMLInputElement>(null);
  const [envOpen, setEnvOpen] = useState(false);

  const selection = graph.selection;
  const hasSelection = selection.length > 0;

  const file: MenuEntry[] = [
    { label: "New Scene", onClick: () => void newScene() },
    { label: "Open Scene...", shortcut: `${MOD}O`, onClick: () => void openScene() },
    { label: "Save Scene", shortcut: `${MOD}S`, onClick: () => void explicitSave() },
    { divider: true },
    { label: "Import Model...", onClick: () => importRef.current?.click() },
  ];

  const withBypass = () => {
    const first = graph.nodes.find((n) => n.id === selection[0]);
    const bypassed = !(first?.bypassed ?? false);
    for (const id of selection) {
      dispatch({ type: "setBypass", ctx: current, node: id, bypassed });
    }
  };

  const edit: MenuEntry[] = [
    { label: "Undo", shortcut: `${MOD}Z`, onClick: () => dispatch({ type: "undo" }) },
    { label: "Redo", shortcut: `${MOD}⇧Z`, onClick: () => dispatch({ type: "redo" }) },
    { divider: true },
    { label: "Copy", shortcut: `${MOD}C`, disabled: !hasSelection, onClick: copySelection },
    { label: "Paste", shortcut: `${MOD}V`, onClick: paste },
    { label: "Duplicate", shortcut: `${MOD}D`, disabled: !hasSelection, onClick: duplicateSelection },
    { divider: true },
    { label: "Toggle Bypass", shortcut: "B", disabled: !hasSelection, onClick: withBypass },
    {
      label: "Set Display Flag",
      shortcut: "E",
      disabled: !hasSelection || current === "root",
      onClick: () =>
        dispatch({ type: "setActiveOutput", ctx: current, node: selection[0] }),
    },
    {
      label: "Delete Selection",
      shortcut: "⌫",
      disabled: !hasSelection,
      onClick: () => dispatch({ type: "removeNodes", ctx: current, ids: selection }),
    },
  ];

  // Registry-driven node creation, filtered to the current context.
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
  const nodes: MenuEntry[] = [...byCat.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([cat, list]) => ({
      label: cat,
      submenu: list.map((t) => ({ label: t.label, onClick: () => addNode(t.typeId) })),
    }));

  const setTheme = (t: ThemeChoice) => useUi.getState().setTheme(t);
  const view: MenuEntry[] = [
    { label: "Environment...", onClick: () => setEnvOpen(true) },
    { divider: true },
    {
      label: "Theme",
      submenu: (["dark", "light", "system"] as const).map((t) => ({
        label: t[0].toUpperCase() + t.slice(1),
        checked: theme === t,
        onClick: () => setTheme(t),
      })),
    },
    { divider: true },
    {
      label: "Maximize Viewport",
      checked: viewportMaximized,
      onClick: () => useUi.getState().toggleViewportMaximized(),
    },
    {
      label: "Collapse Properties",
      checked: drawerCollapsed,
      onClick: () => useUi.getState().toggleDrawerCollapsed(),
    },
  ];

  const help: MenuEntry[] = [
    {
      label: "About Solarxy Web",
      onClick: () =>
        pushToast(
          `Solarxy Web - ${registry ? `${registry.nodes.length} node types` : "engine loading"}`,
          "info",
        ),
    },
  ];

  return (
    <nav className="menu-bar">
      <MenuItem title="File" entries={file} />
      <MenuItem title="Edit" entries={edit} />
      <MenuItem title="Nodes" entries={nodes} />
      <MenuItem title="View" entries={view} />
      <MenuItem title="Help" entries={help} />
      {envOpen && <EnvironmentModal onClose={() => setEnvOpen(false)} />}
      <input
        ref={importRef}
        type="file"
        multiple
        accept=".obj,.gltf,.glb,.stl,.ply,.bin,.slxy"
        style={{ display: "none" }}
        onChange={(e) => {
          const files = Array.from(e.target.files ?? []);
          if (files.length) void importDroppedFiles(files);
          e.target.value = "";
        }}
      />
    </nav>
  );
}
