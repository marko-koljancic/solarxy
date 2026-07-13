// The radial ring's anchor math (Phase 10). Before this, the ring captured the
// node's rect once at open time and drifted off it on pan and zoom; the ring now
// re-measures every render, and this is the geometry it derives.

import { describe, expect, it } from "vitest";
import { RADIUS_CAP_PX, radialAnchor } from "./radialAnchor";

describe("radialAnchor", () => {
  it("centres the ring on the node's screen box", () => {
    const a = radialAnchor({ left: 100, top: 50, width: 40, height: 20 });
    expect(a.cx).toBe(120);
    expect(a.cy).toBe(60);
  });

  it("hugs the node: the inner radius is its half-extent", () => {
    expect(radialAnchor({ left: 0, top: 0, width: 40, height: 20 }).radius).toBe(20);
    expect(radialAnchor({ left: 0, top: 0, width: 20, height: 60 }).radius).toBe(30);
  });

  it("grows with the node as the canvas zooms in (the rect already has zoom in it)", () => {
    const atZoom1 = radialAnchor({ left: 100, top: 50, width: 48, height: 48 });
    const atZoom2 = radialAnchor({ left: 200, top: 100, width: 96, height: 96 });
    expect(atZoom1.radius).toBe(24);
    expect(atZoom2.radius).toBe(48);
    // and stays centred on the node at both
    expect(atZoom2.cx).toBe(248);
    expect(atZoom2.cy).toBe(148);
  });

  it("caps the inner radius so a zoomed-in node cannot push the ring off screen", () => {
    expect(radialAnchor({ left: 0, top: 0, width: 400, height: 400 }).radius).toBe(RADIUS_CAP_PX);
  });

  it("follows the node when the canvas pans (the rect moves with it)", () => {
    const before = radialAnchor({ left: 100, top: 50, width: 40, height: 40 });
    const afterPan = radialAnchor({ left: 130, top: 35, width: 40, height: 40 });
    expect(afterPan.cx - before.cx).toBe(30);
    expect(afterPan.cy - before.cy).toBe(-15);
    expect(afterPan.radius).toBe(before.radius);
  });
});
