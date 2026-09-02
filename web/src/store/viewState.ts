// The view-state mirror: pane layout, per-pane display settings, pane
// rectangles, and the hover context flag. Rust (the wasm host) owns the
// truth; this store only mirrors ViewStateDto returns and HostEvents, and
// components mutate through the session's view actions.

import { create } from "zustand";
import type {
  BackendCapsSet,
  EnvironmentState,
  PaneRectDto,
  ToolMode,
  ViewStateDto,
} from "../engine/types";

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
  /** Per-pane traced-preview sample counts, `[samples, target]` or null.
   * Only meaningful while that pane's engine is traced; the toolbar reads
   * visibility from the pane settings, not from this. */
  paneSamples: (readonly [number, number] | null)[];
  /** Both backends' capabilities, read once at boot. Null until then. */
  backendCaps: BackendCapsSet | null;
  /** Which tools apply to the current selection, and which params its
   * transform is made of. Empty when nothing manipulable is selected, which
   * every consumer reads as "do not narrow anything": arming a tool with no
   * selection is harmless and becomes live the moment you select something. */
  selectionTools: ToolMode[];
  selectionTransformParams: string[];
  setView: (view: ViewStateDto) => void;
  setPaneRects: (rects: PaneRectDto[]) => void;
  setActivePaneMirror: (pane: number) => void;
  setPointerOverViewport: (over: boolean) => void;
  setPointerOverCanvas: (over: boolean) => void;
  setUvOverlap: (pct: number | null, pending: boolean) => void;
  setEnvironment: (env: EnvironmentState) => void;
  setToolMode: (tool: ToolMode) => void;
  setGizmoReadout: (text: string | null) => void;
  setPaneSamples: (pane: number, samples: number, target: number) => void;
  setBackendCaps: (caps: BackendCapsSet) => void;
  setSelectionCapability: (tools: ToolMode[], transformParams: string[]) => void;
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
  paneSamples: [null, null, null, null],
  backendCaps: null,
  selectionTools: [],
  selectionTransformParams: [],
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
  // No equality guard: the host already pushes this event on change only.
  setPaneSamples: (pane, samples, target) =>
    set((s) => {
      const next = s.paneSamples.slice();
      next[pane] = [samples, target] as const;
      return { paneSamples: next };
    }),
  setBackendCaps: (caps) => set({ backendCaps: caps }),
  // No equality guard: the host already pushes this event on change only.
  setSelectionCapability: (tools, transformParams) =>
    set({ selectionTools: tools, selectionTransformParams: transformParams }),
}));

/** Whether a tool can act on what is selected.
 *
 * With nothing manipulable selected the host reports an empty set, and every
 * tool stays available: arming one then is harmless, draws nothing, and goes
 * live as soon as something is selected. A tool only greys out when there IS a
 * target and it genuinely cannot use it.
 *
 * Exported for tests and for the two components that ask. */
export function toolApplies(tool: ToolMode, available: ToolMode[]): boolean {
  return available.length === 0 || available.includes(tool);
}
