// User preferences (Phase 7 W4): a single persisted zustand store on the
// Minimystix pattern (one localStorage key, versioned, migration hook).
// Four groups per the ratified scope: Appearance (theme + reduced motion),
// Review (opt-in author), Autosave (enable + cadence), Screenshot defaults.
// Theme ownership moved here from ui.ts; the legacy "solarxy.ui.theme" key
// migrates in once.

import { create } from "zustand";
import { persist } from "zustand/middleware";

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
      version: 1,
      // Persist only the prefs blob (resolvedTheme derives).
      partialize: (s) => ({ prefs: s.prefs }),
      merge: (persisted, current) => {
        // Deep-merge over the defaults so new fields backfill on upgrade
        // (the Minimystix onRehydrateStorage pattern).
        const p = (persisted as { prefs?: Partial<Prefs> } | undefined)?.prefs;
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
          // Backfilled by the same deep merge, so no version bump is needed for
          // the group added in Phase 12.
          viewport: { ...DEFAULT_PREFS.viewport, ...p?.viewport },
        };
        if (!p) {
          const migrated = legacyTheme();
          if (migrated) prefs.appearance.theme = migrated;
        }
        return { ...current, prefs };
      },
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
