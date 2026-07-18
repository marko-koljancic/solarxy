// The tour script's self-consistency. The companion check that every
// `target` selector matches a real class in web/src lives in Rust
// (`solarxy-core/tests/tokens_drift.rs`, `tour_steps_point_at_real_classes`)
// because this test graph has no node:fs types; vitest holds the pure half.

import { describe, expect, it } from "vitest";
import { TOUR_STEPS } from "./steps";

describe("tour steps", () => {
  it("have unique ids", () => {
    const ids = TOUR_STEPS.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("carry real copy, not placeholders", () => {
    for (const s of TOUR_STEPS) {
      expect(s.title.length, s.id).toBeGreaterThan(3);
      expect(s.body.length, s.id).toBeGreaterThan(40);
      expect(s.target.trim().length, s.id).toBeGreaterThan(1);
    }
  });

  it("keep the script walkable in one sitting", () => {
    expect(TOUR_STEPS.length).toBeLessThanOrEqual(8);
    expect(TOUR_STEPS.length).toBeGreaterThanOrEqual(4);
  });
});
