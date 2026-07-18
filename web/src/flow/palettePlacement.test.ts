import { describe, expect, it } from "vitest";
import { palettePlacement, type Rect, type Size } from "./palettePlacement";

// A pane offset from the viewport origin: the bug being guarded against is
// placement that ignores the pane and resolves against the window, so a pane
// at 0,0 would hide it.
const PANE: Rect = { left: 400, top: 100, width: 600, height: 500 };
const PANEL: Size = { width: 440, height: 300 };

describe("palettePlacement", () => {
  it("opens at the pointer when the pointer is over the pane", () => {
    const at = palettePlacement({ x: 500, y: 200 }, PANE, PANEL);
    expect(at.x).toBeCloseTo(506);
    expect(at.y).toBeCloseTo(206);
  });

  it("stays inside the pane when the pointer is near the far edge", () => {
    // Bottom-right corner: naive pointer placement would put the panel
    // mostly outside the pane.
    const at = palettePlacement({ x: 995, y: 595 }, PANE, PANEL);
    expect(at.x).toBe(400 + 600 - 440 - 8);
    expect(at.y).toBe(100 + 500 - 300 - 8);
    expect(at.x + PANEL.width).toBeLessThanOrEqual(PANE.left + PANE.width);
    expect(at.y + PANEL.height).toBeLessThanOrEqual(PANE.top + PANE.height);
  });

  it("never places the panel above or left of the pane", () => {
    const at = palettePlacement({ x: 401, y: 101 }, PANE, PANEL);
    expect(at.x).toBeGreaterThanOrEqual(PANE.left);
    expect(at.y).toBeGreaterThanOrEqual(PANE.top);
  });

  it("centres over the pane when there is no pointer (opened from a menu)", () => {
    const at = palettePlacement(null, PANE, PANEL);
    expect(at.x).toBeCloseTo(400 + (600 - 440) / 2);
    expect(at.y).toBeCloseTo(100 + 500 / 6);
  });

  it("centres when the pointer is outside the pane", () => {
    const outside = palettePlacement({ x: 50, y: 50 }, PANE, PANEL);
    expect(outside).toEqual(palettePlacement(null, PANE, PANEL));
  });

  // The regression that motivated this: the old CSS resolved against the
  // viewport, so the palette appeared top-right of the WINDOW regardless of
  // the pane. Wherever it lands now, it must be within the pane.
  it("lands inside the pane for pointers all over it", () => {
    for (let x = PANE.left; x <= PANE.left + PANE.width; x += 37) {
      for (let y = PANE.top; y <= PANE.top + PANE.height; y += 41) {
        const at = palettePlacement({ x, y }, PANE, PANEL);
        expect(at.x).toBeGreaterThanOrEqual(PANE.left);
        expect(at.y).toBeGreaterThanOrEqual(PANE.top);
        expect(at.x + PANEL.width).toBeLessThanOrEqual(PANE.left + PANE.width);
        expect(at.y + PANEL.height).toBeLessThanOrEqual(PANE.top + PANE.height);
      }
    }
  });

  // A pane narrower than the panel cannot satisfy both edges; it must still
  // pin to the near edge rather than producing a negative-width clamp.
  it("degrades to the pane's top-left when the pane is smaller than the panel", () => {
    const tiny: Rect = { left: 10, top: 20, width: 100, height: 80 };
    const at = palettePlacement({ x: 60, y: 60 }, tiny, PANEL);
    expect(at.x).toBe(18);
    expect(at.y).toBe(28);
  });
});
