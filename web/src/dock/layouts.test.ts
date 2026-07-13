// The dock layout recipes (Phase 10). The recipe is what makes a preset and a
// migrated legacy desk survive a hand-edit and a dockview version bump, so its
// coercion is the part worth pinning.

import { describe, expect, it } from "vitest";
import { DEFAULT_RECIPE, clampSplit, sanitizeRecipe, synthesizeRecipe } from "./layouts";

describe("clampSplit", () => {
  it("holds the split inside 20-80 percent", () => {
    expect(clampSplit(5)).toBe(20);
    expect(clampSplit(95)).toBe(80);
    expect(clampSplit(50)).toBe(50);
  });

  it("falls back on a non-finite value", () => {
    expect(clampSplit(Number.NaN)).toBe(DEFAULT_RECIPE.splitPct);
  });
});

describe("sanitizeRecipe", () => {
  it("defaults an absent recipe", () => {
    expect(sanitizeRecipe(undefined)).toEqual(DEFAULT_RECIPE);
  });

  it("coerces unknown enum values to the defaults", () => {
    expect(
      sanitizeRecipe({
        viewportSide: "up" as never,
        propertiesDock: "floating" as never,
        splitPct: 55,
        review: false,
      }),
    ).toEqual({ viewportSide: "left", propertiesDock: "bottom", splitPct: 55, review: false });
  });

  it("keeps valid values", () => {
    expect(
      sanitizeRecipe({
        viewportSide: "right",
        propertiesDock: "right",
        splitPct: 70,
        review: true,
      }),
    ).toEqual({ viewportSide: "right", propertiesDock: "right", splitPct: 70, review: true });
  });
});

describe("synthesizeRecipe", () => {
  it("maps a pre-docking arrangement onto the equivalent recipe", () => {
    expect(
      synthesizeRecipe({ viewportSide: "right", propertiesDock: "right", splitPct: 63 }),
    ).toEqual({ viewportSide: "right", propertiesDock: "right", splitPct: 63, review: false });
  });

  it("defaults the fields the old shell did not have", () => {
    expect(synthesizeRecipe({})).toEqual(DEFAULT_RECIPE);
  });
});
