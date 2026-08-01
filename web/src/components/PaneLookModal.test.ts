// The pane Look dialog's constants, which are mirrors of Rust values and
// therefore the part that can silently drift.

import { describe, expect, it } from "vitest";
import type { PaneLook } from "../engine/types";
import { NEUTRAL, TONE_MODES } from "./PaneLookModal";

describe("the neutral pane look", () => {
  // These are the values `PaneLook::default()` produces in
  // `solarxy_core::view_config`. Neutral has to be exactly neutral rather
  // than approximately so: the renderer skips the grade entirely at these
  // values, and a Reset that wrote 0.999 would quietly turn it back on.
  it("matches the Rust defaults exactly", () => {
    expect(NEUTRAL.exposure).toBe(1);
    expect(NEUTRAL.toneMode).toBe("AcesFilmic");
    expect(NEUTRAL.lift).toEqual([0, 0, 0]);
    expect(NEUTRAL.gamma).toEqual([1, 1, 1]);
    expect(NEUTRAL.gain).toEqual([1, 1, 1]);
  });
});

describe("the tone map options", () => {
  // The values are sent to the host verbatim and deserialized into a Rust
  // enum, so a typo here is a rejected command rather than a wrong render.
  it("covers every variant of the Rust enum and nothing else", () => {
    const values = TONE_MODES.map(([v]) => v);
    const expected: PaneLook["toneMode"][] = ["None", "Linear", "Reinhard", "AcesFilmic"];
    expect(values).toEqual(expected);
  });

  it("labels every option", () => {
    for (const [value, label] of TONE_MODES) {
      expect(value.length).toBeGreaterThan(0);
      expect(label.length).toBeGreaterThan(0);
    }
  });
});
