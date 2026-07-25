// Preferences: defaults, theme/motion resolution, and the deep-merge that
// backfills new fields when an older persisted blob rehydrates.

import { describe, expect, it } from "vitest";
import {
  DEFAULT_PREFS,
  mergePersistedPrefs,
  motionReduced,
  resolveTheme,
  sanitizeTheme,
} from "./prefs";

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

  it("ship the Stage 8 display defaults (Light on Gradient at 6 rpm)", () => {
    // Light is the ratified default (and the desktop's); the previous web
    // build hardcoded Medium in the host, which this preference replaces.
    expect(DEFAULT_PREFS.display).toEqual({
      wireframeWeight: "Light",
      background: "Gradient",
      turntableRpm: 6,
    });
  });
});

describe("rehydration backfill", () => {
  it("a stored blob without the display group backfills it from defaults", () => {
    // The deep merge is what makes a new group need no persist version
    // bump; a pre-Stage-8 blob rehydrates with the display defaults.
    const prefs = mergePersistedPrefs({ appearance: { theme: "light", reducedMotion: "system" } });
    expect(prefs.appearance.theme).toBe("light");
    expect(prefs.display).toEqual(DEFAULT_PREFS.display);
  });

  it("a stored display group survives the merge", () => {
    const prefs = mergePersistedPrefs({
      display: { wireframeWeight: "Bold", background: "Black", turntableRpm: 12 },
    });
    expect(prefs.display).toEqual({
      wireframeWeight: "Bold",
      background: "Black",
      turntableRpm: 12,
    });
  });
});
