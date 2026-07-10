// The ui store's pure pieces: layout clamps (theme resolution moved to the
// preferences store; see prefs.test.ts).

import { describe, expect, it } from "vitest";
import {
  clampDrawer,
  clampSplit,
  drawerMaxPx,
  DRAWER_MIN_PX,
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
