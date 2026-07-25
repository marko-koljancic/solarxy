// UI chrome state: the dock layout, canvas chrome toggles, and transient modal
// flags. Persisted to localStorage; pure presentation, never document truth.
// Theme moved to the preferences store (store/prefs.ts) in W4.
//
// retired the hand-rolled layout state (splitPct, viewportSide,
// propertiesDock, drawerHeight, drawerWidth, drawerCollapsed, viewportMaximized):
// dockview owns all of it now, and `dockLayout` is the one persisted arrangement.
// The legacy keys are still READ once, to migrate an existing user's arrangement
// forward, and never written again.

import { create } from "zustand";

import type { SerializedDockview } from "dockview-react";
import type { LegacyArrangement } from "../dock/layouts";
import type { SidecarRefs } from "../engine/sidecars";
import type { GraphContext } from "../engine/types";

const FLOW_CHROME_KEY = "solarxy.ui.flowChrome";
const EDGE_STYLE_KEY = "solarxy.ui.edgeStyle";
const DOCK_LAYOUT_KEY = "solarxy.ui.dockLayout";
const FLOW_VIEW_KEY = "solarxy.ui.flowView";
const PANE_COLORS_KEY = "solarxy.ui.paneColors";

// Retired; read once by loadLegacyArrangement, never written.
const LEGACY_SPLIT_KEY = "solarxy.ui.splitPct";
const LEGACY_ARRANGEMENT_KEY = "solarxy.ui.arrangement";

/** Connection styles carried from Minimystix (its "step" id is renamed to
 * the honest smoothStep; the path function was smooth-step all along).
 * Cycle order (S key) is array order. A browser preference, deliberately
 * not captured by Desks. */
export const EDGE_STYLES = ["bezier", "straight", "simpleBezier", "smoothStep"] as const;
export type EdgeStyle = (typeof EDGE_STYLES)[number];
export const EDGE_STYLE_LABELS: Record<EdgeStyle, string> = {
  bezier: "Bezier",
  straight: "Straight",
  simpleBezier: "Simple Bezier",
  smoothStep: "Smooth Step",
};

/** Reads the pre-Phase-10 arrangement so a returning user's shell comes back the
 * way they left it instead of snapping to the default. Returns null once the
 * user has a dock layout (the normal case) or has never had either. */
export function loadLegacyArrangement(): LegacyArrangement | null {
  if (typeof localStorage === "undefined") return null;
  const raw = localStorage.getItem(LEGACY_ARRANGEMENT_KEY);
  const rawSplit = localStorage.getItem(LEGACY_SPLIT_KEY);
  // Absence is the common case (a fresh user, or one already migrated). Test
  // the stored STRINGS, not Number(...) of them: Number(null) is 0, which is
  // perfectly finite and would make every fresh user look like a legacy one.
  if (raw === null && rawSplit === null) return null;
  const split = Number(rawSplit);
  try {
    const parsed = raw ? (JSON.parse(raw) as LegacyArrangement) : {};
    return {
      ...parsed,
      splitPct: rawSplit !== null && Number.isFinite(split) && split > 0 ? split : undefined,
    };
  } catch {
    return null;
  }
}

/** A pending missing-sidecars prompt (multi-file model import preflight):
 * the primary model references companion files that are not staged. The
 * modal stages what the user adds and then runs the deferred completion. */
export interface SidecarPrompt {
  primaryName: string;
  primaryHash: string;
  missing: SidecarRefs;
  complete:
    | { kind: "setParam"; ctx: GraphContext; node: number; key: string }
    | { kind: "createImportNode" };
}

interface UiState {
  /** The serialized dockview arrangement. The single source of truth
   * for the shell's geometry; `null` before the first layout settles. */
  dockLayout: SerializedDockview | null;
  /** The generated keyboard-shortcuts modal (not persisted). */
  shortcutsOpen: boolean;
  /** The preferences modal (not persisted). */
  prefsOpen: boolean;
  /** The screenshot modal (not persisted). */
  screenshotOpen: boolean;
  /** Resolution preset for the next screenshot-modal open (the render
   * node's Render button); consumed once by the modal. */
  screenshotPreset: { width: number; height: number } | null;
  /** The turntable-export modal (not persisted). */
  turntableOpen: boolean;
  /** The node palette (not persisted; Tab and the Add menu toggle it). */
  paletteOpen: boolean;
  /** A fatal boot failure (WebGPU unavailable, wasm init error). */
  bootError: string | null;
  /** Node-canvas chrome toggles (G / M / C; Minimystix defaults). */
  showFlowGrid: boolean;
  showMinimap: boolean;
  showFlowControls: boolean;
  /** Snap node drags to the 18px canvas grid (View menu; default off). */
  snapToGrid: boolean;
  /** Connection style for canvas edges (S cycles; View menu selects). */
  edgeStyle: EdgeStyle;
  /** Per-context graph-or-list node view, keyed by ctxKey. Persisted;
   * ctx keys embed document-scoped node ids, so a stale subflow entry can
   * carry across documents. Accepted: it is only a view preference. */
  flowView: Record<string, "graph" | "list">;
  /** A pending inline-rename request (F2): the node whose label editor
   * should open. Consumed by whichever node view is mounted. */
  renameRequest: number | null;
  /** The missing-sidecars import prompt (not persisted). */
  sidecarPrompt: SidecarPrompt | null;
  /** Per-pane header tint, keyed by dockview panel id. Persisted. */
  paneColors: Record<string, string>;
  /** The asset the preview panel shows (item 2; not persisted). */
  assetPreview: { hash: string; name: string } | null;
  /** The Properties panel's active param tab, lifted here so the panel's
   * menu bar can read it (Reset Current Tab). Not persisted; the panel
   * falls back to the first tab when the selection lacks this group. */
  paramTab: string;
  setDockLayout: (layout: SerializedDockview) => void;
  setShortcutsOpen: (open: boolean) => void;
  setPrefsOpen: (open: boolean) => void;
  setScreenshotOpen: (open: boolean) => void;
  setScreenshotPreset: (preset: { width: number; height: number } | null) => void;
  setTurntableOpen: (open: boolean) => void;
  setPaletteOpen: (open: boolean) => void;
  setBootError: (message: string) => void;
  toggleFlowChrome: (
    key: "showFlowGrid" | "showMinimap" | "showFlowControls" | "snapToGrid",
  ) => void;
  setEdgeStyle: (style: EdgeStyle) => void;
  cycleEdgeStyle: () => void;
  setFlowView: (ctxKey: string, view: "graph" | "list") => void;
  setRenameRequest: (node: number | null) => void;
  setSidecarPrompt: (prompt: SidecarPrompt | null) => void;
  /** Sets (or clears, with null) a pane's header tint and persists it. */
  setPaneColor: (id: string, color: string | null) => void;
  setAssetPreview: (asset: { hash: string; name: string } | null) => void;
  setParamTab: (tab: string) => void;
}

/** The persisted dock arrangement. A corrupt blob is discarded here rather than
 * handed to `fromJSON`, which would wedge the dock (dockview #341). */
export function loadDockLayout(): SerializedDockview | null {
  if (typeof localStorage === "undefined") return null;
  const raw = localStorage.getItem(DOCK_LAYOUT_KEY);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as SerializedDockview;
    return parsed && typeof parsed === "object" && "grid" in parsed ? parsed : null;
  } catch {
    return null;
  }
}

function loadFlowChrome(): {
  showFlowGrid: boolean;
  showMinimap: boolean;
  showFlowControls: boolean;
  snapToGrid: boolean;
} {
  // Minimystix defaults: grid on, minimap OFF, controls on, snap OFF.
  const defaults = {
    showFlowGrid: true,
    showMinimap: false,
    showFlowControls: true,
    snapToGrid: false,
  };
  if (typeof localStorage === "undefined") return defaults;
  try {
    const raw = localStorage.getItem(FLOW_CHROME_KEY);
    return raw ? { ...defaults, ...(JSON.parse(raw) as object) } : defaults;
  } catch {
    return defaults;
  }
}

/** The persisted per-context graph/list choice; malformed entries are
 * dropped rather than trusted. */
export function loadFlowView(): Record<string, "graph" | "list"> {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(FLOW_VIEW_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, "graph" | "list"> = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (v === "graph" || v === "list") out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

export function loadEdgeStyle(): EdgeStyle {
  if (typeof localStorage === "undefined") return "bezier";
  const raw = localStorage.getItem(EDGE_STYLE_KEY);
  return (EDGE_STYLES as readonly string[]).includes(raw ?? "") ? (raw as EdgeStyle) : "bezier";
}

/** The persisted per-pane header tint, keyed by dockview panel id.
 * Pure chrome; malformed entries are dropped. */
export function loadPaneColors(): Record<string, string> {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(PANE_COLORS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (typeof v === "string") out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

export const useUi = create<UiState>((set) => {
  return {
    ...loadFlowChrome(),
    dockLayout: loadDockLayout(),
    edgeStyle: loadEdgeStyle(),
    shortcutsOpen: false,
    prefsOpen: false,
    screenshotOpen: false,
    screenshotPreset: null,
    turntableOpen: false,
    paletteOpen: false,
    bootError: null,
    flowView: loadFlowView(),
    renameRequest: null,
    sidecarPrompt: null,
    paneColors: loadPaneColors(),
    assetPreview: null,
    paramTab: "",
    setParamTab: (tab) => set({ paramTab: tab }),
    setDockLayout: (layout) => {
      localStorage.setItem(DOCK_LAYOUT_KEY, JSON.stringify(layout));
      set({ dockLayout: layout });
    },
    setShortcutsOpen: (open) => set({ shortcutsOpen: open }),
    setPrefsOpen: (open) => set({ prefsOpen: open }),
    setScreenshotOpen: (open) => set({ screenshotOpen: open }),
    setScreenshotPreset: (preset) => set({ screenshotPreset: preset }),
    setTurntableOpen: (open) => set({ turntableOpen: open }),
    setPaletteOpen: (open) => set({ paletteOpen: open }),
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
            snapToGrid: next.snapToGrid,
          }),
        );
        return { [key]: !s[key] };
      }),
    setEdgeStyle: (style) => {
      localStorage.setItem(EDGE_STYLE_KEY, style);
      set({ edgeStyle: style });
    },
    cycleEdgeStyle: () =>
      set((s) => {
        const next = EDGE_STYLES[(EDGE_STYLES.indexOf(s.edgeStyle) + 1) % EDGE_STYLES.length];
        localStorage.setItem(EDGE_STYLE_KEY, next);
        return { edgeStyle: next };
      }),
    setFlowView: (key, view) =>
      set((s) => {
        const flowView = { ...s.flowView, [key]: view };
        localStorage.setItem(FLOW_VIEW_KEY, JSON.stringify(flowView));
        return { flowView };
      }),
    setRenameRequest: (node) => set({ renameRequest: node }),
    setSidecarPrompt: (prompt) => set({ sidecarPrompt: prompt }),
    setPaneColor: (id, color) =>
      set((s) => {
        const paneColors = { ...s.paneColors };
        if (color) paneColors[id] = color;
        else delete paneColors[id];
        localStorage.setItem(PANE_COLORS_KEY, JSON.stringify(paneColors));
        return { paneColors };
      }),
    setAssetPreview: (asset) => set({ assetPreview: asset }),
  };
});
