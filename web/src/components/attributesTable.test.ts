// What is left of the Attributes pane's helpers. The cell and header
// formatting moved into `solarxy-studio` and is tested there; the engine
// formats a page where the values are. These two stay for the reasons the
// module header gives.

import { describe, expect, it } from "vitest";
import { pageWindow, watchedNode } from "./attributesTable";

describe("watchedNode", () => {
  it("prefers the first selected node", () => {
    expect(watchedNode([7, 9], 3)).toBe(7);
  });
  it("falls back to the display-flag node", () => {
    expect(watchedNode([], 3)).toBe(3);
  });
  it("yields null with neither", () => {
    expect(watchedNode([], null)).toBeNull();
  });
});

describe("pageWindow", () => {
  it("covers the visible rows plus overscan", () => {
    const w = pageWindow(0, 220, 22, 1000, 128);
    expect(w.first).toBe(0);
    expect(w.last).toBe(10 + 8);
    expect(w.pages).toEqual([0]);
  });

  it("spans page boundaries when the window crosses one", () => {
    // Rows ~120..146 visible: pages 0 and 1 both needed.
    const w = pageWindow(120 * 22, 26 * 22, 22, 1000, 128);
    expect(w.first).toBe(112);
    expect(w.last).toBe(154);
    expect(w.pages).toEqual([0, 1]);
  });

  it("clamps to the data extent", () => {
    const w = pageWindow(10_000, 300, 22, 40, 128);
    expect(w.last).toBe(40);
    expect(w.pages).toEqual([0]);
  });

  it("is empty for empty data", () => {
    expect(pageWindow(0, 300, 22, 0, 128)).toEqual({ first: 0, last: 0, pages: [] });
  });
});


