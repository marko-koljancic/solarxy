// The traced-preview slices of the view-state mirror: the per-pane sample
// counter the pane toolbar reads, and the backend capabilities the display
// menu gates its traced entry on.

import { describe, expect, it } from "vitest";

import { useViewState } from "./viewState";

describe("paneSamples", () => {
  it("stores one pane's count without disturbing the others", () => {
    useViewState.setState({ paneSamples: [null, null, null, null] });
    useViewState.getState().setPaneSamples(2, 17, 4096);
    const s = useViewState.getState().paneSamples;
    expect(s[2]).toEqual([17, 4096]);
    expect(s[0]).toBeNull();
    expect(s[1]).toBeNull();
    expect(s[3]).toBeNull();
  });

  it("replaces a pane's count as the accumulation advances", () => {
    useViewState.setState({ paneSamples: [null, null, null, null] });
    useViewState.getState().setPaneSamples(0, 1, 4096);
    useViewState.getState().setPaneSamples(0, 2, 4096);
    expect(useViewState.getState().paneSamples[0]).toEqual([2, 4096]);
  });
});

describe("backendCaps", () => {
  it("starts unknown and holds the boot read", () => {
    useViewState.setState({ backendCaps: null });
    expect(useViewState.getState().backendCaps).toBeNull();
    const caps = {
      raster: { progressive: false, supportsInstancing: true, writesAovs: false },
      traced: { progressive: true, supportsInstancing: true, writesAovs: true },
    };
    useViewState.getState().setBackendCaps(caps);
    expect(useViewState.getState().backendCaps?.traced.progressive).toBe(true);
  });
});
