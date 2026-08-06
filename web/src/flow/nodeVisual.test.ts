// The silhouette generator: rounded-corner polygon paths, the one-box
// geometric contract (every role occupies NODE_BOX; risers ride above it,
// never inside; sized-down bodies stay inside it), the left-right
// symmetry commitment for every shaped role body, the stylesheet's
// agreement with the geometry tables, and the category fallback totality
// (every taxonomy id must resolve to real art).

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { NodeRole, NodeTypeSnapshot } from "../engine/types";
import {
  GLYPH_PATHS,
  NODE_BOX,
  ROLE_BODIES,
  ROLE_BODY_SIZE,
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

describe("ROLE_BODIES", () => {
  const entries = Object.entries(ROLE_BODIES) as [string, RoleBody][];

  /** Roles whose silhouette is deliberately NOT left-right symmetric,
   * each with the meaning the asymmetry carries. Everything else is held
   * to the symmetry rule below, so an accidental lopsided path still
   * fails. */
  const ASYMMETRIC: Record<string, string> = {
    container: "carries a folder tab on the left",
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

  it("stays inside the box plus its declared riser (risers above, never inside)", () => {
    // The one-contract rule: the box is never consumed. A riser (folder
    // tab, viewfinder bump) is authored at negative y, up to the declared
    // overhang; the body proper spans the full box, which is what makes a
    // box-centred glyph a body-centred glyph with no compensating offset.
    for (const [role, body] of entries) {
      for (const [x, y] of pathPoints(body.path)) {
        expect(x, `${role} x`).toBeGreaterThanOrEqual(0);
        expect(x, `${role} x`).toBeLessThanOrEqual(NODE_BOX.w);
        expect(y, `${role} y`).toBeGreaterThanOrEqual(-body.riser);
        expect(y, `${role} y`).toBeLessThanOrEqual(NODE_BOX.h);
      }
    }
  });

  it("a role without a riser has no negative-y vertex", () => {
    for (const [role, body] of entries) {
      if (body.riser > 0) continue;
      for (const [, y] of pathPoints(body.path)) {
        expect(y, `${role}: riser 0 but the path rises above the box`).toBeGreaterThanOrEqual(0);
      }
    }
  });

  it("a declared riser is real (the declaration is not stale)", () => {
    for (const [role, body] of entries) {
      if (body.riser === 0) continue;
      const rises = pathPoints(body.path).some(([, y]) => y < 0);
      expect(rises, `${role}: declares riser ${body.riser} but never rises`).toBe(true);
    }
  });

  it("is left-right symmetric except where the asymmetry means something", () => {
    for (const [role, body] of entries) {
      if (role in ASYMMETRIC) continue;
      const pts = pathPoints(body.path);
      for (const [x, y] of pts) {
        const mirrored = pts.some(
          ([mx, my]) => Math.abs(mx - (NODE_BOX.w - x)) < 0.01 && Math.abs(my - y) < 0.01,
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
        pts.some(([mx, my]) => Math.abs(mx - (NODE_BOX.w - x)) < 0.01 && Math.abs(my - y) < 0.01),
      );
      expect(symmetric, `${role} is symmetric now, so drop its exemption`).toBe(false);
    }
  });
});

describe("the one geometric contract (box, body, riser)", () => {
  const ALL_ROLES: NodeRole[] = [
    "standard",
    "container",
    "gather",
    "branch",
    "terminal",
    "analyzer",
    "imageSource",
    "light",
    "camera",
    "text",
    "note",
  ];

  it("every role declares a visible body no larger than the one box", () => {
    for (const role of ALL_ROLES) {
      const body = ROLE_BODY_SIZE[role];
      expect(body, role).toBeDefined();
      expect(body.w, `${role} w`).toBeGreaterThan(0);
      expect(body.w, `${role} w`).toBeLessThanOrEqual(NODE_BOX.w);
      expect(body.h, `${role} h`).toBeGreaterThan(0);
      expect(body.h, `${role} h`).toBeLessThanOrEqual(NODE_BOX.h);
    }
  });

  it("a sized-down body sits centred in the box on whole pixels", () => {
    // The CSS insets are (box - body) / 2 per axis; a half-pixel inset
    // would blur the 1px strokes, so the size difference must stay even.
    for (const role of ALL_ROLES) {
      const body = ROLE_BODY_SIZE[role];
      expect((NODE_BOX.w - body.w) % 2, `${role} horizontal inset`).toBe(0);
      expect((NODE_BOX.h - body.h) % 2, `${role} vertical inset`).toBe(0);
    }
  });

  const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

  it("the stylesheet carries no compensating offset on the centred overlays", () => {
    // The acceptance criterion is the ABSENCE of the old per-role
    // recentring rules: any role-scoped block that positions the chip,
    // the cook arc, the display halo or the terminal core must not shift
    // it with margins or insets. Overlays centre at 50%/50% of the box
    // and per-role geometry is carried by the body, never by an offset.
    for (const block of css.split("}")) {
      const [selector] = block.split("{");
      if (!selector || !selector.includes(".role-")) continue;
      if (!/\.node-chip|\.cook-arc|\.display-halo|\.terminal-core/.test(selector)) continue;
      expect(block, `offset rule in: ${selector.trim()}`).not.toMatch(/margin|top:|left:/);
    }
  });

  it("the stylesheet never overrides the layout box per role", () => {
    // One box for every role: a bare `.flow-node.role-*` selector must
    // not set width or height (the sized-down bodies size .node-body,
    // not the box).
    for (const block of css.split("}")) {
      const [selector, body] = block.split("{");
      if (!selector || !body) continue;
      const selectors = selector.split(",").map((s) => s.trim());
      const allBareRole = selectors.every((s) => /^\.flow-node\.role-[a-zA-Z]+$/.test(s));
      if (!allBareRole || selectors.length === 0) continue;
      expect(body, `box override in: ${selector.trim()}`).not.toMatch(/width:|height:/);
    }
  });

  it("the stylesheet's sized bodies mirror ROLE_BODY_SIZE", () => {
    // The inset shorthand is derived from the table, so a size change in
    // either place breaks this pin until both move together.
    for (const role of ["light", "text", "terminal"] as const) {
      const body = ROLE_BODY_SIZE[role];
      const inset = `inset: ${(NODE_BOX.h - body.h) / 2}px ${(NODE_BOX.w - body.w) / 2}px;`;
      const block = css.split("}").find((b) => {
        const brace = b.lastIndexOf("{");
        if (brace === -1) return false;
        const selector = (b.slice(0, brace).split("*/").pop() ?? "").trim();
        return selector === `.flow-node.role-${role} .node-body`;
      });
      expect(block, `.flow-node.role-${role} .node-body block`).toBeDefined();
      expect(block, `${role} body inset`).toContain(inset);
    }
  });
});
