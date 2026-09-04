// The six cases `render_watch/view.rs` asserts, ported with the transform they
// cover. They are meant to be read against the Rust ones: this is the only
// thing keeping the browser's render window and the terminal's watch window
// behaving the same way under a drag and a wheel.

import { describe, expect, it } from "vitest";
import {
  MIN_VISIBLE,
  ZOOM_CEILING,
  ZOOM_FLOOR_OF_FIT,
  actualSize,
  fitScale,
  letterbox,
  pan,
  viewRect,
  zoomAbout,
  zoomOf,
  type ViewMode,
} from "./view";

const size = (w: number, h: number) => ({ w, h });

describe("the letterbox", () => {
  it("keeps the picture's proportions", () => {
    expect(letterbox(size(400, 100), size(800, 800))).toMatchObject({
      w: 800,
      h: 200,
      y: 300,
    });
    expect(letterbox(size(100, 400), size(800, 800))).toMatchObject({
      w: 200,
      h: 800,
      x: 300,
    });
    // An exact fit leaves no bars at all.
    expect(letterbox(size(640, 480), size(1280, 960))).toMatchObject({
      x: 0,
      y: 0,
      w: 1280,
      h: 960,
    });
  });
});

describe("the view", () => {
  it("returns to the fit on reset", () => {
    const picture = size(400, 300);
    const window = size(800, 600);
    const fit = viewRect(null, picture, window);

    const zoomed = zoomAbout(null, { x: 100, y: 100 }, 2, picture, window);
    expect(viewRect(zoomed, picture, window)).not.toEqual(fit);
    // `null` is the fit, so resetting is discarding the mode.
    expect(viewRect(null, picture, window)).toEqual(fit);
  });

  it("holds the pixel under the cursor still through a zoom", () => {
    const picture = size(400, 300);
    const window = size(800, 600);
    const cursor = { x: 250, y: 220 };

    const before = viewRect(null, picture, window);
    const at = {
      x: (cursor.x - before.x) / before.w,
      y: (cursor.y - before.y) / before.h,
    };

    const after = viewRect(zoomAbout(null, cursor, 2.5, picture, window), picture, window);
    expect((cursor.x - after.x) / after.w).toBeCloseTo(at.x, 2);
    expect((cursor.y - after.y) / after.h).toBeCloseTo(at.y, 2);
  });

  it("clamps the zoom at both ends", () => {
    const picture = size(400, 300);
    const window = size(800, 600);
    const cursor = { x: 400, y: 300 };

    let mode: ViewMode = null;
    for (let i = 0; i < 40; i += 1) mode = zoomAbout(mode, cursor, 4, picture, window);
    expect(zoomOf(mode, picture, window)).toBeCloseTo(ZOOM_CEILING, 2);

    mode = null;
    for (let i = 0; i < 40; i += 1) mode = zoomAbout(mode, cursor, 0.25, picture, window);
    expect(zoomOf(mode, picture, window)).toBeCloseTo(fitScale(picture, window) * ZOOM_FLOOR_OF_FIT, 3);
  });

  it("moves on a pan and cannot lose the picture", () => {
    const picture = size(400, 300);
    const window = size(800, 600);
    const before = viewRect(null, picture, window);

    const moved = viewRect(pan(null, { x: 10, y: 20 }, picture, window), picture, window);
    expect(moved.x).toBeCloseTo(before.x + 10, 3);
    expect(moved.y).toBeCloseTo(before.y + 20, 3);

    const far = viewRect(pan(null, { x: 1e6, y: 1e6 }, picture, window), picture, window);
    expect(far.x).toBeLessThanOrEqual(window.w - MIN_VISIBLE);
    expect(far.y).toBeLessThanOrEqual(window.h - MIN_VISIBLE);

    const back = viewRect(pan(null, { x: -1e6, y: -1e6 }, picture, window), picture, window);
    expect(back.x + back.w).toBeGreaterThanOrEqual(MIN_VISIBLE);
    expect(back.y + back.h).toBeGreaterThanOrEqual(MIN_VISIBLE);
  });

  it("recomputes the fit for a new window", () => {
    const picture = size(400, 300);
    // The same view, asked about two windows: the fit is not stored, so a
    // resize refits with no event handler involved.
    const small = viewRect(null, picture, size(800, 600));
    const large = viewRect(null, picture, size(1600, 1200));
    expect(large.w).toBeCloseTo(small.w * 2, 3);
  });

  // Beyond the port: the terminal sizes its window to the picture, so a
  // hundred percent is where it starts and it needs no action for it.
  it("puts one image pixel on one window pixel, centred", () => {
    const picture = size(200, 100);
    const window = size(800, 600);
    const rect = viewRect(actualSize(picture, window), picture, window);
    expect(rect.w).toBeCloseTo(200, 3);
    expect(rect.h).toBeCloseTo(100, 3);
    expect(rect.x + rect.w / 2).toBeCloseTo(400, 3);
    expect(rect.y + rect.h / 2).toBeCloseTo(300, 3);
  });

  it("survives a degenerate size rather than dividing by zero", () => {
    const rect = viewRect(null, size(0, 0), size(0, 0));
    expect(Number.isFinite(rect.w)).toBe(true);
    expect(Number.isFinite(rect.h)).toBe(true);
  });
});
