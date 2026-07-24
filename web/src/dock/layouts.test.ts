// The dock layout recipes. The recipe is what makes a preset and a
// migrated legacy desk survive a hand-edit and a dockview version bump, so its
// coercion is the part worth pinning.

import { describe, expect, it } from "vitest";
import {
  DEFAULT_RECIPE,
  clampAttributesPct,
  clampSplit,
  sanitizeRecipe,
  synthesizeRecipe,
} from "./layouts";

/** The wave-4 panel fields at their absent-means-off defaults. */
const PANEL_DEFAULTS = { attributes: false, attributesPct: 30, texture: false, tree: false };

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
    ).toEqual({
      viewportSide: "left",
      propertiesDock: "bottom",
      splitPct: 55,
      review: false,
      ...PANEL_DEFAULTS,
    });
  });

  it("keeps valid values", () => {
    expect(
      sanitizeRecipe({
        viewportSide: "right",
        propertiesDock: "right",
        splitPct: 70,
        review: true,
      }),
    ).toEqual({
      viewportSide: "right",
      propertiesDock: "right",
      splitPct: 70,
      review: true,
      ...PANEL_DEFAULTS,
    });
  });

  it("coerces the wave-4 panel fields from junk and keeps them when valid", () => {
    const junk = sanitizeRecipe({
      attributes: "yes" as never,
      attributesPct: Number.NaN,
      texture: 1 as never,
      tree: null as never,
    });
    expect(junk.attributes).toBe(false);
    expect(junk.attributesPct).toBe(30);
    expect(junk.texture).toBe(false);
    expect(junk.tree).toBe(false);

    const set = sanitizeRecipe({ attributes: true, attributesPct: 40, texture: true, tree: true });
    expect(set.attributes).toBe(true);
    expect(set.attributesPct).toBe(40);
    expect(set.texture).toBe(true);
    expect(set.tree).toBe(true);
  });
});

describe("clampAttributesPct", () => {
  it("holds the spreadsheet share inside 15-50 percent", () => {
    expect(clampAttributesPct(5)).toBe(15);
    expect(clampAttributesPct(90)).toBe(50);
    expect(clampAttributesPct(30)).toBe(30);
    expect(clampAttributesPct(Number.NaN)).toBe(30);
  });
});

describe("synthesizeRecipe", () => {
  it("maps a pre-docking arrangement onto the equivalent recipe", () => {
    expect(
      synthesizeRecipe({ viewportSide: "right", propertiesDock: "right", splitPct: 63 }),
    ).toEqual({
      viewportSide: "right",
      propertiesDock: "right",
      splitPct: 63,
      review: false,
      ...PANEL_DEFAULTS,
    });
  });

  it("defaults the fields the old shell did not have", () => {
    expect(synthesizeRecipe({})).toEqual(DEFAULT_RECIPE);
  });

  it("never grants a legacy desk the wave-4 panels", () => {
    const r = synthesizeRecipe({ viewportSide: "right", splitPct: 40 });
    expect(r.attributes).toBe(false);
    expect(r.texture).toBe(false);
    expect(r.tree).toBe(false);
  });
});
