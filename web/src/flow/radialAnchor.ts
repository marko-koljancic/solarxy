// The radial ring's screen anchor.
//
// Before the ring captured the node's rect ONCE, when it opened, and
// never moved again: pan or zoom and the ring drifted off its node. The fix is
// to recompute the anchor on every viewport change, which RadialMenu does by
// subscribing to the xyflow transform and re-measuring the node.
//
// The node's SCREEN rect is the input, because that is the one thing that is
// unambiguously true: it already has pan and zoom baked in, and it does not
// depend on xyflow's internal bookkeeping (`measured` / `positionAbsolute`,
// which can legitimately be empty before a node is measured).
//
// Pure: no DOM, no React, no xyflow import, so it is unit-testable.

/** A node's rect in viewport (client) CSS px, i.e. `getBoundingClientRect()`. */
export interface ScreenRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The ring hugs the node, but stops growing past this screen radius so a
 * zoomed-in node cannot push the ring off the screen. Carried from the original
 * implementation. */
export const RADIUS_CAP_PX = 70;

export interface RadialAnchor {
  /** Ring centre, in viewport (client) CSS px. */
  cx: number;
  cy: number;
  /** Inner radius, screen px. */
  radius: number;
}

/**
 * Where the ring sits, given the node's current on-screen box.
 *
 * The inner radius scales with the node (so the ring hugs it at any zoom) up to
 * the cap, while the band width and grace distance stay screen-space constants
 * in RadialMenu. That combination is what makes the ring read identically at
 * every zoom level instead of ballooning.
 */
export function radialAnchor(nodeRect: ScreenRect): RadialAnchor {
  return {
    cx: nodeRect.left + nodeRect.width / 2,
    cy: nodeRect.top + nodeRect.height / 2,
    radius: Math.min(Math.max(nodeRect.width, nodeRect.height) / 2, RADIUS_CAP_PX),
  };
}
