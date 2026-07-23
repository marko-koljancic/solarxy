// The attribute-pin channel's pure text logic (slot patching is DOM-side
// and exercised in the browser QA pass).

import { describe, expect, it } from "vitest";
import { fmtPinValue, pinText } from "./attrPins";
import type { AttrPin } from "./types";

const pin = (over: Partial<AttrPin>): AttrPin => ({
  pane: 0,
  x: 0,
  y: 0,
  ptnum: 7,
  slot: 0,
  value: null,
  ...over,
});

describe("fmtPinValue", () => {
  it("rounds to two decimals and joins components", () => {
    expect(fmtPinValue([0.125])).toBe("0.13");
    expect(fmtPinValue([1, 0.5, 0.25])).toBe("1, 0.5, 0.25");
  });
  it("normalizes negative zero", () => {
    expect(fmtPinValue([-0.001])).toBe("0");
  });
});

describe("pinText", () => {
  it("shows the value in labels mode", () => {
    expect(pinText(pin({ value: [0.5] }), true, false)).toBe("0.5");
  });
  it("shows the point number in points mode", () => {
    expect(pinText(pin({}), false, true)).toBe("7");
  });
  it("combines both when both modes are on", () => {
    expect(pinText(pin({ value: [0.5] }), true, true)).toBe("7: 0.5");
  });
  it("falls back to the point number when the lane is absent", () => {
    expect(pinText(pin({ value: null }), true, false)).toBe("7");
    expect(pinText(pin({ value: [] }), true, true)).toBe("7");
  });
});
