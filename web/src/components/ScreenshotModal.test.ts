// The preset dimension helper: physical-pixel math over the active pane.

import { describe, expect, it } from "vitest";
import { screenshotDims } from "./ScreenshotModal";

const pane = { width: 800, height: 600 };

describe("screenshotDims", () => {
  it("viewport preset uses the pane rect times dpr", () => {
    expect(screenshotDims("viewport", pane, 2, 0, 0)).toEqual({ width: 1600, height: 1200 });
  });

  it("multiplier presets scale the physical rect", () => {
    expect(screenshotDims("2x", pane, 1, 0, 0)).toEqual({ width: 1600, height: 1200 });
    expect(screenshotDims("1.5x", pane, 2, 0, 0)).toEqual({ width: 2400, height: 1800 });
    expect(screenshotDims("4x", pane, 1, 0, 0)).toEqual({ width: 3200, height: 2400 });
  });

  it("custom ignores the pane and clamps to a sane floor", () => {
    expect(screenshotDims("custom", pane, 2, 3840, 2160)).toEqual({ width: 3840, height: 2160 });
    expect(screenshotDims("custom", pane, 1, 1, 1)).toEqual({ width: 16, height: 16 });
  });
});
