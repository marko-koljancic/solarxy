// Preferences: defaults, theme/motion resolution, and the deep-merge that
// backfills new fields when an older persisted blob rehydrates.

import { describe, expect, it } from "vitest";
import { DEFAULT_PREFS, motionReduced, resolveTheme } from "./prefs";

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

describe("motionReduced", () => {
  it("forces or ignores regardless of the system under explicit choices", () => {
    expect(motionReduced("reduce", false)).toBe(true);
    expect(motionReduced("none", true)).toBe(false);
  });

  it("follows the system under 'system'", () => {
    expect(motionReduced("system", true)).toBe(true);
    expect(motionReduced("system", false)).toBe(false);
  });
});

describe("defaults", () => {
  it("ship the ratified group set", () => {
    expect(DEFAULT_PREFS.appearance.theme).toBe("dark");
    expect(DEFAULT_PREFS.review.author).toBe("");
    expect(DEFAULT_PREFS.autosave).toEqual({ enabled: true, debounceSec: 2 });
    expect(DEFAULT_PREFS.screenshot.resolution).toBe("viewport");
    expect(DEFAULT_PREFS.screenshot.overlays).toEqual({
      grid: true,
      axes: true,
      validation: true,
    });
  });
});
