import { describe, expect, it } from "vitest";
import { GAP, placeCoachmark, type Rect, type Size } from "./placement";

const VIEWPORT: Size = { width: 1400, height: 800 };
const CARD: Size = { width: 320, height: 160 };

const inside = (p: { left: number; top: number }, card: Size, v: Size) =>
  p.left >= 0 && p.top >= 0 && p.left + card.width <= v.width && p.top + card.height <= v.height;

describe("placeCoachmark", () => {
  it("honours the preferred side when there is room", () => {
    const anchor: Rect = { left: 600, top: 300, width: 200, height: 80 };
    const p = placeCoachmark(anchor, CARD, VIEWPORT, "bottom");
    expect(p.side).toBe("bottom");
    expect(p.top).toBe(300 + 80 + GAP);
    // Horizontally centred on the anchor.
    expect(p.left).toBe(600 + 100 - 160);
  });

  it("flips away from an edge rather than going off-screen", () => {
    // Anchor hugging the bottom: "bottom" cannot fit a 160px card.
    const anchor: Rect = { left: 600, top: 740, width: 200, height: 40 };
    const p = placeCoachmark(anchor, CARD, VIEWPORT, "bottom");
    expect(p.side).not.toBe("bottom");
    expect(inside(p, CARD, VIEWPORT)).toBe(true);
  });

  // The tool column lives hard against the viewport's left edge. Asking for
  // "left" there must not push the card out of sight.
  it("does not honour a preferred side that has no room", () => {
    const anchor: Rect = { left: 4, top: 200, width: 40, height: 160 };
    const p = placeCoachmark(anchor, CARD, VIEWPORT, "left");
    expect(p.side).not.toBe("left");
    expect(inside(p, CARD, VIEWPORT)).toBe(true);
  });

  it("stays inside the viewport for anchors all over it", () => {
    for (let x = 0; x <= VIEWPORT.width - 40; x += 97) {
      for (let y = 0; y <= VIEWPORT.height - 40; y += 71) {
        for (const side of ["top", "bottom", "left", "right"] as const) {
          const p = placeCoachmark({ left: x, top: y, width: 40, height: 40 }, CARD, VIEWPORT, side);
          expect(inside(p, CARD, VIEWPORT), `${x},${y} ${side} -> ${p.left},${p.top}`).toBe(true);
        }
      }
    }
  });

  it("degrades to the near edge when the card cannot fit at all", () => {
    const tiny: Size = { width: 200, height: 100 };
    const p = placeCoachmark({ left: 10, top: 10, width: 20, height: 20 }, CARD, tiny);
    expect(p.left).toBe(GAP);
    expect(p.top).toBe(GAP);
  });
});
