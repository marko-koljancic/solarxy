// The in-flight still render, mirrored for whatever is showing it.
//
// A store slice rather than component state because two things read it: the
// modal that shows progress, and the frame loop that pushes progress into it
// without knowing whether a modal is open. The tiles are painted into a canvas
// as they land rather than kept here: a list of 67 megapixels of Uint8Array
// would be the largest thing in the store by three orders of magnitude.

import { create } from "zustand";

export interface RenderJobState {
  /** Whether a job is running. */
  busy: boolean;
  /** Which tile of how many, and how many samples of how many within it. */
  tile: number;
  tiles: number;
  sample: number;
  samples: number;
  /** The image being assembled, so a preview knows how big its canvas is. */
  width: number;
  height: number;
  start: (width: number, height: number) => void;
  progress: (p: {
    tile: number;
    tiles: number;
    sample: number;
    samples: number;
    done: boolean;
  }) => void;
  stop: () => void;
}

export const useRenderJob = create<RenderJobState>((set) => ({
  busy: false,
  tile: 0,
  tiles: 0,
  sample: 0,
  samples: 0,
  width: 0,
  height: 0,
  start: (width, height) =>
    set({ busy: true, tile: 0, tiles: 0, sample: 0, samples: 0, width, height }),
  progress: ({ tile, tiles, sample, samples, done }) =>
    set({ tile, tiles, sample, samples, busy: !done }),
  stop: () => set({ busy: false }),
}));
