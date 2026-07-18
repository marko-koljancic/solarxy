// Desks: a desk is a named snapshot of the
// app ARRANGEMENT, never document state. Since the arrangement IS the
// dock layout, plus the canvas chrome toggles and the viewport pane layout.
// Applying one drives the dock and the host view layout; scene files stay
// portable.
//
// Two layout shapes, deliberately (see dock/layouts.ts): presets and migrated
// legacy desks are RECIPES, which survive a hand-edit and a dockview bump; desks
// the user saves are SERIALIZED dockview layouts, because that is the whole
// point of saving one.

import { create } from "zustand";
import { applyLayout, captureLayout } from "../dock/api";
import { sanitizeRecipe, type DeskLayout } from "../dock/layouts";
import { setViewLayout } from "../engine/session";
import type { ViewLayout } from "../engine/types";
import { useViewState } from "./viewState";
import { useUi } from "./ui";

const DESKS_KEY = "solarxy.desks";

export interface DeskSnapshot {
  name: string;
  layout: DeskLayout;
  showFlowGrid: boolean;
  showMinimap: boolean;
  showFlowControls: boolean;
  viewLayout: ViewLayout;
}

/** Built-in desks; user desks may shadow a preset name (user wins). */
export const DESK_PRESETS: DeskSnapshot[] = [
  {
    name: "Default",
    layout: {
      kind: "recipe",
      recipe: { viewportSide: "left", propertiesDock: "bottom", splitPct: 55, review: false },
    },
    showFlowGrid: true,
    showMinimap: false,
    showFlowControls: true,
    viewLayout: "single",
  },
  {
    name: "Modeling",
    layout: {
      kind: "recipe",
      recipe: { viewportSide: "left", propertiesDock: "right", splitPct: 50, review: false },
    },
    showFlowGrid: true,
    showMinimap: false,
    showFlowControls: true,
    viewLayout: "single",
  },
  {
    name: "Review",
    layout: {
      // The Review preset now ships with the Review panel docked.
      kind: "recipe",
      recipe: { viewportSide: "left", propertiesDock: "bottom", splitPct: 70, review: true },
    },
    showFlowGrid: false,
    showMinimap: false,
    showFlowControls: false,
    viewLayout: "quad",
  },
];

/** The current arrangement as a desk snapshot. Maximize is not captured:
 * `SerializedDockview` carries no maximized state, so a desk saved while
 * maximized restores the underlying grid, which is the intended contract. */
export function captureDesk(
  name: string,
  ui: { showFlowGrid: boolean; showMinimap: boolean; showFlowControls: boolean },
  viewLayout: ViewLayout,
): DeskSnapshot | null {
  const json = captureLayout();
  if (!json) return null;
  return {
    name,
    layout: { kind: "serialized", json },
    showFlowGrid: ui.showFlowGrid,
    showMinimap: ui.showMinimap,
    showFlowControls: ui.showFlowControls,
    viewLayout,
  };
}

const VIEW_LAYOUTS: ViewLayout[] = [
  "single",
  "splitVertical",
  "splitHorizontal",
  "quad",
  "threeLeftBig",
];

/** The pre-Phase-10 desk shape, as stored by an existing user. */
interface LegacyDeskSnapshot {
  name: string;
  viewportSide?: "left" | "right";
  propertiesDock?: "bottom" | "right";
  splitPct?: number;
  showFlowGrid?: boolean;
  showMinimap?: boolean;
  showFlowControls?: boolean;
  viewLayout?: ViewLayout;
}

/** Coerces a stored desk (possibly hand-edited, possibly written by the
 * pre-docking shell) into a valid current snapshot. A legacy desk's arrangement
 * fields synthesize forward into the equivalent recipe. */
export function sanitizeDesk(desk: DeskSnapshot | LegacyDeskSnapshot): DeskSnapshot {
  const legacy = desk as LegacyDeskSnapshot;
  const current = desk as DeskSnapshot;

  const layout: DeskLayout =
    current.layout?.kind === "serialized" && current.layout.json
      ? current.layout
      : {
          kind: "recipe",
          recipe: sanitizeRecipe(
            current.layout?.kind === "recipe"
              ? current.layout.recipe
              : {
                  // Forward migration: the old shell could express nothing a
                  // recipe cannot.
                  viewportSide: legacy.viewportSide,
                  propertiesDock: legacy.propertiesDock,
                  splitPct: legacy.splitPct,
                  review: false,
                },
          ),
        };

  return {
    name: String(desk.name ?? "Desk"),
    layout,
    showFlowGrid: legacy.showFlowGrid !== false,
    showMinimap: legacy.showMinimap === true,
    showFlowControls: legacy.showFlowControls !== false,
    viewLayout: VIEW_LAYOUTS.includes(legacy.viewLayout as ViewLayout)
      ? (legacy.viewLayout as ViewLayout)
      : "single",
  };
}

function loadDesks(): DeskSnapshot[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(DESKS_KEY);
    const parsed = raw ? (JSON.parse(raw) as DeskSnapshot[]) : [];
    return Array.isArray(parsed) ? parsed.map(sanitizeDesk) : [];
  } catch {
    return [];
  }
}

interface DesksStore {
  /** User-saved desks (presets live in DESK_PRESETS). */
  desks: DeskSnapshot[];
  saveCurrent: (name: string) => void;
  apply: (name: string) => void;
  remove: (name: string) => void;
}

export const useDesks = create<DesksStore>((set, get) => ({
  desks: loadDesks(),
  saveCurrent: (name) => {
    const ui = useUi.getState();
    const layout = useViewState.getState().view?.layout ?? "single";
    const desk = captureDesk(name.trim(), ui, layout);
    if (!desk) return;
    const desks = [...get().desks.filter((d) => d.name !== desk.name), desk];
    localStorage.setItem(DESKS_KEY, JSON.stringify(desks));
    set({ desks });
  },
  apply: (name) => {
    const desk =
      get().desks.find((d) => d.name === name) ?? DESK_PRESETS.find((d) => d.name === name);
    if (!desk) return;
    const d = sanitizeDesk(desk);
    const ui = useUi.getState();
    applyLayout(d.layout);
    if (ui.showFlowGrid !== d.showFlowGrid) ui.toggleFlowChrome("showFlowGrid");
    if (ui.showMinimap !== d.showMinimap) ui.toggleFlowChrome("showMinimap");
    if (ui.showFlowControls !== d.showFlowControls) ui.toggleFlowChrome("showFlowControls");
    setViewLayout(d.viewLayout);
  },
  remove: (name) => {
    const desks = get().desks.filter((d) => d.name !== name);
    localStorage.setItem(DESKS_KEY, JSON.stringify(desks));
    set({ desks });
  },
}));
