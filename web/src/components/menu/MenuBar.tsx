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
import { ConfirmDialog } from "../ConfirmDialog";
import { applyDagreLayout, applyElkLayout } from "../../flow/layout";
import { AboutModal } from "../AboutModal";
import { EnvironmentModal } from "../EnvironmentModal";
import { selectGraph, useMirror } from "../../store/mirror";
import { DESK_PRESETS, useDesks } from "../../store/desks";
import { useReview } from "../../store/review";
import { usePrefs, type ThemeChoice } from "../../store/prefs";
import { useUi } from "../../store/ui";
import { DeskSaveModal } from "../DeskSaveModal";
import { MenuItem, type MenuEntry } from "./MenuItem";

const MOD = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";

/** Clears the dirty flag (so the beforeunload guard stays quiet), drops
 * the autosave ring, and reloads into a fresh scene. Confirmation happens
 * in the styled dialog before this runs. */
async function newScene(): Promise<void> {
  useMirror.getState().setDirty(false);
  await clearAutosaves();
  window.location.reload();
}

export function MenuBar() {
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const theme = usePrefs((s) => s.prefs.appearance.theme);
  const viewportMaximized = useUi((s) => s.viewportMaximized);
  const drawerCollapsed = useUi((s) => s.drawerCollapsed);
  const importRef = useRef<HTMLInputElement>(null);
  const [envOpen, setEnvOpen] = useState(false);
  const [confirmNew, setConfirmNew] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);

  const selection = graph.selection;
  const hasSelection = selection.length > 0;

  const file: MenuEntry[] = [
    {
      label: "New Scene",
      onClick: () => {
        if (useMirror.getState().dirty) setConfirmNew(true);
        else void newScene();
      },
    },
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
    { divider: true },
    { label: "Preferences...", shortcut: `${MOD},`, onClick: () => useUi.getState().setPrefsOpen(true) },
  ];

  const setTheme = (t: ThemeChoice) => usePrefs.getState().setTheme(t);
  const showFlowGrid = useUi((s) => s.showFlowGrid);
  const showMinimap = useUi((s) => s.showMinimap);
  const showFlowControls = useUi((s) => s.showFlowControls);
  const runLayout = (algo: "dagre" | "elk") => {
    const g = selectGraph(useMirror.getState(), current);
    if (g.nodes.length === 0) return;
    const done = () => window.dispatchEvent(new Event("solarxy:fitView"));
    if (algo === "dagre") {
      applyDagreLayout(current, g);
      done();
    } else {
      void applyElkLayout(current, g).then(done);
    }
  };
  const view: MenuEntry[] = [
    { label: "Environment...", onClick: () => setEnvOpen(true) },
    { label: "Save Screenshot...", shortcut: "C", onClick: () => useUi.getState().setScreenshotOpen(true) },
    { divider: true },
    { label: "Auto-Layout (Dagre)", shortcut: "L", onClick: () => runLayout("dagre") },
    { label: "Auto-Layout (ELK)", onClick: () => runLayout("elk") },
    { divider: true },
    { label: "Canvas Grid", shortcut: "G", checked: showFlowGrid, onClick: () => useUi.getState().toggleFlowChrome("showFlowGrid") },
    { label: "Minimap", shortcut: "M", checked: showMinimap, onClick: () => useUi.getState().toggleFlowChrome("showMinimap") },
    { label: "Canvas Controls", shortcut: "C", checked: showFlowControls, onClick: () => useUi.getState().toggleFlowChrome("showFlowControls") },
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

  // Desks (Phase 7b D3): presets + user-saved arrangements, direct
  // arrangement toggles, save-as, delete. Applying never touches the
  // document, only chrome.
  const userDesks = useDesks((s) => s.desks);
  const viewportSide = useUi((s) => s.viewportSide);
  const propertiesDock = useUi((s) => s.propertiesDock);
  const [deskSaveOpen, setDeskSaveOpen] = useState(false);
  const desks: MenuEntry[] = [
    ...DESK_PRESETS.map((d) => ({
      label: d.name,
      onClick: () => useDesks.getState().apply(d.name),
    })),
    ...(userDesks.length > 0 ? [{ divider: true } as MenuEntry] : []),
    ...userDesks.map((d) => ({
      label: d.name,
      onClick: () => useDesks.getState().apply(d.name),
    })),
    { divider: true },
    {
      label: "Viewport on Left",
      checked: viewportSide === "left",
      onClick: () => useUi.getState().setArrangement({ viewportSide: "left" }),
    },
    {
      label: "Viewport on Right",
      checked: viewportSide === "right",
      onClick: () => useUi.getState().setArrangement({ viewportSide: "right" }),
    },
    {
      label: "Properties at Bottom",
      checked: propertiesDock === "bottom",
      onClick: () => useUi.getState().setArrangement({ propertiesDock: "bottom" }),
    },
    {
      label: "Properties on Right",
      checked: propertiesDock === "right",
      onClick: () => useUi.getState().setArrangement({ propertiesDock: "right" }),
    },
    { divider: true },
    { label: "Save Current As...", onClick: () => setDeskSaveOpen(true) },
    {
      label: "Delete Desk",
      disabled: userDesks.length === 0,
      submenu: userDesks.map((d) => ({
        label: d.name,
        onClick: () => useDesks.getState().remove(d.name),
      })),
    },
  ];

  const reviewMode = useReview((s) => s.reviewMode);
  const markersHidden = useReview((s) => s.markersHidden);
  const panelOpen = useReview((s) => s.panelOpen);
  const review: MenuEntry[] = [
    {
      label: "Review Mode",
      shortcut: "⇧R",
      checked: reviewMode,
      onClick: () => useReview.getState().setReviewMode(!reviewMode),
    },
    {
      label: "Review Panel",
      shortcut: "N",
      checked: panelOpen,
      onClick: () => useReview.getState().setPanelOpen(!panelOpen),
    },
    {
      label: "Show Markers",
      checked: !markersHidden,
      onClick: () => useReview.getState().setMarkersHidden(!markersHidden),
    },
  ];

  const help: MenuEntry[] = [
    {
      label: "Keyboard Shortcuts",
      shortcut: "?",
      onClick: () => useUi.getState().setShortcutsOpen(true),
    },
    {
      label: "Wiki",
      onClick: () =>
        window.open("https://github.com/marko-koljancic/solarxy/wiki", "_blank", "noreferrer"),
    },
    { divider: true },
    {
      label: "About Solarxy Web",
      onClick: () => setAboutOpen(true),
    },
  ];

  return (
    <nav className="menu-bar">
      <MenuItem title="File" entries={file} />
      <MenuItem title="Edit" entries={edit} />
      <MenuItem title="View" entries={view} />
      <MenuItem title="Desks" entries={desks} />
      <MenuItem title="Review" entries={review} />
      <MenuItem title="Help" entries={help} />
      {envOpen && <EnvironmentModal onClose={() => setEnvOpen(false)} />}
      {aboutOpen && <AboutModal onClose={() => setAboutOpen(false)} />}
      {deskSaveOpen && <DeskSaveModal onClose={() => setDeskSaveOpen(false)} />}
      {confirmNew && (
        <ConfirmDialog
          title="New scene"
          message="Discard unsaved changes and start a new scene?"
          confirmLabel="Discard & New"
          onConfirm={() => {
            setConfirmNew(false);
            void newScene();
          }}
          onCancel={() => setConfirmNew(false)}
        />
      )}
      <input
        ref={importRef}
        type="file"
        multiple
        accept=".obj,.mtl,.gltf,.glb,.stl,.ply,.bin,.slxy,.png,.jpg,.jpeg,.webp"
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
