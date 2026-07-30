// Shared behaviour for the public pages: theme choice, scroll reveals, the
// accent progress rail, and the footer year. Every public page loads exactly
// one module entry, because the production CSP allows no inline script.
//
// Kept vanilla on purpose. These pages are marketing and reference surfaces,
// not the editor, and their JavaScript counts against the same size budget the
// editor boot is measured by.

const THEME_KEY = "sx-theme";

export const reducedMotion = (): boolean =>
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** The stored theme choice, or null when the page follows the system. */
function storedTheme(): string | null {
  try {
    return localStorage.getItem(THEME_KEY);
  } catch {
    // Private-mode and blocked-storage browsers fall back to the system theme.
    return null;
  }
}

/**
 * Applies any stored theme and wires the toggle. Called before first paint
 * would be ideal, but an inline script is not available under the CSP, so the
 * module is loaded early and the flash is limited to one frame.
 */
export function initTheme(): void {
  const root = document.documentElement;
  const saved = storedTheme();
  if (saved === "dark" || saved === "light") root.dataset.theme = saved;

  const isDark = (): boolean => {
    const t = root.dataset.theme;
    if (t) return t === "dark";
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  };

  document.getElementById("themeBtn")?.addEventListener("click", () => {
    root.dataset.theme = isDark() ? "light" : "dark";
    try {
      localStorage.setItem(THEME_KEY, root.dataset.theme);
    } catch {
      // A rejected write only costs persistence, not the switch itself.
    }
  });
}

/** Reveals elements as they scroll in. Everything is shown at once when motion
 * is reduced or the observer is unavailable, so content never depends on it. */
export function initReveals(): void {
  const els = Array.from(document.querySelectorAll<HTMLElement>(".reveal"));
  if (reducedMotion() || !("IntersectionObserver" in window)) {
    els.forEach((el) => el.classList.add("in"));
    return;
  }
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) {
          (e.target as HTMLElement).classList.add("in");
          io.unobserve(e.target);
        }
      }
    },
    { rootMargin: "0px 0px -8% 0px", threshold: 0.06 },
  );
  els.forEach((el) => io.observe(el));
}

/**
 * Drives the accent progress rail from scroll position.
 * Returns the scroll handler so a caller can re-run it after changing the
 * document height (the roadmap page does this when it switches views).
 */
export function initRail(): () => void {
  const rail = document.getElementById("rail");
  let ticking = false;
  const onScroll = (): void => {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(() => {
      const h = document.documentElement.scrollHeight - window.innerHeight;
      const p = h > 0 ? Math.min(1, window.scrollY / h) : 0;
      if (rail) rail.style.transform = `scaleX(${p})`;
      ticking = false;
    });
  };
  window.addEventListener("scroll", onScroll, { passive: true });
  onScroll();
  return onScroll;
}

export function initYear(): void {
  const year = document.getElementById("year");
  if (year) year.textContent = String(new Date().getFullYear());
}
