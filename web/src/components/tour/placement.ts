// Where a coachmark sits relative to the thing it points at.
//
// Pure: no DOM, no React, so the edge cases are unit-testable rather than
// discovered by a user finding a card half off-screen.

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

export type Side = "top" | "bottom" | "left" | "right";

export interface Placement {
  left: number;
  top: number;
  /** Which side of the anchor the card landed on; the caller points its
   * arrow the opposite way. */
  side: Side;
}

/** Gap between the anchor and the card, and between the card and the
 * viewport edge. */
export const GAP = 12;

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));

/** Room available on each side of `anchor` inside `viewport`. */
function room(anchor: Rect, viewport: Size): Record<Side, number> {
  return {
    top: anchor.top,
    bottom: viewport.height - (anchor.top + anchor.height),
    left: anchor.left,
    right: viewport.width - (anchor.left + anchor.width),
  };
}

/**
 * Place `card` beside `anchor`.
 *
 * Tries `preferred`, then falls back to whichever side has the most room,
 * then clamps into the viewport. The order matters: a step that points at
 * the left-edge tool column must not have its card pushed off-screen left
 * just because "left" was asked for.
 */
export function placeCoachmark(
  anchor: Rect,
  card: Size,
  viewport: Size,
  preferred: Side = "bottom",
): Placement {
  const space = room(anchor, viewport);
  const needed: Record<Side, number> = {
    top: card.height + GAP,
    bottom: card.height + GAP,
    left: card.width + GAP,
    right: card.width + GAP,
  };

  const side: Side =
    space[preferred] >= needed[preferred]
      ? preferred
      : (Object.keys(space) as Side[]).sort((a, b) => space[b] - space[a] - (needed[b] - needed[a]))[0];

  const maxLeft = viewport.width - card.width - GAP;
  const maxTop = viewport.height - card.height - GAP;

  switch (side) {
    case "top":
      return {
        side,
        left: clamp(anchor.left + anchor.width / 2 - card.width / 2, GAP, maxLeft),
        top: clamp(anchor.top - card.height - GAP, GAP, maxTop),
      };
    case "bottom":
      return {
        side,
        left: clamp(anchor.left + anchor.width / 2 - card.width / 2, GAP, maxLeft),
        top: clamp(anchor.top + anchor.height + GAP, GAP, maxTop),
      };
    case "left":
      return {
        side,
        left: clamp(anchor.left - card.width - GAP, GAP, maxLeft),
        top: clamp(anchor.top + anchor.height / 2 - card.height / 2, GAP, maxTop),
      };
    case "right":
      return {
        side,
        left: clamp(anchor.left + anchor.width + GAP, GAP, maxLeft),
        top: clamp(anchor.top + anchor.height / 2 - card.height / 2, GAP, maxTop),
      };
  }
}
