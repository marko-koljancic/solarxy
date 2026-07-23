// The Attributes pane's pure logic: watched-node resolution, the
// virtualization window and page math, and cell/header formatting.

import { describe, expect, it } from "vitest";
import { fmtCell, headerCells, pageWindow, watchedNode } from "./attributesTable";

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

describe("fmtCell", () => {
  it("renders four fixed decimals with tabular alignment in mind", () => {
    expect(fmtCell(1)).toBe("1.0000");
    expect(fmtCell(-0.25)).toBe("-0.2500");
  });
  it("normalizes negative zero", () => {
    expect(fmtCell(-0.000001)).toBe("0.0000");
  });
  it("renders missing lanes as a hyphen", () => {
    expect(fmtCell(null)).toBe("-");
    expect(fmtCell(Number.NaN)).toBe("-");
  });
});

describe("headerCells", () => {
  it("keeps scalar lanes flat and fans vectors out by component", () => {
    expect(
      headerCells([
        { key: "P", ty: "vec3", components: 3 },
        { key: "mask", ty: "float", components: 1 },
        { key: "color", ty: "vec4", components: 4 },
      ]),
    ).toEqual(["P.x", "P.y", "P.z", "mask", "color.x", "color.y", "color.z", "color.w"]);
  });
});
