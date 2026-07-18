// Preferences: defaults, theme/motion resolution, and the deep-merge that
// backfills new fields when an older persisted blob rehydrates.

import { describe, expect, it } from "vitest";
import { DEFAULT_PREFS, motionReduced, resolveTheme, sanitizeTheme } from "./prefs";

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

describe("sanitizeTheme", () => {
  // 0.7.1 collapsed "mpw" into "light". Anyone who had selected the MPW
  // variant must keep the palette they picked rather than being bounced to
  // a theme option that no longer exists.
  it("migrates the retired mpw choice to light", () => {
    expect(sanitizeTheme("mpw")).toBe("light");
  });

  it("passes the surviving choices through", () => {
    expect(sanitizeTheme("dark")).toBe("dark");
    expect(sanitizeTheme("light")).toBe("light");
    expect(sanitizeTheme("system")).toBe("system");
  });

  // A blob hand-edited or written by a future build would otherwise put an
  // unrenderable value on `body`, which reads as an unthemed page rather
  // than an error.
  it("falls back to the default for anything unrecognised", () => {
    for (const junk of ["solarized", "", null, undefined, 7, {}]) {
      expect(sanitizeTheme(junk)).toBe(DEFAULT_PREFS.appearance.theme);
    }
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
