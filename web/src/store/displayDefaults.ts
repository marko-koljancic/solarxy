// Viewport display defaults, deliberately free of every dependency.
//
// This exists so the PLAYER can read a default without importing the
// preferences store. That is not fastidiousness: `store/prefs.ts` is
// zustand-backed, zustand pulls React, and Vite has React inside the `dock`
// chunk, so one value import from the player dragged 431 KB of React and
// dockview into a published scene bundle. The player never renders a single
// React component.
//
// Anything in here must stay importable by a page that has no framework at
// all, so this file imports nothing and must keep importing nothing.
// `web/src/export/playerBundle.test.ts` fails the build if the player's graph
// picks up React or dockview again.

export type WireframeWeight = "Light" | "Medium" | "Bold";

/** Named backgrounds; `HdriSky` renders the loaded environment. */
export type BackgroundChoice =
  | "Gradient"
  | "White"
  | "DarkGray"
  | "AyuMirage"
  | "Black"
  | "HdriSky";

/** Attribute-label appearance. Mirrors `solarxy_renderer::labels`, but as
 * plain string unions so this file keeps its no-import promise. */
export type LabelSizeChoice = "small" | "medium" | "large";
export type LabelBackgroundChoice = "chip" | "none";

/** Viewport display defaults, pushed into the Rust host. Wireframe weight
 * and background seed every pane's settings (a scene file's saved per-pane
 * values still win on load; the pane's Display menu stays the live
 * per-pane override). The turntable speed is the live global rpm.
 *
 * The four label fields seed the attribute gear the same way: they are what
 * a fresh session starts from, and the gear is the live override for the
 * session in front of you. */
export interface DisplayPrefs {
  wireframeWeight: WireframeWeight;
  background: BackgroundChoice;
  /** Turntable revolutions per minute, clamped 1..60. */
  turntableRpm: number;
  /** Attribute-label text size. */
  labelSize: LabelSizeChoice;
  /** What an attribute label draws behind its text. */
  labelBackground: LabelBackgroundChoice;
  /** Attribute-label opacity, 0..1. */
  labelOpacity: number;
  /** Decimal places in an attribute label's value text, 0..4. */
  labelDecimals: number;
  /** On-screen size of a rendered point, in pixels, clamped 1..32.
   *
   * Global rather than per pane (decision M-27): there is no comparison
   * worth two point sizes side by side. */
  pointSize: number;
}

/** The shipped defaults.
 *
 * `pointSize` matches the renderer's own constant
 * (`solarxy_core::view_config::DEFAULT_POINT_SIZE`), so turning the
 * preference on changes nothing until somebody moves it.
 */
export const DEFAULT_DISPLAY_PREFS: DisplayPrefs = {
  wireframeWeight: "Light",
  background: "Gradient",
  turntableRpm: 6,
  pointSize: 6,
  // The label defaults match the renderer's own (`LabelStyle::new_default`),
  // so turning the preference on changes nothing until somebody moves it.
  labelSize: "medium",
  labelBackground: "chip",
  labelOpacity: 1,
  labelDecimals: 2,
};
