// The ui store's pure pieces: layout clamps (theme resolution moved to the
// preferences store; see prefs.test.ts) and the connection-style state.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clampDrawer,
  clampSplit,
  drawerMaxPx,
  loadEdgeStyle,
  useUi,
  DRAWER_MIN_PX,
  EDGE_STYLES,
  SPLIT_MAX_PCT,
  SPLIT_MIN_PCT,
} from "./ui";

describe("layout clamps", () => {
  it("clamps the split to 20-80 percent", () => {
    expect(clampSplit(5)).toBe(SPLIT_MIN_PCT);
    expect(clampSplit(95)).toBe(SPLIT_MAX_PCT);
    expect(clampSplit(50)).toBe(50);
  });

  it("clamps the drawer between the floor and ~85 percent of the window", () => {
    expect(clampDrawer(10)).toBe(DRAWER_MIN_PX);
    expect(clampDrawer(99999)).toBe(drawerMaxPx());
    expect(clampDrawer(280)).toBe(280);
  });
});

describe("connection style", () => {
  // The node test environment has no localStorage; a Map-backed stub lets
  // the setters' persistence writes be asserted directly.
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("falls back to bezier on a missing or invalid persisted value", () => {
    expect(loadEdgeStyle()).toBe("bezier");
    localStorage.setItem("solarxy.ui.edgeStyle", "zigzag");
    expect(loadEdgeStyle()).toBe("bezier");
    localStorage.setItem("solarxy.ui.edgeStyle", "smoothStep");
    expect(loadEdgeStyle()).toBe("smoothStep");
  });

  it("cycles through all four styles in order and wraps, persisting each", () => {
    useUi.getState().setEdgeStyle("bezier");
    const seen = [useUi.getState().edgeStyle];
    for (let i = 0; i < EDGE_STYLES.length; i += 1) {
      useUi.getState().cycleEdgeStyle();
      seen.push(useUi.getState().edgeStyle);
    }
    expect(seen).toEqual(["bezier", "straight", "simpleBezier", "smoothStep", "bezier"]);
    expect(localStorage.getItem("solarxy.ui.edgeStyle")).toBe("bezier");
  });
});
