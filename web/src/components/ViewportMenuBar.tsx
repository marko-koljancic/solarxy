// The viewport menu bar: a slim DOM bar above the canvas holding
// viewport-global actions in a View menu. Per-pane state stays on the ghost
// toolbars floating over the scene; this bar owns only what applies to the
// viewport as a whole.

import { useState } from "react";
import { toggleMaximize } from "../dock/api";
import { cameraCommand, setActivePane, setViewLayout } from "../engine/session";
import type { ViewLayout } from "../engine/types";
import { usePrefs, type GizmoOrientation } from "../store/prefs";
import { useUi } from "../store/ui";
import { useViewState } from "../store/viewState";
import { EnvironmentModal } from "./EnvironmentModal";
import { MenuItem, type MenuEntry } from "./menu/MenuItem";

const PANE_LAYOUTS: { layout: ViewLayout; label: string; shortcut: string }[] = [
  { layout: "single", label: "Single", shortcut: "F1" },
  { layout: "splitVertical", label: "Split Vertical", shortcut: "F2" },
  { layout: "splitHorizontal", label: "Split Horizontal", shortcut: "F3" },
  { layout: "quad", label: "Quad", shortcut: "F4" },
  { layout: "threeLeftBig", label: "Three Left Big", shortcut: "F5" },
];

const ORIENTATIONS: { value: GizmoOrientation; label: string }[] = [
  { value: "world", label: "World Axes" },
  { value: "local", label: "Object Axes" },
];

export function ViewportMenuBar() {
  const view = useViewState((s) => s.view);
  const orientation = usePrefs((s) => s.prefs.viewport.orientation);
  const [envOpen, setEnvOpen] = useState(false);

  const entries: MenuEntry[] = [
    {
      label: "Fit View",
      shortcut: "Z",
      onClick: () => {
        if (!view) return;
        setActivePane(view.activePane);
        cameraCommand(view.activePane, { kind: "fit" });
      },
    },
    {
      label: "Pane Layout",
      submenu: PANE_LAYOUTS.map((p) => ({
        label: p.label,
        shortcut: p.shortcut,
        checked: view?.layout === p.layout,
        onClick: () => setViewLayout(p.layout),
      })),
    },
    {
      // Writes the same pref the X hotkey and the Preferences Viewport tab do,
      // so the three can never disagree about which frame the handles are in.
      label: "Gizmo Orientation",
      submenu: ORIENTATIONS.map((o) => ({
        label: o.label,
        checked: orientation === o.value,
        onClick: () => {
          const { prefs, setPrefs } = usePrefs.getState();
          setPrefs({ ...prefs, viewport: { ...prefs.viewport, orientation: o.value } });
        },
      })),
    },
    { divider: true },
    { label: "Environment...", onClick: () => setEnvOpen(true) },
    {
      label: "Save Screenshot...",
      shortcut: "C",
      onClick: () => useUi.getState().setScreenshotOpen(true),
    },
    {
      label: "Export Turntable...",
      onClick: () => useUi.getState().setTurntableOpen(true),
    },
    { divider: true },
    {
      // Real dock maximize, on the viewport's own group. Esc restores
 // (the keymap's cancel ladder). The interim toggle is gone.
      label: "Maximize Panel",
      shortcut: "Esc to restore",
      onClick: () => toggleMaximize("viewport"),
    },
  ];

  return (
    <nav className="menu-bar viewport-menu">
      <MenuItem title="View" entries={entries} />
      {envOpen && <EnvironmentModal onClose={() => setEnvOpen(false)} />}
    </nav>
  );
}
