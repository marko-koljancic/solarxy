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
    // Round 2: the shipped default follows the OS rather than forcing dark.
    // The stored VALUE stays "system" (the label reads "Device"), so every
    // preference blob already on disk keeps working.
    expect(DEFAULT_PREFS.appearance.theme).toBe("system");
    expect(DEFAULT_PREFS.review.author).toBe("");
    expect(DEFAULT_PREFS.autosave).toEqual({ enabled: true, debounceSec: 2 });
    expect(DEFAULT_PREFS.screenshot.resolution).toBe("viewport");
    expect(DEFAULT_PREFS.screenshot.overlays).toEqual({
      grid: true,
      axes: true,
      validation: true,
    });
  });

  it("ship the display defaults (Light on Gradient, 6 rpm, 6 px points)", () => {
    // Light is the ratified default (and the desktop's); the previous web
    // build hardcoded Medium in the host, which this preference replaces.
    // pointSize joined in 0.8.1 and matches the renderer's own default, so
    // turning the preference on changes nothing until you move it.
    expect(DEFAULT_PREFS.display).toEqual({
      wireframeWeight: "Light",
      background: "Gradient",
      turntableRpm: 6,
      pointSize: 6,
      // The label defaults likewise match `LabelStyle::new_default()` in
      // the renderer, so they are a no-op until somebody moves them.
      labelSize: "medium",
      labelBackground: "chip",
      labelOpacity: 1,
      labelDecimals: 2,
      // On by default, matching the desktop's shipped preferences; the web
      // host booted both hard-off for six releases, which left AO Preview
      // a white screen in every browser.
      ssaoEnabled: true,
      bloomEnabled: true,
    });
  });
});

describe("rehydration backfill", () => {
  it("a stored blob without the display group backfills it from defaults", () => {
    // The deep merge is what makes a new group need no persist version
    // bump; a blob stored before the group existed rehydrates with the
    // display defaults.
    const prefs = mergePersistedPrefs({ appearance: { theme: "light", reducedMotion: "system" } });
    expect(prefs.appearance.theme).toBe("light");
    expect(prefs.display).toEqual(DEFAULT_PREFS.display);
  });

  it("a stored display group survives the merge", () => {
    const prefs = mergePersistedPrefs({
      display: { wireframeWeight: "Bold", background: "Black", pointSize: 6, turntableRpm: 12 },
    });
    // Stored fields win; anything the blob predates backfills. Asserted
    // field-by-field rather than against a whole-object literal, so adding
    // a display default does not falsely fail this test -- the backfill
    // cases below are what guard the new fields.
    expect(prefs.display.wireframeWeight).toBe("Bold");
    expect(prefs.display.background).toBe("Black");
    expect(prefs.display.turntableRpm).toBe(12);
    expect(prefs.display.pointSize).toBe(6);
  });

  it("backfills pointSize for a blob persisted before 0.8.1", () => {
    // The real upgrade path: every existing user's stored prefs predate the
    // field, and a missing one must become the default rather than 0, which
    // would render every point invisible.
    const prefs = mergePersistedPrefs({
      display: { wireframeWeight: "Bold", background: "Black", turntableRpm: 12 },
    });
    expect(prefs.display.pointSize).toBe(6);
    expect(prefs.display.turntableRpm).toBe(12);
  });

  it("backfills the attribute-label defaults for a blob that predates them", () => {
    // Same upgrade path as pointSize. A missing opacity becoming 0 would
    // render every label invisible, and a missing decimals becoming 0 would
    // silently round every value to an integer.
    const prefs = mergePersistedPrefs({
      display: { wireframeWeight: "Bold", background: "Black", turntableRpm: 12 },
    });
    expect(prefs.display.labelSize).toBe("medium");
    expect(prefs.display.labelBackground).toBe("chip");
    expect(prefs.display.labelOpacity).toBe(1);
    expect(prefs.display.labelDecimals).toBe(2);
  });

  it("backfills the viewport-chrome group, which no stored blob has yet", () => {
    const prefs = mergePersistedPrefs({ appearance: { theme: "light", reducedMotion: "system" } });
    expect(prefs.chrome.transportBar).toBe(true);
  });
});
