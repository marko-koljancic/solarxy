// The drag clamp: a dragged modal must always keep a grabbable corner on
// screen (the resize math is symmetric and exercised in the browser QA).

import { describe, expect, it, vi } from "vitest";
import { clampPos } from "./useDragResize";

describe("clampPos", () => {
  vi.stubGlobal("window", { innerWidth: 1000, innerHeight: 600 });

  it("passes an on-screen position through", () => {
    expect(clampPos(100, 100, 400)).toEqual({ x: 100, y: 100 });
  });

  it("keeps a sliver reachable when dragged off the left edge", () => {
    const { x } = clampPos(-1000, 100, 400);
    expect(x).toBe(8 + 80 - 400);
  });

  it("stops short of the right and bottom edges", () => {
    const c = clampPos(5000, 5000, 400);
    expect(c.x).toBe(1000 - 80);
    expect(c.y).toBe(600 - 40);
  });

  it("never lets the titlebar go above the viewport", () => {
    expect(clampPos(100, -50, 400).y).toBe(8);
  });
});
