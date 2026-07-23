// The viz settings popover's color mapping: the host's raw RGB 0..1
// triple to the picker's hex and back, byte-exact both ways.

import { describe, expect, it } from "vitest";
import { hexToRgb, rgbToHex } from "./AttrColumn";

describe("viz color mapping", () => {
  it("maps the default amber to hex and back", () => {
    const hex = rgbToHex([1, 0.62, 0.15]);
    expect(hex).toBe("#ff9e26");
    const [r, g, b] = hexToRgb(hex);
    expect(r).toBe(1);
    expect(Math.abs(g - 0.62)).toBeLessThan(1 / 255);
    expect(Math.abs(b - 0.15)).toBeLessThan(1 / 255);
  });

  it("round-trips every byte-aligned value exactly", () => {
    for (const hex of ["#000000", "#ffffff", "#3f80bf"]) {
      expect(rgbToHex(hexToRgb(hex))).toBe(hex);
    }
  });

  it("clamps out-of-range components and survives malformed hex", () => {
    expect(rgbToHex([2, -1, 0.5])).toBe("#ff0080");
    expect(hexToRgb("#nope")).toEqual([1, 1, 1]);
  });
});
