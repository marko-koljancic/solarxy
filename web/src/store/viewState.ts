// The view-state mirror: pane layout, per-pane display settings, pane
// rectangles, and the hover context flag. Rust (the wasm host) owns the
// truth; this store only mirrors ViewStateDto returns and HostEvents, and
// components mutate through the session's view actions.

import { create } from "zustand";
import type { EnvironmentState, PaneRectDto, ViewStateDto } from "../engine/types";

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
  setView: (view: ViewStateDto) => void;
  setPaneRects: (rects: PaneRectDto[]) => void;
  setActivePaneMirror: (pane: number) => void;
  setPointerOverViewport: (over: boolean) => void;
  setPointerOverCanvas: (over: boolean) => void;
  setUvOverlap: (pct: number | null, pending: boolean) => void;
  setEnvironment: (env: EnvironmentState) => void;
}

export const useViewState = create<ViewStateStore>((set) => ({
  view: null,
  pointerOverViewport: false,
  pointerOverCanvas: false,
  uvOverlapPct: null,
  uvOverlapPending: false,
  environment: null,
  setView: (view) => set({ view }),
  setPaneRects: (rects) =>
    set((s) => (s.view ? { view: { ...s.view, paneRects: rects } } : s)),
  setActivePaneMirror: (pane) =>
    set((s) => (s.view ? { view: { ...s.view, activePane: pane } } : s)),
  setPointerOverViewport: (over) => set({ pointerOverViewport: over }),
  setPointerOverCanvas: (over) => set({ pointerOverCanvas: over }),
  setUvOverlap: (pct, pending) => set({ uvOverlapPct: pct, uvOverlapPending: pending }),
  setEnvironment: (env) => set({ environment: env }),
}));
