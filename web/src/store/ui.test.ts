// The ui store's pure pieces: theme resolution and layout clamps.

import { describe, expect, it } from "vitest";
import {
  clampDrawer,
  clampSplit,
  DRAWER_MAX_PX,
  DRAWER_MIN_PX,
  resolveTheme,
  SPLIT_MAX_PCT,
  SPLIT_MIN_PCT,
} from "./ui";

describe("resolveTheme", () => {
  it("passes explicit choices through", () => {
    expect(resolveTheme("dark", false)).toBe("dark");
    expect(resolveTheme("light", true)).toBe("light");
  });

  it("resolves system from the media query", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });
});

describe("layout clamps", () => {
  it("clamps the split to 20-80 percent", () => {
    expect(clampSplit(5)).toBe(SPLIT_MIN_PCT);
    expect(clampSplit(95)).toBe(SPLIT_MAX_PCT);
    expect(clampSplit(50)).toBe(50);
  });

  it("clamps the drawer to 100-600 px", () => {
    expect(clampDrawer(10)).toBe(DRAWER_MIN_PX);
    expect(clampDrawer(9999)).toBe(DRAWER_MAX_PX);
    expect(clampDrawer(280)).toBe(280);
  });
});
