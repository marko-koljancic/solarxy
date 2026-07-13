// The view-state mirror: pane layout, per-pane display settings, pane
// rectangles, and the hover context flag. Rust (the wasm host) owns the
// truth; this store only mirrors ViewStateDto returns and HostEvents, and
// components mutate through the session's view actions.

import { create } from "zustand";
import type { EnvironmentState, PaneRectDto, ToolMode, ViewStateDto } from "../engine/types";

interface ViewStateStore {
  view: ViewStateDto | null;
  /** Whether the pointer is over the 3D viewport region (the cursor-hover
   * keymap context). */
  pointerOverViewport: boolean;
  /** Whether the pointer is over the node canvas (the "canvas" context). */
  pointerOverCanvas: boolean;
  /** The last UV overlap percentage (null before the first run or while a
   * fresh source is computing). */
  uvOverlapPct: number | null;
  /** Whether an overlap statistics readback is in flight. */
  uvOverlapPending: boolean;
  /** The host's environment (IBL mode + loaded HDRI identity). */
  environment: EnvironmentState | null;
  /** The active viewport tool. A mirror: Rust owns it (the drag loop reads it),
   * this drives the tool column's highlight. Not persisted. */
  toolMode: ToolMode;
  /** The live gizmo drag's delta text ("X +1.250 m"), or null when nothing is
   * dragging. Polled from the host once per frame. */
  gizmoReadout: string | null;
  setView: (view: ViewStateDto) => void;
  setPaneRects: (rects: PaneRectDto[]) => void;
  setActivePaneMirror: (pane: number) => void;
  setPointerOverViewport: (over: boolean) => void;
  setPointerOverCanvas: (over: boolean) => void;
  setUvOverlap: (pct: number | null, pending: boolean) => void;
  setEnvironment: (env: EnvironmentState) => void;
  setToolMode: (tool: ToolMode) => void;
  setGizmoReadout: (text: string | null) => void;
}

export const useViewState = create<ViewStateStore>((set) => ({
  view: null,
  pointerOverViewport: false,
  pointerOverCanvas: false,
  uvOverlapPct: null,
  uvOverlapPending: false,
  environment: null,
  toolMode: "select",
  gizmoReadout: null,
  setView: (view) => set({ view }),
  setPaneRects: (rects) =>
    set((s) => (s.view ? { view: { ...s.view, paneRects: rects } } : s)),
  setActivePaneMirror: (pane) =>
    set((s) => (s.view ? { view: { ...s.view, activePane: pane } } : s)),
  setPointerOverViewport: (over) => set({ pointerOverViewport: over }),
  setPointerOverCanvas: (over) => set({ pointerOverCanvas: over }),
  setUvOverlap: (pct, pending) => set({ uvOverlapPct: pct, uvOverlapPending: pending }),
  setEnvironment: (env) => set({ environment: env }),
  setToolMode: (tool) => set({ toolMode: tool }),
  setGizmoReadout: (text) =>
    // Guarded: this runs every frame, and an unconditional set would re-render
    // every readout consumer 60 times a second for no reason.
    set((s) => (s.gizmoReadout === text ? s : { gizmoReadout: text })),
}));
