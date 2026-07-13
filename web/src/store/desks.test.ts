// Desk snapshots (Phase 10 shape): the forward migration of pre-docking desks,
// and sanitize's coercion of anything stale or hand-edited.

import { describe, expect, it } from "vitest";
import { sanitizeDesk, type DeskSnapshot } from "./desks";
import { DEFAULT_RECIPE } from "../dock/layouts";

/** A desk exactly as the pre-Phase-10 shell stored it. */
const LEGACY_DESK = {
  name: "My Old Desk",
  viewportSide: "right" as const,
  propertiesDock: "right" as const,
  splitPct: 63,
  drawerHeight: 300,
  drawerWidth: 380,
  showFlowGrid: false,
  showMinimap: true,
  showFlowControls: false,
  viewLayout: "quad" as const,
};

describe("legacy desk migration", () => {
  it("synthesizes a recipe from a pre-docking desk instead of dropping it", () => {
    const desk = sanitizeDesk(LEGACY_DESK);
    expect(desk.layout).toEqual({
      kind: "recipe",
      recipe: {
        viewportSide: "right",
        propertiesDock: "right",
        splitPct: 63,
        review: false,
      },
    });
    // The non-arrangement fields carry over untouched.
    expect(desk.showFlowGrid).toBe(false);
    expect(desk.showMinimap).toBe(true);
    expect(desk.showFlowControls).toBe(false);
    expect(desk.viewLayout).toBe("quad");
    expect(desk.name).toBe("My Old Desk");
  });

  it("clamps a hand-edited legacy split and falls back on bad enums", () => {
    const wild = sanitizeDesk({
      ...LEGACY_DESK,
      viewportSide: "up" as never,
      propertiesDock: "floating" as never,
      splitPct: 5,
    });
    expect(wild.layout).toEqual({
      kind: "recipe",
      recipe: { viewportSide: "left", propertiesDock: "bottom", splitPct: 20, review: false },
    });

    const tooWide = sanitizeDesk({ ...LEGACY_DESK, splitPct: 99 });
    expect(tooWide.layout.kind === "recipe" && tooWide.layout.recipe.splitPct).toBe(80);
  });
});

describe("sanitizeDesk", () => {
  it("passes a serialized dock layout through untouched", () => {
    const json = {
      grid: { root: {}, width: 10, height: 10, orientation: "HORIZONTAL" },
      panels: {},
    };
    const desk: DeskSnapshot = {
      name: "Saved",
      layout: { kind: "serialized", json: json as never },
      showFlowGrid: true,
      showMinimap: false,
      showFlowControls: true,
      viewLayout: "single",
    };
    const back = JSON.parse(JSON.stringify(desk)) as DeskSnapshot;
    expect(sanitizeDesk(back)).toEqual(desk);
  });

  it("falls back to the default recipe when a desk has no usable layout", () => {
    const desk = sanitizeDesk({ name: "Broken" } as never);
    expect(desk.layout).toEqual({ kind: "recipe", recipe: DEFAULT_RECIPE });
    expect(desk.viewLayout).toBe("single");
  });

  it("rejects an unknown pane layout", () => {
    expect(sanitizeDesk({ ...LEGACY_DESK, viewLayout: "hexview" as never }).viewLayout).toBe(
      "single",
    );
  });
});
