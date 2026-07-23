// The portal's pure placement math: below-left / below-right alignment,
// side placement for submenu flyouts, and the viewport flips and clamps
// that keep a panel on screen near the edges.

import { describe, expect, it } from "vitest";
import { placeDropdown } from "./DropdownPortal";

const VIEWPORT = { width: 1000, height: 800 };
const PANEL = { width: 200, height: 300 };

function anchorAt(left: number, top: number, width = 80, height = 20) {
  return { left, top, right: left + width, bottom: top + height };
}

describe("placeDropdown", () => {
  it("opens below the anchor, left-aligned by default", () => {
    const pos = placeDropdown(anchorAt(100, 50), PANEL, VIEWPORT, "below", "left");
    expect(pos.left).toBe(100);
    expect(pos.top).toBe(72);
  });

  it("right-aligns the panel to the anchor's right edge", () => {
    const pos = placeDropdown(anchorAt(700, 50), PANEL, VIEWPORT, "below", "right");
    expect(pos.left).toBe(780 - 200);
    expect(pos.top).toBe(72);
  });

  it("clamps a below panel that would overflow the right edge", () => {
    const pos = placeDropdown(anchorAt(950, 50), PANEL, VIEWPORT, "below", "left");
    expect(pos.left).toBe(1000 - 200 - 8);
  });

  it("flips a below panel above the anchor when the bottom would overflow", () => {
    const pos = placeDropdown(anchorAt(100, 700), PANEL, VIEWPORT, "below", "left");
    expect(pos.top).toBe(700 - 300 - 2);
  });

  it("never places a panel off the top or left edge", () => {
    const pos = placeDropdown(anchorAt(2, 4), { width: 900, height: 900 }, VIEWPORT, "below", "left");
    expect(pos.left).toBeGreaterThanOrEqual(8);
    expect(pos.top).toBeGreaterThanOrEqual(8);
  });

  it("side placement opens to the right of the anchor row", () => {
    const pos = placeDropdown(anchorAt(100, 200, 160, 24), PANEL, VIEWPORT, "side", "left");
    expect(pos.left).toBe(100 + 160 + 2);
    expect(pos.top).toBe(196);
  });

  it("side placement flips to the left when the right edge would overflow", () => {
    const pos = placeDropdown(anchorAt(880, 200, 100, 24), PANEL, VIEWPORT, "side", "left");
    expect(pos.left).toBe(880 - 200 - 2);
  });

  it("side placement clamps against the bottom edge", () => {
    const pos = placeDropdown(anchorAt(100, 750, 160, 24), PANEL, VIEWPORT, "side", "left");
    expect(pos.top).toBe(800 - 300 - 8);
  });
});
