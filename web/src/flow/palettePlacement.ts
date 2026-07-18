// Where the Tab palette opens.
//
// It used to be pinned by CSS: `.palette { position: absolute; top: 7rem;
// right: 4rem }` against a `position: fixed; inset: 0` backdrop. The backdrop
// being fixed made IT the containing block, so those offsets resolved against
// the whole viewport and the palette's DOM parent (the node pane) was
// irrelevant — it opened in the top-right corner of the window no matter
// where the pane was, or where you were looking.
//
// Blender and Houdini both spawn their add-node menu at the pointer, and drop
// the node there. That is what this computes.
//
// Pure: no DOM, no React, no xyflow, so it is unit-testable.

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Point {
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

/** Gap kept between the panel and the pane's edges when clamping. */
export const MARGIN_PX = 8;

/** How far the panel's top-left sits from the pointer, so the cursor lands
 * just outside the panel rather than on top of its first row. */
const POINTER_OFFSET_PX = 6;

function contains(pane: Rect, p: Point): boolean {
  return (
    p.x >= pane.left &&
    p.x <= pane.left + pane.width &&
    p.y >= pane.top &&
    p.y <= pane.top + pane.height
  );
}

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));

/**
 * The palette's top-left corner, in viewport (client) CSS px.
 *
 * At the pointer when the pointer is over the pane; otherwise centred
 * horizontally and set down from the pane's top edge, which is where a
 * command palette is expected when it was opened from a menu rather than a
 * gesture. Always clamped so the whole panel stays inside the pane — opening
 * a menu half off-screen is worse than opening it slightly away from the
 * cursor.
 */
export function palettePlacement(
  pointer: Point | null,
  pane: Rect,
  panel: Size,
  margin: number = MARGIN_PX,
): Point {
  const minX = pane.left + margin;
  const minY = pane.top + margin;
  const maxX = pane.left + pane.width - panel.width - margin;
  const maxY = pane.top + pane.height - panel.height - margin;

  if (!pointer || !contains(pane, pointer)) {
    return {
      x: clamp(pane.left + (pane.width - panel.width) / 2, minX, maxX),
      // A third down reads better than dead centre: the eye is already high
      // in the pane, and it leaves room for the list to grow downward.
      y: clamp(pane.top + pane.height / 6, minY, maxY),
    };
  }

  return {
    x: clamp(pointer.x + POINTER_OFFSET_PX, minX, maxX),
    y: clamp(pointer.y + POINTER_OFFSET_PX, minY, maxY),
  };
}
