// The D-21 silhouette generator: rounded-corner polygon paths and the
// left-right symmetry commitment for every shaped role body.

import { describe, expect, it } from "vitest";
import { ROLE_BODY_PATHS, roundedPolygonPath } from "./nodeVisual";

/** Every coordinate pair in a path built from M/L/Q commands. */
function pathPoints(d: string): [number, number][] {
  const nums = d.match(/-?\d+(?:\.\d+)?/g)?.map(Number) ?? [];
  const pts: [number, number][] = [];
  for (let i = 0; i + 1 < nums.length; i += 2) pts.push([nums[i], nums[i + 1]]);
  return pts;
}

describe("roundedPolygonPath", () => {
  it("emits one closed subpath with a quadratic corner per vertex", () => {
    const d = roundedPolygonPath([
      [0, 0, 4],
      [10, 0, 4],
      [10, 10, 4],
      [0, 10, 4],
    ]);
    expect(d.startsWith("M ")).toBe(true);
    expect(d.endsWith("Z")).toBe(true);
    expect(d.match(/Q /g)).toHaveLength(4);
    expect(d.match(/L /g)).toHaveLength(3);
  });

  it("clamps the radius on edges shorter than 2r instead of overshooting", () => {
    const d = roundedPolygonPath([
      [0, 0, 50],
      [10, 0, 50],
      [10, 10, 50],
      [0, 10, 50],
    ]);
    for (const [x, y] of pathPoints(d)) {
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThanOrEqual(10);
      expect(y).toBeGreaterThanOrEqual(0);
      expect(y).toBeLessThanOrEqual(10);
    }
  });
});

describe("ROLE_BODY_PATHS (D-21)", () => {
  const entries = Object.entries(ROLE_BODY_PATHS) as [string, string][];

  it("covers the four shaped roles", () => {
    expect(entries.map(([k]) => k).sort()).toEqual([
      "analyzer",
      "branch",
      "imageSource",
      "light",
    ]);
  });

  it("stays inside the 112x32 body box", () => {
    for (const [, d] of entries) {
      for (const [x, y] of pathPoints(d)) {
        expect(x).toBeGreaterThanOrEqual(0);
        expect(x).toBeLessThanOrEqual(112);
        expect(y).toBeGreaterThanOrEqual(0);
        expect(y).toBeLessThanOrEqual(32);
      }
    }
  });

  it("is left-right symmetric: mirroring x across 56 maps the outline onto itself", () => {
    for (const [role, d] of entries) {
      const pts = pathPoints(d);
      for (const [x, y] of pts) {
        const mirrored = pts.some(([mx, my]) => Math.abs(mx - (112 - x)) < 0.01 && Math.abs(my - y) < 0.01);
        expect(mirrored, `${role}: (${x}, ${y}) has no mirror twin`).toBe(true);
      }
    }
  });
});
