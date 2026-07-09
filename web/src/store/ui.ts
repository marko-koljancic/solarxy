// UI chrome state: the theme choice (dark / light / system, carried from
// Minimystix per the UX spec section 19) and the resizable-layout state
// (viewport/canvas split, properties drawer, viewport maximize). Persisted
// to localStorage; pure presentation, never document truth.

import { create } from "zustand";

export type ThemeChoice = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

const THEME_KEY = "solarxy.ui.theme";
const SPLIT_KEY = "solarxy.ui.splitPct";
const DRAWER_KEY = "solarxy.ui.drawerHeight";

/** Clamp bounds carried from Minimystix (variables.css / PropertiesDrawer). */
export const SPLIT_MIN_PCT = 20;
export const SPLIT_MAX_PCT = 80;
export const DRAWER_MIN_PX = 100;
export const DRAWER_MAX_PX = 600;

export function clampSplit(pct: number): number {
  return Math.min(SPLIT_MAX_PCT, Math.max(SPLIT_MIN_PCT, pct));
}

export function clampDrawer(px: number): number {
  return Math.min(DRAWER_MAX_PX, Math.max(DRAWER_MIN_PX, px));
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

function applyBodyClass(resolved: ResolvedTheme): void {
  if (typeof document === "undefined") return;
  document.body.classList.toggle("light-theme", resolved === "light");
  document.body.classList.toggle("dark-theme", resolved === "dark");
}

interface UiState {
  theme: ThemeChoice;
  resolvedTheme: ResolvedTheme;
  /** Viewport share of the horizontal split, in percent (20-80). */
  splitPct: number;
  drawerHeight: number;
  drawerCollapsed: boolean;
  viewportMaximized: boolean;
  setTheme: (t: ThemeChoice) => void;
  setSplitPct: (pct: number) => void;
  setDrawerHeight: (px: number) => void;
  toggleDrawerCollapsed: () => void;
  toggleViewportMaximized: () => void;
}

function loadNumber(key: string, fallback: number): number {
  if (typeof localStorage === "undefined") return fallback;
  const raw = Number(localStorage.getItem(key));
  return Number.isFinite(raw) && raw > 0 ? raw : fallback;
}

function loadTheme(): ThemeChoice {
  if (typeof localStorage === "undefined") return "dark";
  const raw = localStorage.getItem(THEME_KEY);
  return raw === "light" || raw === "system" ? raw : "dark";
}

export const useUi = create<UiState>((set, get) => {
  const initialTheme = loadTheme();
  const initialResolved = resolveTheme(initialTheme, systemPrefersDark());
  applyBodyClass(initialResolved);

  // System changes retarget only the "system" choice.
  if (typeof window !== "undefined" && window.matchMedia) {
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => {
      if (get().theme === "system") {
        const resolved = resolveTheme("system", e.matches);
        applyBodyClass(resolved);
        set({ resolvedTheme: resolved });
      }
    });
  }

  return {
    theme: initialTheme,
    resolvedTheme: initialResolved,
    splitPct: clampSplit(loadNumber(SPLIT_KEY, 55)),
    drawerHeight: clampDrawer(loadNumber(DRAWER_KEY, 280)),
    drawerCollapsed: false,
    viewportMaximized: false,
    setTheme: (t) => {
      localStorage.setItem(THEME_KEY, t);
      const resolved = resolveTheme(t, systemPrefersDark());
      applyBodyClass(resolved);
      set({ theme: t, resolvedTheme: resolved });
    },
    setSplitPct: (pct) => {
      const clamped = clampSplit(pct);
      localStorage.setItem(SPLIT_KEY, String(clamped));
      set({ splitPct: clamped });
    },
    setDrawerHeight: (px) => {
      const clamped = clampDrawer(px);
      localStorage.setItem(DRAWER_KEY, String(clamped));
      set({ drawerHeight: clamped });
    },
    toggleDrawerCollapsed: () => set((s) => ({ drawerCollapsed: !s.drawerCollapsed })),
    toggleViewportMaximized: () => set((s) => ({ viewportMaximized: !s.viewportMaximized })),
  };
});
