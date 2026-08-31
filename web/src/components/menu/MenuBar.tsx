// The global menu bar (File / Edit / Desks / Review / Help), slimmed in
// Pane-scoped chrome lives on the pane bars (the node-pane Add
// and View menus, the viewport View menu) and Theme lives in Preferences.
// Every item dispatches through the same session functions the keyboard
// uses.

import { useRef, useState } from "react";
import {
  copySelection,
  dispatch,
  duplicateSelection,
  explicitSave,
  importDroppedFiles,
  openSampleScene,
  openScene,
  paste,
} from "../../engine/session";
import { clearAutosaves } from "../../persistence/opfs";
import { ConfirmDialog } from "../ConfirmDialog";
import { AboutModal } from "../AboutModal";
import { isAssetsPanelOpen, isAttributesPanelOpen, isNodesPanelOpen, isPropertiesPanelOpen, isTexturePanelOpen, isTextPanelOpen, isTreePanelOpen, setAssetsPanelOpen, setAttributesPanelOpen, setNodesPanelOpen, setPropertiesPanelOpen, setReviewPanelOpen, setTexturePanelOpen, setTextPanelOpen, setTreePanelOpen } from "../../dock/api";
import { selectGraph, useMirror } from "../../store/mirror";
import { DESK_PRESETS, useDesks } from "../../store/desks";
import { useReview } from "../../store/review";
import { useUi } from "../../store/ui";
import { DeskSaveModal } from "../DeskSaveModal";
import { WebBundleModal } from "../WebBundleModal";
import { TOURS } from "../tour/steps";
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

/** The bundled sample scenes (web/public/samples/), each a fully
 * parametric .slxy whose note nodes explain the workflow it teaches. A
 * Rust fixture test cooks every committed file, so a node change that
 * breaks one fails CI rather than a learner. */
const SAMPLE_SCENES: { label: string; file: string }[] = [
  { label: "Modeling Basics", file: "modeling-basics.slxy" },
  { label: "Copy & Scatter", file: "copy-and-scatter.slxy" },
  { label: "Attributes & Displace", file: "attributes-and-displace.slxy" },
  { label: "Texture to Material", file: "texture-to-material.slxy" },
  { label: "Lights, Camera, Review", file: "lights-camera-review.slxy" },
  { label: "Animated Field", file: "animated-field.slxy" },
  { label: "Procedural Look-dev", file: "procedural-lookdev.slxy" },
  // The flagship: everything above composed into one scene.
  { label: "The Orrery", file: "the-orrery.slxy" },
  // Last because it teaches the renderer rather than the node graph: it is
  // the one sample whose point is what the traced still does that the
  // viewport preview cannot.
  { label: "Cornell Box", file: "cornell-box.slxy" },
];

export function MenuBar() {
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const importRef = useRef<HTMLInputElement>(null);
  const [confirmNew, setConfirmNew] = useState(false);
  const [confirmSample, setConfirmSample] = useState<{ label: string; file: string } | null>(null);
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
    {
      label: "Sample Scenes",
      submenu: SAMPLE_SCENES.map((s) => ({
        label: s.label,
        onClick: () => {
          if (useMirror.getState().dirty) setConfirmSample(s);
          else void openSampleScene(s.file);
        },
      })),
    },
    { label: "Save Scene", shortcut: `${MOD}S`, onClick: () => void explicitSave() },
    { divider: true },
    { label: "Import Model...", onClick: () => importRef.current?.click() },
    { divider: true },
    { label: "Export web bundle...", onClick: () => setBundleOpen(true) },
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

 // Desks: presets + user-saved dock
  // arrangements, save-as, delete. Applying never touches the document, only
  // chrome. The old "Viewport on Left / Properties at Bottom" radios are gone:
  // the user drags panels wherever they want now, and a desk captures it.
  const userDesks = useDesks((s) => s.desks);
  const [deskSaveOpen, setDeskSaveOpen] = useState(false);
  const [bundleOpen, setBundleOpen] = useState(false);
  // Subscribe to the persisted dock layout (debounced 400 ms) so the panel
  // checkmarks below refresh after tabs close or panels reopen.
  const dockLayout = useUi((s) => s.dockLayout);
  void dockLayout;
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
    { label: "Save Current As...", onClick: () => setDeskSaveOpen(true) },
    {
      label: "Delete Desk",
      disabled: userDesks.length === 0,
      submenu: userDesks.map((d) => ({
        label: d.name,
        onClick: () => useDesks.getState().remove(d.name),
      })),
    },
    { divider: true },
    // Panels: presence in the dock is the open state (the Review-panel
    // pattern). Nodes and Properties are here so a closed core panel can be
    // reopened without applying a whole desk.
    { label: "Nodes Panel", checked: isNodesPanelOpen(), onClick: () => setNodesPanelOpen(!isNodesPanelOpen()) },
    { label: "Properties Panel", checked: isPropertiesPanelOpen(), onClick: () => setPropertiesPanelOpen(!isPropertiesPanelOpen()) },
    { label: "Tree Panel", checked: isTreePanelOpen(), onClick: () => setTreePanelOpen(!isTreePanelOpen()) },
    { label: "Text Panel", checked: isTextPanelOpen(), onClick: () => setTextPanelOpen(!isTextPanelOpen()) },
    { label: "Assets Panel", checked: isAssetsPanelOpen(), onClick: () => setAssetsPanelOpen(!isAssetsPanelOpen()) },
    { label: "Texture Viewer", checked: isTexturePanelOpen(), onClick: () => setTexturePanelOpen(!isTexturePanelOpen()) },
    { label: "Attributes Panel", checked: isAttributesPanelOpen(), onClick: () => setAttributesPanelOpen(!isAttributesPanelOpen()) },
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
      // The dock owns the panel's existence; the store mirrors it.
      onClick: () => setReviewPanelOpen(!panelOpen),
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
      // Replayable, so skipping the first-run tour is not a one-way door.
      // The submenu lists the catalog; each entry names its tour in the
      // event detail (a plain Event still replays the overview).
      label: "Take a Tour",
      submenu: TOURS.map((t) => ({
        label: t.title,
        onClick: () =>
          window.dispatchEvent(new CustomEvent("solarxy:tour", { detail: { id: t.id } })),
      })),
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
      <MenuItem title="Desks" entries={desks} />
      <MenuItem title="Review" entries={review} />
      <MenuItem title="Help" entries={help} />
      {aboutOpen && <AboutModal onClose={() => setAboutOpen(false)} />}
      {deskSaveOpen && <DeskSaveModal onClose={() => setDeskSaveOpen(false)} />}
      {bundleOpen && <WebBundleModal onClose={() => setBundleOpen(false)} />}
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
      {confirmSample && (
        <ConfirmDialog
          title="Open sample scene"
          message={`Discard unsaved changes and open ${confirmSample.label}?`}
          confirmLabel="Discard & Open"
          onConfirm={() => {
            const picked = confirmSample;
            setConfirmSample(null);
            void openSampleScene(picked.file);
          }}
          onCancel={() => setConfirmSample(null)}
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
