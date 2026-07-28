// The silhouette generator: rounded-corner polygon paths and the
// left-right symmetry commitment for every shaped role body, plus the
// category fallback totality (every taxonomy id must resolve to real art).

import { describe, expect, it } from "vitest";
import type { NodeTypeSnapshot } from "../engine/types";
import {
  GLYPH_PATHS,
  ROLE_BODIES,
  glyphPath,
  nodeRole,
  roundedPolygonPath,
  type RoleBody,
} from "./nodeVisual";

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

describe("category fallback totality (the 15-category taxonomy)", () => {
  const CATEGORIES: NodeTypeSnapshot["category"][] = [
    "container",
    "generators",
    "attribute",
    "transform",
    "copy",
    "topology",
    "shaders",
    "import",
    "export",
    "lights",
    "cameras",
    "utility",
    "tex_generate",
    "tex_adjust",
    "tex_composite",
  ];

  const probe = (category: NodeTypeSnapshot["category"]): NodeTypeSnapshot =>
    ({
      typeId: "probe",
      category,
      glyph: "no_such_glyph",
      role: "hologram",
    }) as unknown as NodeTypeSnapshot;

  it("every category id resolves drawable glyph art for an unknown glyph key", () => {
    for (const cat of CATEGORIES) {
      const d = glyphPath(probe(cat));
      expect(d, cat).toBeTypeOf("string");
      expect(Object.values(GLYPH_PATHS), cat).toContain(d);
    }
  });

  it("every category id resolves a silhouette role for an unknown role", () => {
    for (const cat of CATEGORIES) {
      expect(nodeRole(probe(cat)), cat).toBeTypeOf("string");
    }
  });
});

describe("ROLE_BODIES (D-21)", () => {
  const entries = Object.entries(ROLE_BODIES) as [string, RoleBody][];

  /** Roles whose silhouette is deliberately NOT left-right symmetric,
   * each with the meaning the asymmetry carries. Everything else is held
   * to the symmetry rule below, so an accidental lopsided path still
   * fails. */
  const ASYMMETRIC: Record<string, string> = {
    camera: "points the way it aims",
    container: "carries a folder tab",
  };

  it("covers the six shaped roles", () => {
    expect(entries.map(([k]) => k).sort()).toEqual([
      "analyzer",
      "branch",
      "camera",
      "container",
      "imageSource",
      "light",
    ]);
  });

  it("stays inside its own declared body box", () => {
    for (const [role, body] of entries) {
      for (const [x, y] of pathPoints(body.path)) {
        expect(x, `${role} x`).toBeGreaterThanOrEqual(0);
        expect(x, `${role} x`).toBeLessThanOrEqual(body.w);
        expect(y, `${role} y`).toBeGreaterThanOrEqual(0);
        expect(y, `${role} y`).toBeLessThanOrEqual(body.h);
      }
    }
  });

  it("declares a positive body box (the SVG viewBox is built from it)", () => {
    for (const [role, body] of entries) {
      expect(body.w, `${role} w`).toBeGreaterThan(0);
      expect(body.h, `${role} h`).toBeGreaterThan(0);
    }
  });

  it("is left-right symmetric except where the asymmetry means something", () => {
    for (const [role, body] of entries) {
      if (role in ASYMMETRIC) continue;
      const pts = pathPoints(body.path);
      for (const [x, y] of pts) {
        const mirrored = pts.some(
          ([mx, my]) => Math.abs(mx - (body.w - x)) < 0.01 && Math.abs(my - y) < 0.01,
        );
        expect(mirrored, `${role}: (${x}, ${y}) has no mirror twin`).toBe(true);
      }
    }
  });

  it("the asymmetric roles really are asymmetric (the exemption is not stale)", () => {
    for (const role of Object.keys(ASYMMETRIC)) {
      const body = ROLE_BODIES[role as keyof typeof ROLE_BODIES];
      expect(body, role).toBeDefined();
      const pts = pathPoints(body!.path);
      const symmetric = pts.every(([x, y]) =>
        pts.some(([mx, my]) => Math.abs(mx - (body!.w - x)) < 0.01 && Math.abs(my - y) < 0.01),
      );
      expect(symmetric, `${role} is symmetric now, so drop its exemption`).toBe(false);
    }
  });
});
