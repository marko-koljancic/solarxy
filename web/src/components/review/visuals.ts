// Shared review visual vocabulary: category glyphs, labels, and the CSS
// class carrying each category's color. The same classes color pins and
// panel chips so the correlation cue never drifts.

import type { ReviewCategory } from "../../engine/types";

export const CATEGORY_LABELS: Record<ReviewCategory, string> = {
  info: "Info",
  warning: "Warning",
  question: "Question",
  change: "Change",
};

/** The pin glyph. Must stay identical to the desktop set in
 * `gui/review_visuals.rs`: a marker and its panel chip are correlated by
 * BOTH color and glyph, so a shell-specific glyph breaks the cue for anyone
 * who uses both. "change" drew a `*` here against the desktop's pen until
 * 0.7.1, under a comment that claimed it was already a pen. */
export const CATEGORY_GLYPHS: Record<ReviewCategory, string> = {
  info: "i",
  warning: "!",
  question: "?",
  change: "✎",
};

/** Relative time for annotation rows ("3h ago"); falls back to the raw
 * string when unparsable (older files carry empty timestamps). */
export function relativeTime(iso: string): string {
  if (!iso) return "";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  const s = Math.max(0, (Date.now() - t) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** A one-line preview of the note text. */
export function shortPreview(text: string, max = 60): string {
  const line = text.split("\n")[0];
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}
