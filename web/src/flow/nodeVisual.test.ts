// The silhouette generator: rounded-corner polygon paths, the one-box
// geometric contract (every role occupies NODE_BOX; risers ride above it,
// never inside; sized-down bodies stay inside it), the left-right
// symmetry commitment for every shaped role body, the stylesheet's
// agreement with the geometry tables, and the plumbing that takes the
// family fallback from the presentation tables rather than deciding it
// here (the taxonomy itself is asserted in Rust, against the real
// registry).

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { NodeRole, NodeTypeSnapshot, PresentationTables } from "../engine/types";
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

describe("the family fallback, which the tables name and this file draws", () => {
  // The TAXONOMY is not restated here any more. Which family a category
  // falls back to, and that every one of them names art the catalog
  // declares, are asserted in `solarxy-studio` against the real registry
  // (`every_category_falls_back_to_a_glyph_the_catalog_declares` and its
  // role twin), plus `tokens_drift.rs` for the art itself. A copy of the
  // fifteen ids here would be the second opinion this release removed.
  //
  // What is left to test is the plumbing: that this file draws whatever
  // the tables name rather than deciding for itself, and that it still
  // draws something during the frames before the tables arrive.
  const tables = {
    dataTypes: {},
    categories: {
      generators: { order: 1, glyph: "box", role: "standard" },
      lights: { order: 9, glyph: "point", role: "light" },
    },
  } as unknown as PresentationTables;

  const probe = (category: string): NodeTypeSnapshot =>
    ({
      typeId: "probe",
      category,
      glyph: "no_such_glyph",
      role: "hologram",
    }) as unknown as NodeTypeSnapshot;

  it("falls back to the art the tables name, not to art of its own", () => {
    expect(glyphPath(probe("generators"), tables)).toBe(GLYPH_PATHS.box);
    expect(glyphPath(probe("lights"), tables)).toBe(GLYPH_PATHS.point);
  });

  it("takes the silhouette the tables name for a role it cannot draw", () => {
    expect(nodeRole(probe("generators"), tables)).toBe("standard");
    expect(nodeRole(probe("lights"), tables)).toBe("light");
  });

  it("still draws before the tables arrive, and after an unheard-of category", () => {
    // Null tables are the frames between mount and boot; an unknown
    // category is an engine newer than this build. Neither may produce a
    // broken icon or a crash.
    expect(Object.values(GLYPH_PATHS)).toContain(glyphPath(probe("generators"), null));
    expect(Object.values(GLYPH_PATHS)).toContain(glyphPath(probe("no_such_family"), tables));
    expect(nodeRole(probe("generators"), null)).toBe("standard");
    expect(nodeRole(probe("no_such_family"), tables)).toBe("standard");
  });

  it("prefers a declared glyph that has art over any fallback", () => {
    const merged = { ...probe("generators"), glyph: "merge" } as NodeTypeSnapshot;
    expect(glyphPath(merged, tables)).toBe(GLYPH_PATHS.merge);
  });
});

describe("ROLE_BODIES", () => {
  const entries = Object.entries(ROLE_BODIES) as [string, RoleBody][];

  it("covers the three shaped subflow roles (the root roles are CSS pills)", () => {
    expect(entries.map(([k]) => k).sort()).toEqual(["analyzer", "branch", "imageSource"]);
  });

  it("stays inside the one layout box", () => {
    for (const [role, body] of entries) {
      for (const [x, y] of pathPoints(body.path)) {
        expect(x, `${role} x`).toBeGreaterThanOrEqual(0);
        expect(x, `${role} x`).toBeLessThanOrEqual(NODE_BOX.w);
        expect(y, `${role} y`).toBeGreaterThanOrEqual(0);
        expect(y, `${role} y`).toBeLessThanOrEqual(NODE_BOX.h);
      }
    }
  });

  it("is left-right symmetric (no shaped role carries asymmetry any more)", () => {
    for (const [role, body] of entries) {
      const pts = pathPoints(body.path);
      for (const [x, y] of pts) {
        const mirrored = pts.some(
          ([mx, my]) => Math.abs(mx - (NODE_BOX.w - x)) < 0.01 && Math.abs(my - y) < 0.01,
        );
        expect(mirrored, `${role}: (${x}, ${y}) has no mirror twin`).toBe(true);
      }
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
    // either place breaks this pin until both move together. The sizing
    // rule for a role may live in a grouped selector (the root pills
    // share one), so the finder matches any block whose selector list
    // contains the role's .node-body selector and whose body sets an
    // inset.
    for (const role of ["container", "camera", "light", "text", "terminal"] as const) {
      const body = ROLE_BODY_SIZE[role];
      const inset = `inset: ${(NODE_BOX.h - body.h) / 2}px ${(NODE_BOX.w - body.w) / 2}px;`;
      const block = css.split("}").find((b) => {
        const brace = b.lastIndexOf("{");
        if (brace === -1) return false;
        const selectors = (b.slice(0, brace).split("*/").pop() ?? "")
          .split(",")
          .map((s) => s.trim());
        return (
          selectors.includes(`.flow-node.role-${role} .node-body`) && b.includes("inset:")
        );
      });
      expect(block, `.flow-node.role-${role} .node-body sizing block`).toBeDefined();
      expect(block, `${role} body inset`).toContain(inset);
    }
  });

  it("the glyph inks directly on the body (the chip slot paints no plate)", () => {
    // The maintainer's ruling: transparent glyph backgrounds on every
    // node. The chip element survives as a centring slot only, so its
    // block must not declare a background.
    const block = css.split("}").find((b) => {
      const brace = b.lastIndexOf("{");
      if (brace === -1) return false;
      return (b.slice(0, brace).split("*/").pop() ?? "").trim() === ".node-chip";
    });
    expect(block, ".node-chip block").toBeDefined();
    expect(block, "chip plate").not.toMatch(/background/);
  });
});
