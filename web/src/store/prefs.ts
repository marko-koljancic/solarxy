// User preferences: a single persisted zustand store on the
// Minimystix pattern (one localStorage key, versioned, migration hook).
// Four groups per the ratified scope: Appearance (theme + reduced motion),
// Review (opt-in author), Autosave (enable + cadence), Screenshot defaults.
// Theme ownership moved here from ui.ts; the legacy "solarxy.ui.theme" key
// migrates in once.

import { create } from "zustand";
import { persist } from "zustand/middleware";

/** 0.7.1 collapsed three themes into two. The MPW "Balanced Editorial"
 * palette (warm cream + terracotta, the koljam.com design language) used to
 * be a separate "mpw" variant layered over a neutral light theme; it IS the
 * light theme now, on web and on the desktop GUI alike. Stored "mpw" values
 * migrate to "light" (see `migrate` below). */
export type ThemeChoice = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";
export type MotionChoice = "system" | "reduce" | "none";
export type ScreenshotResolution = "viewport" | "1.5x" | "2x" | "4x" | "custom";
export type GizmoOrientation = "world" | "local";

/** The viewport gizmo's drag ergonomics. Pushed into the Rust host (which owns
 * the drag loop) rather than read from it: the drag never crosses back into JS,
 * so it cannot ask. */
export interface GizmoPrefs {
  /** Which frame the Move and Rotate handles align to. Scale is always local
   * (a world-axis scale on a rotated object would need a shear param). */
  orientation: GizmoOrientation;
  /** World units a translate drag snaps to while Ctrl is held. */
  snapTranslate: number;
  /** Degrees a rotate drag snaps to. */
  snapRotate: number;
  /** The increment a scale drag snaps to. */
  snapScale: number;
}

export interface ScreenshotPrefs {
  resolution: ScreenshotResolution;
  customWidth: number;
  customHeight: number;
  overlays: {
    grid: boolean;
    axes: boolean;
    validation: boolean;
  };
}

export type SelectionHighlightStyle = "outline" | "tint" | "none";

export type WireframeWeight = "Light" | "Medium" | "Bold";
/** The built-in pane backgrounds, pinned to
 * `solarxy_core::preferences::BuiltinBg`'s serde names. */
export type BackgroundChoice = "Gradient" | "White" | "DarkGray" | "AyuMirage" | "Black" | "HdriSky";

/** Viewport display defaults, pushed into the Rust host. Wireframe weight
 * and background seed every pane's settings (a scene file's saved per-pane
 * values still win on load; the pane's Display menu stays the live
 * per-pane override). The turntable speed is the live global rpm. */
export interface DisplayPrefs {
  wireframeWeight: WireframeWeight;
  background: BackgroundChoice;
  /** Turntable revolutions per minute, clamped 1..60. */
  turntableRpm: number;
  /** On-screen size of a rendered point, in pixels, clamped 1..32.
   *
   * Global rather than per pane (decision M-27): there is no comparison
   * worth two point sizes side by side. */
  pointSize: number;
}

/** How selection presents in the 3D viewport:
 * the jump-flood rim (default), the legacy translucent tint, or nothing.
 * Pushed into the Rust host like the gizmo ergonomics. */
export interface SelectionPrefs {
  style: SelectionHighlightStyle;
  /** Rim color, sRGB hex; converted to linear at the host boundary. */
  color: string;
  /** Rim width in pixels (clamped 1..16 renderer-side). */
  width: number;
}

export interface Prefs {
  appearance: {
    theme: ThemeChoice;
    /** "system" follows prefers-reduced-motion; "reduce" forces it;
     * "none" keeps animations regardless. */
    reducedMotion: MotionChoice;
  };
  review: {
    /** Written to new annotations; "" = anonymous (attribution is opt-in,
     * never derived from the OS). */
    author: string;
  };
  autosave: {
    enabled: boolean;
    /** Debounce after the last edit, seconds (the 15s force cap stays). */
    debounceSec: number;
  };
  screenshot: ScreenshotPrefs;
  viewport: GizmoPrefs;
  selection: SelectionPrefs;
  display: DisplayPrefs;
  /** First-run tour state. `version` lets a materially changed tour show
   * again to someone who has already seen an older one. */
  onboarding: {
    completed: boolean;
    version: number;
  };
}

export const DEFAULT_PREFS: Prefs = {
  appearance: { theme: "dark", reducedMotion: "system" },
  review: { author: "" },
  autosave: { enabled: true, debounceSec: 2 },
  screenshot: {
    resolution: "viewport",
    customWidth: 1920,
    customHeight: 1080,
    overlays: { grid: true, axes: true, validation: true },
  },
  selection: { style: "outline", color: "#ff9e21", width: 3 },
  display: { wireframeWeight: "Light", background: "Gradient", turntableRpm: 6, pointSize: 6 },
  onboarding: { completed: false, version: 0 },
  viewport: {
    orientation: "world",
    snapTranslate: 0.5,
    snapRotate: 15,
    snapScale: 0.1,
  },
};

interface PrefsStore {
  prefs: Prefs;
  resolvedTheme: ResolvedTheme;
  setPrefs: (prefs: Prefs) => void;
  setTheme: (theme: ThemeChoice) => void;
}

export function resolveTheme(choice: ThemeChoice, systemDark: boolean): ResolvedTheme {
  if (choice === "system") return systemDark ? "dark" : "light";
  return choice;
}

function systemPrefersDark(): boolean {
  return typeof window === "undefined"
    ? true
    : window.matchMedia?.("(prefers-color-scheme: dark)").matches !== false;
}

function systemPrefersReducedMotion(): boolean {
  return typeof window === "undefined"
    ? false
    : window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

/** Whether animations should be suppressed under the given choice. */
export function motionReduced(choice: MotionChoice, systemReduced: boolean): boolean {
  if (choice === "reduce") return true;
  if (choice === "none") return false;
  return systemReduced;
}

function applyBodyClasses(prefs: Prefs): void {
  if (typeof document === "undefined") return;
  const resolved = resolveTheme(prefs.appearance.theme, systemPrefersDark());
  document.body.classList.toggle("light-theme", resolved === "light");
  document.body.classList.toggle("dark-theme", resolved === "dark");
  document.body.classList.toggle(
    "reduce-motion",
    motionReduced(prefs.appearance.reducedMotion, systemPrefersReducedMotion()),
  );
}

/** One-time import of the pre-W4 theme key when no prefs blob exists yet. */
function legacyTheme(): ThemeChoice | null {
  if (typeof localStorage === "undefined") return null;
  const raw = localStorage.getItem("solarxy.ui.theme");
  return raw === "light" || raw === "system" || raw === "dark" ? raw : null;
}

const THEME_CHOICES: readonly string[] = ["dark", "light", "system"];

/** Coerce a stored theme to one that still exists.
 *
 * Defensive rather than clever: `migrate` below handles the known "mpw"
 * case, but a blob hand-edited or written by a future build would otherwise
 * put an unrenderable value on `body`, which shows up as an unthemed page
 * rather than an error. */
export function sanitizeTheme(raw: unknown): ThemeChoice {
  if (raw === "mpw") return "light";
  return THEME_CHOICES.includes(raw as string) ? (raw as ThemeChoice) : DEFAULT_PREFS.appearance.theme;
}

/** Deep-merges a persisted (possibly older) prefs blob over the defaults so
 * new fields backfill on upgrade (the Minimystix onRehydrateStorage
 * pattern): a newly added group needs no persist version bump. Exported for
 * the backfill tests. */
/** A persisted blob: every group optional, and every FIELD within a group
 * optional too.
 *
 * `Partial<Prefs>` would be wrong here, and was: it says a stored group is
 * complete if present, which is false for every blob written by an older
 * version -- exactly the case this function exists to handle. A user
 * upgrading to 0.8.1 has a `display` group with no `pointSize` in it. */
type PersistedPrefs = {
  [K in keyof Prefs]?: Prefs[K] extends object ? Partial<Prefs[K]> : Prefs[K];
};

export function mergePersistedPrefs(p: PersistedPrefs | undefined): Prefs {
  const prefs: Prefs = {
    appearance: { ...DEFAULT_PREFS.appearance, ...p?.appearance },
    review: { ...DEFAULT_PREFS.review, ...p?.review },
    autosave: { ...DEFAULT_PREFS.autosave, ...p?.autosave },
    screenshot: {
      ...DEFAULT_PREFS.screenshot,
      ...p?.screenshot,
      overlays: {
        ...DEFAULT_PREFS.screenshot.overlays,
        ...p?.screenshot?.overlays,
      },
    },
    viewport: { ...DEFAULT_PREFS.viewport, ...p?.viewport },
    selection: { ...DEFAULT_PREFS.selection, ...p?.selection },
    display: { ...DEFAULT_PREFS.display, ...p?.display },
    // An existing user rehydrates with `completed: false` and is offered
    // the tour once.
    onboarding: { ...DEFAULT_PREFS.onboarding, ...p?.onboarding },
  };
  if (!p) {
    const migrated = legacyTheme();
    if (migrated) prefs.appearance.theme = migrated;
  }
  // Belt and braces over `migrate`: a blob already stamped at the
  // current version still gets a renderable theme.
  prefs.appearance.theme = sanitizeTheme(prefs.appearance.theme);
  return prefs;
}

export const usePrefs = create<PrefsStore>()(
  persist(
    (set, get) => ({
      prefs: DEFAULT_PREFS,
      resolvedTheme: resolveTheme(DEFAULT_PREFS.appearance.theme, systemPrefersDark()),
      setPrefs: (prefs) => {
        applyBodyClasses(prefs);
        set({
          prefs,
          resolvedTheme: resolveTheme(prefs.appearance.theme, systemPrefersDark()),
        });
      },
      setTheme: (theme) => {
        const prefs = {
          ...get().prefs,
          appearance: { ...get().prefs.appearance, theme },
        };
        get().setPrefs(prefs);
      },
    }),
    {
      name: "solarxy.prefs",
      // 1 -> 2 (0.7.1): the "mpw" theme became the light theme.
      version: 2,
      // Persist only the prefs blob (resolvedTheme derives).
      partialize: (s) => ({ prefs: s.prefs }),
      migrate: (persisted, version) => {
        const state = persisted as { prefs?: Partial<Prefs> } | undefined;
        if (version < 2 && state?.prefs?.appearance) {
          // Anyone who had selected the MPW variant keeps the palette they
          // chose: it is what "light" now means. Without this they would
          // land on a theme option that no longer exists.
          state.prefs.appearance.theme = sanitizeTheme(state.prefs.appearance.theme);
        }
        return state;
      },
      merge: (persisted, current) => ({
        ...current,
        prefs: mergePersistedPrefs((persisted as { prefs?: Partial<Prefs> } | undefined)?.prefs),
      }),
      onRehydrateStorage: () => (state) => {
        if (state) state.setPrefs(state.prefs);
      },
    },
  ),
);

// Re-apply body classes when the system schemes change under "system".
if (typeof window !== "undefined" && window.matchMedia) {
  const reapply = () => usePrefs.getState().setPrefs(usePrefs.getState().prefs);
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", reapply);
  window.matchMedia("(prefers-reduced-motion: reduce)").addEventListener("change", reapply);
}
