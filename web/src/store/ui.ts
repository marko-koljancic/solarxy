// UI chrome state: the resizable-layout state (viewport/canvas split,
// properties drawer, viewport maximize) and transient modal flags.
// Persisted to localStorage; pure presentation, never document truth.
// Theme moved to the preferences store (store/prefs.ts) in Phase 7 W4.

import { create } from "zustand";

const SPLIT_KEY = "solarxy.ui.splitPct";
const FLOW_CHROME_KEY = "solarxy.ui.flowChrome";
const DRAWER_KEY = "solarxy.ui.drawerHeight";
const DRAWER_WIDTH_KEY = "solarxy.ui.drawerWidth";
const ARRANGEMENT_KEY = "solarxy.ui.arrangement";

/** Clamp bounds carried from Minimystix (variables.css / PropertiesDrawer). */
export const SPLIT_MIN_PCT = 20;
export const SPLIT_MAX_PCT = 80;
export const DRAWER_MIN_PX = 100;
/** The drawer may take most of the window, not a fixed 600px (the
 * "properties cannot grow" complaint): capped at 85 percent of the
 * viewport height at clamp time. */
export function drawerMaxPx(): number {
  if (typeof window === "undefined") return 600;
  return Math.max(300, Math.round(window.innerHeight * 0.85));
}

export function clampSplit(pct: number): number {
  return Math.min(SPLIT_MAX_PCT, Math.max(SPLIT_MIN_PCT, pct));
}

export function clampDrawer(px: number): number {
  return Math.min(drawerMaxPx(), Math.max(DRAWER_MIN_PX, px));
}

/** Right-docked properties width bounds. */
export const DRAWER_WIDTH_MIN_PX = 240;
export function drawerWidthMaxPx(): number {
  if (typeof window === "undefined") return 600;
  return Math.max(DRAWER_WIDTH_MIN_PX, Math.round(window.innerWidth * 0.5));
}

export function clampDrawerWidth(px: number): number {
  return Math.min(drawerWidthMaxPx(), Math.max(DRAWER_WIDTH_MIN_PX, px));
}

export type ViewportSide = "left" | "right";
export type PropertiesDock = "bottom" | "right";

interface UiState {
  /** Viewport share of the horizontal split, in percent (20-80). */
  splitPct: number;
  drawerHeight: number;
  /** Right-docked properties width, px. */
  drawerWidth: number;
  drawerCollapsed: boolean;
  viewportMaximized: boolean;
  /** Desk arrangement (Phase 7b D3): which side the 3D viewport sits on
   * and where the properties panel docks. */
  viewportSide: ViewportSide;
  propertiesDock: PropertiesDock;
  /** The generated keyboard-shortcuts modal (not persisted). */
  shortcutsOpen: boolean;
  /** The preferences modal (not persisted). */
  prefsOpen: boolean;
  /** The screenshot modal (not persisted). */
  screenshotOpen: boolean;
  /** A fatal boot failure (WebGPU unavailable, wasm init error). */
  bootError: string | null;
  /** Node-canvas chrome toggles (G / M / C; Minimystix defaults). */
  showFlowGrid: boolean;
  showMinimap: boolean;
  showFlowControls: boolean;
  /** Per-context graph-or-list node view, keyed by ctxKey (in-memory). */
  flowView: Record<string, "graph" | "list">;
  setSplitPct: (pct: number) => void;
  setDrawerHeight: (px: number) => void;
  toggleDrawerCollapsed: () => void;
  toggleViewportMaximized: () => void;
  setShortcutsOpen: (open: boolean) => void;
  setPrefsOpen: (open: boolean) => void;
  setScreenshotOpen: (open: boolean) => void;
  setBootError: (message: string) => void;
  toggleFlowChrome: (key: "showFlowGrid" | "showMinimap" | "showFlowControls") => void;
  setFlowView: (ctxKey: string, view: "graph" | "list") => void;
  setDrawerWidth: (px: number) => void;
  setArrangement: (a: Partial<{ viewportSide: ViewportSide; propertiesDock: PropertiesDock }>) => void;
}

function loadArrangement(): { viewportSide: ViewportSide; propertiesDock: PropertiesDock } {
  const defaults = { viewportSide: "left" as ViewportSide, propertiesDock: "bottom" as PropertiesDock };
  if (typeof localStorage === "undefined") return defaults;
  try {
    const raw = localStorage.getItem(ARRANGEMENT_KEY);
    return raw ? { ...defaults, ...(JSON.parse(raw) as object) } : defaults;
  } catch {
    return defaults;
  }
}

function loadFlowChrome(): { showFlowGrid: boolean; showMinimap: boolean; showFlowControls: boolean } {
  // Minimystix defaults: grid on, minimap OFF, controls on.
  const defaults = { showFlowGrid: true, showMinimap: false, showFlowControls: true };
  if (typeof localStorage === "undefined") return defaults;
  try {
    const raw = localStorage.getItem(FLOW_CHROME_KEY);
    return raw ? { ...defaults, ...(JSON.parse(raw) as object) } : defaults;
  } catch {
    return defaults;
  }
}

function loadNumber(key: string, fallback: number): number {
  if (typeof localStorage === "undefined") return fallback;
  const raw = Number(localStorage.getItem(key));
  return Number.isFinite(raw) && raw > 0 ? raw : fallback;
}

export const useUi = create<UiState>((set) => {
  return {
    ...loadFlowChrome(),
    ...loadArrangement(),
    splitPct: clampSplit(loadNumber(SPLIT_KEY, 55)),
    drawerHeight: clampDrawer(loadNumber(DRAWER_KEY, 280)),
    drawerWidth: clampDrawerWidth(loadNumber(DRAWER_WIDTH_KEY, 340)),
    drawerCollapsed: false,
    viewportMaximized: false,
    shortcutsOpen: false,
    prefsOpen: false,
    screenshotOpen: false,
    bootError: null,
    flowView: {},
    setSplitPct: (pct) => {
      const clamped = clampSplit(pct);
      localStorage.setItem(SPLIT_KEY, String(clamped));
      set({ splitPct: clamped });
    },
    setDrawerHeight: (px) => {
      const clamped = clampDrawer(px);
      localStorage.setItem(DRAWER_KEY, String(clamped));
      set({ drawerHeight: clamped });
    },
    toggleDrawerCollapsed: () => set((s) => ({ drawerCollapsed: !s.drawerCollapsed })),
    toggleViewportMaximized: () => set((s) => ({ viewportMaximized: !s.viewportMaximized })),
    setShortcutsOpen: (open) => set({ shortcutsOpen: open }),
    setPrefsOpen: (open) => set({ prefsOpen: open }),
    setScreenshotOpen: (open) => set({ screenshotOpen: open }),
    setBootError: (message) => set({ bootError: message }),
    toggleFlowChrome: (key) =>
      set((s) => {
        const next = { ...s, [key]: !s[key] };
        localStorage.setItem(
          FLOW_CHROME_KEY,
          JSON.stringify({
            showFlowGrid: next.showFlowGrid,
            showMinimap: next.showMinimap,
            showFlowControls: next.showFlowControls,
          }),
        );
        return { [key]: !s[key] };
      }),
    setFlowView: (key, view) => set((s) => ({ flowView: { ...s.flowView, [key]: view } })),
    setDrawerWidth: (px) => {
      const clamped = clampDrawerWidth(px);
      localStorage.setItem(DRAWER_WIDTH_KEY, String(clamped));
      set({ drawerWidth: clamped });
    },
    setArrangement: (a) =>
      set((s) => {
        const next = {
          viewportSide: a.viewportSide ?? s.viewportSide,
          propertiesDock: a.propertiesDock ?? s.propertiesDock,
        };
        localStorage.setItem(ARRANGEMENT_KEY, JSON.stringify(next));
        return next;
      }),
  };
});
