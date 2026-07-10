// Desk snapshots: capture/apply round-trip shape and the sanitize clamps.

import { describe, expect, it } from "vitest";
import { captureDesk, sanitizeDesk, type DeskSnapshot } from "./desks";
import { DRAWER_MIN_PX, DRAWER_WIDTH_MIN_PX, SPLIT_MAX_PCT, SPLIT_MIN_PCT } from "./ui";

const UI = {
  viewportSide: "right" as const,
  propertiesDock: "right" as const,
  splitPct: 63,
  drawerHeight: 300,
  drawerWidth: 380,
  showFlowGrid: false,
  showMinimap: true,
  showFlowControls: false,
};

describe("captureDesk", () => {
  it("snapshots the arrangement and survives a JSON round trip", () => {
    const desk = captureDesk("My Desk", UI, "quad");
    const back = JSON.parse(JSON.stringify(desk)) as DeskSnapshot;
    expect(back).toEqual({
      name: "My Desk",
      viewportSide: "right",
      propertiesDock: "right",
      splitPct: 63,
      drawerHeight: 300,
      drawerWidth: 380,
      showFlowGrid: false,
      showMinimap: true,
      showFlowControls: false,
      viewLayout: "quad",
    });
    expect(sanitizeDesk(back)).toEqual(back);
  });
});

describe("sanitizeDesk", () => {
  it("clamps out-of-range sizes and falls back on bad enums", () => {
    const wild = {
      ...captureDesk("x", UI, "single"),
      viewportSide: "up" as never,
      propertiesDock: "floating" as never,
      splitPct: 5,
      drawerHeight: 1,
      drawerWidth: 1,
    };
    const clean = sanitizeDesk(wild);
    expect(clean.viewportSide).toBe("left");
    expect(clean.propertiesDock).toBe("bottom");
    expect(clean.splitPct).toBe(SPLIT_MIN_PCT);
    expect(clean.drawerHeight).toBe(DRAWER_MIN_PX);
    expect(clean.drawerWidth).toBe(DRAWER_WIDTH_MIN_PX);
    expect(sanitizeDesk({ ...wild, splitPct: 99 }).splitPct).toBe(SPLIT_MAX_PCT);
  });
});
