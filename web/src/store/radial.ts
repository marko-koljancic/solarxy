// Hover-radial + node-info-modal state: one radial at a time,
// anchored to a canvas node; the info modal is modeless and draggable,
// keyed by node id. Both are pure UI state over the mirror.
//
// The target carries IDENTITY ONLY. Its screen position used to be
// captured once at open time from a DOM rect, which meant the ring drifted off
// its node on pan and zoom; the ring now derives its anchor from the live xyflow
// transform each render (flow/radialAnchor.ts). The mutable per-node flags
// (bypassed, display) likewise come from the mirror, so the ring shows current
// state rather than open-time state.

import { create } from "zustand";
import type { GraphContext } from "../engine/types";

export interface RadialTarget {
  nodeId: number;
  ctx: GraphContext;
  isContainer: boolean;
  bypassable: boolean;
}

interface RadialStore {
  target: RadialTarget | null;
  /** The node whose info modal is open (survives radial close). */
  infoNode: { nodeId: number; ctx: GraphContext; x: number; y: number } | null;
  openRadial: (target: RadialTarget) => void;
  closeRadial: () => void;
  openInfo: (nodeId: number, ctx: GraphContext, x: number, y: number) => void;
  closeInfo: () => void;
}

export const useRadial = create<RadialStore>((set) => ({
  target: null,
  infoNode: null,
  openRadial: (target) => set({ target }),
  closeRadial: () => set({ target: null }),
  openInfo: (nodeId, ctx, x, y) => set({ infoNode: { nodeId, ctx, x, y }, target: null }),
  closeInfo: () => set({ infoNode: null }),
}));
