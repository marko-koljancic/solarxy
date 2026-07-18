// The precision-drag math: deadzone, hysteresis, decade clamping, and
// value scrubbing (section 7 numbers).

import { describe, expect, it } from "vitest";
import { DEADZONE, HYSTERESIS, ROW_HEIGHT, scrubValue, selectDecade } from "./usePrecisionDrag";

describe("selectDecade", () => {
  it("ignores motion inside the deadzone", () => {
    const r = selectDecade(DEADZONE - 1, 100, 2, 100);
    expect(r.index).toBe(2);
    expect(r.changeY).toBe(100);
  });

  it("moves one row per ROW_HEIGHT past the deadzone", () => {
    // deltaY of one full row selects index 3 (down = finer is one below
    // the default index 2).
    const r = selectDecade(ROW_HEIGHT, 100 + ROW_HEIGHT, 2, 100);
    expect(r.index).toBe(3);
    expect(r.changeY).toBe(100 + ROW_HEIGHT);
  });

  it("clamps to the decade list bounds", () => {
    expect(selectDecade(-10 * ROW_HEIGHT, 0, 2, 999).index).toBe(0);
    expect(selectDecade(10 * ROW_HEIGHT, 999, 2, 0).index).toBe(5);
  });

  it("requires the hysteresis distance since the last change", () => {
    // Candidate differs but the cursor is within HYSTERESIS of the last
    // change point: the index holds.
    const r = selectDecade(ROW_HEIGHT, 100, 2, 100 - (HYSTERESIS - 1));
    expect(r.index).toBe(2);
    // Beyond the hysteresis distance it moves.
    const r2 = selectDecade(ROW_HEIGHT, 100, 2, 100 - HYSTERESIS);
    expect(r2.index).toBe(3);
  });
});

describe("scrubValue", () => {
  it("scales horizontal motion by decade and sensitivity", () => {
    expect(scrubValue(1.0, 100, 0.01)).toBeCloseTo(1.5);
    expect(scrubValue(1.0, -100, 0.1)).toBeCloseTo(-4.0);
  });

  it("clamps to the hard range", () => {
    expect(scrubValue(1.0, 10_000, 1, { max: 5 })).toBe(5);
    expect(scrubValue(1.0, -10_000, 1, { min: 0 })).toBe(0);
  });

  it("snaps ints", () => {
    expect(scrubValue(3, 3, 1, { int: true })).toBe(5);
    expect(Number.isInteger(scrubValue(3, 7, 0.1, { int: true }))).toBe(true);
  });
});
