// The scrubber's pure mapping: tick density, tick placement, label
// thinning, and pointer-x to frame. All four are DOM-free by design, so the
// behaviour that decides what the user actually sees is testable without
// rendering anything.

import { describe, expect, it } from "vitest";
import { frameAtX, labelStride, tickFrames, tickStep } from "./TransportTrack";

describe("tickStep", () => {
  it("ticks every frame when there is room", () => {
    // 240 frames across 4000px is ~16px per frame: no thinning needed.
    expect(tickStep(240, 4000)).toBe(1);
  });

  it("climbs the ladder as the track narrows", () => {
    const wide = tickStep(240, 800);
    const narrow = tickStep(240, 200);
    expect(narrow).toBeGreaterThan(wide);
  });

  it("only ever returns a ladder rung, never an arbitrary number", () => {
    const rungs = [1, 2, 5, 10, 25, 50, 100, 250, 500, 1000];
    for (const width of [50, 137, 400, 913, 2000]) {
      for (const frames of [10, 240, 1000, 100_000]) {
        expect(rungs, `${frames} frames at ${width}px`).toContain(tickStep(frames, width));
      }
    }
  });

  it("survives a zero-width track and an empty range", () => {
    // Both happen for real: the first paint before layout, and a one-frame
    // range. Neither may produce NaN or Infinity.
    expect(Number.isFinite(tickStep(240, 0))).toBe(true);
    expect(Number.isFinite(tickStep(0, 800))).toBe(true);
    expect(tickStep(240, 0)).toBeGreaterThan(0);
  });
});

describe("tickFrames", () => {
  it("anchors ticks on multiples of the step, not on the range start", () => {
    // A range starting at 7 should still tick at 10 and 20: the numbers are
    // there to be read, and 7 / 17 / 27 reads as noise.
    expect(tickFrames(7, 30, 10)).toEqual([7, 10, 20, 30]);
  });

  it("always includes both range ends", () => {
    const ticks = tickFrames(1, 240, 50);
    expect(ticks[0]).toBe(1);
    expect(ticks[ticks.length - 1]).toBe(240);
  });

  it("does not duplicate an end that already lands on the step", () => {
    const ticks = tickFrames(0, 100, 50);
    expect(ticks).toEqual([0, 50, 100]);
    expect(new Set(ticks).size).toBe(ticks.length);
  });

  it("collapses a one-frame range to a single tick", () => {
    expect(tickFrames(7, 7, 10)).toEqual([7]);
  });
});

describe("labelStride", () => {
  it("labels every tick when they are far apart", () => {
    expect(labelStride(120)).toBe(1);
  });

  it("thins labels as ticks crowd", () => {
    expect(labelStride(8)).toBeGreaterThan(1);
    expect(labelStride(4)).toBeGreaterThan(labelStride(40));
  });

  it("never returns zero, which would divide by nothing", () => {
    expect(labelStride(0)).toBe(1);
    expect(labelStride(-5)).toBe(1);
  });
});

describe("frameAtX", () => {
  it("maps the track ends onto the range ends", () => {
    expect(frameAtX(0, 400, 1, 241)).toBe(1);
    expect(frameAtX(400, 400, 1, 241)).toBe(241);
  });

  it("rounds to a whole frame: the clock has no in-between", () => {
    for (const x of [17, 63, 118, 355]) {
      expect(Number.isInteger(frameAtX(x, 400, 1, 241))).toBe(true);
    }
  });

  it("clamps a pointer dragged past either edge", () => {
    // Pointer capture keeps the drag alive off the end of the strip, so
    // out-of-range x is the normal case, not an edge case.
    expect(frameAtX(-500, 400, 1, 241)).toBe(1);
    expect(frameAtX(9999, 400, 1, 241)).toBe(241);
  });

  it("returns the start for a degenerate track or range", () => {
    expect(frameAtX(50, 0, 1, 241)).toBe(1);
    expect(frameAtX(50, 400, 7, 7)).toBe(7);
  });

  it("is monotonic across the track", () => {
    let prev = -Infinity;
    for (let x = 0; x <= 400; x += 7) {
      const f = frameAtX(x, 400, 1, 241);
      expect(f).toBeGreaterThanOrEqual(prev);
      prev = f;
    }
  });
});
