// Desks (Phase 7b D3, maintainer decision 1: presets + named desks,
// hand-rolled): a desk is a named snapshot of the app ARRANGEMENT, never
// document state: viewport side, properties dock, splitter sizes, flow
// chrome toggles, and the viewport pane layout name. Applying one mutates
// the ui store and the host view layout; scene files stay portable.

import { create } from "zustand";
import { setViewLayout } from "../engine/session";
import type { ViewLayout } from "../engine/types";
import { useViewState } from "./viewState";
import {
  clampDrawer,
  clampDrawerWidth,
  clampSplit,
  useUi,
  type PropertiesDock,
  type ViewportSide,
} from "./ui";

const DESKS_KEY = "solarxy.desks";

export interface DeskSnapshot {
  name: string;
  viewportSide: ViewportSide;
  propertiesDock: PropertiesDock;
  splitPct: number;
  drawerHeight: number;
  drawerWidth: number;
  showFlowGrid: boolean;
  showMinimap: boolean;
  showFlowControls: boolean;
  viewLayout: ViewLayout;
}

/** Built-in desks; user desks may shadow a preset name (user wins). */
export const DESK_PRESETS: DeskSnapshot[] = [
  {
    name: "Default",
    viewportSide: "left",
    propertiesDock: "bottom",
    splitPct: 55,
    drawerHeight: 280,
    drawerWidth: 340,
    showFlowGrid: true,
    showMinimap: false,
    showFlowControls: true,
    viewLayout: "single",
  },
  {
    name: "Modeling",
    viewportSide: "left",
    propertiesDock: "right",
    splitPct: 50,
    drawerHeight: 280,
    drawerWidth: 340,
    showFlowGrid: true,
    showMinimap: false,
    showFlowControls: true,
    viewLayout: "single",
  },
  {
    name: "Review",
    viewportSide: "left",
    propertiesDock: "bottom",
    splitPct: 70,
    drawerHeight: 220,
    drawerWidth: 340,
    showFlowGrid: false,
    showMinimap: false,
    showFlowControls: false,
    viewLayout: "quad",
  },
];

/** The current arrangement as a desk snapshot. Pure over the two stores'
 * states plus the host layout, for tests. */
export function captureDesk(
  name: string,
  ui: {
    viewportSide: ViewportSide;
    propertiesDock: PropertiesDock;
    splitPct: number;
    drawerHeight: number;
    drawerWidth: number;
    showFlowGrid: boolean;
    showMinimap: boolean;
    showFlowControls: boolean;
  },
  viewLayout: ViewLayout,
): DeskSnapshot {
  return {
    name,
    viewportSide: ui.viewportSide,
    propertiesDock: ui.propertiesDock,
    splitPct: ui.splitPct,
    drawerHeight: ui.drawerHeight,
    drawerWidth: ui.drawerWidth,
    showFlowGrid: ui.showFlowGrid,
    showMinimap: ui.showMinimap,
    showFlowControls: ui.showFlowControls,
    viewLayout,
  };
}

/** Clamps a (possibly hand-edited or stale) desk to valid bounds. */
export function sanitizeDesk(desk: DeskSnapshot): DeskSnapshot {
  return {
    ...desk,
    viewportSide: desk.viewportSide === "right" ? "right" : "left",
    propertiesDock: desk.propertiesDock === "right" ? "right" : "bottom",
    splitPct: clampSplit(desk.splitPct),
    drawerHeight: clampDrawer(desk.drawerHeight),
    drawerWidth: clampDrawerWidth(desk.drawerWidth),
  };
}

function loadDesks(): DeskSnapshot[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(DESKS_KEY);
    const parsed = raw ? (JSON.parse(raw) as DeskSnapshot[]) : [];
    return Array.isArray(parsed) ? parsed : [];
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
    ui.setArrangement({ viewportSide: d.viewportSide, propertiesDock: d.propertiesDock });
    ui.setSplitPct(d.splitPct);
    ui.setDrawerHeight(d.drawerHeight);
    ui.setDrawerWidth(d.drawerWidth);
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
