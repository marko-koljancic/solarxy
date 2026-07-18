// The first-run tour's script.
//
// Data, not JSX, so the steps are inspectable and testable: a step that
// points at a selector nothing renders is a bug a test can catch, rather
// than an empty spotlight a user finds.
//
// It teaches the SHAPE of the app, not a task. Nothing here mutates the
// document: a tour that builds a scene for you leaves you with a scene you
// did not make and did not learn to make.

import type { Side } from "./placement";

export interface TourStep {
  id: string;
  /** The element to point at. The step is skipped when nothing matches, so
   * a tour never spotlights empty space in a layout that omits the panel. */
  target: string;
  title: string;
  /** One or two short sentences. Prose, not a feature list. */
  body: string;
  side?: Side;
}

export const TOUR_STEPS: TourStep[] = [
  {
    id: "viewport",
    target: ".viewport-pane",
    title: "The viewport",
    body: "Your scene, rendered on the GPU. Orbit with the left mouse button, pan with the middle, zoom with the wheel.",
    side: "right",
  },
  {
    id: "tools",
    target: ".tool-column",
    title: "Select, move, rotate, scale",
    body: "Q, W, E and R switch between them. Move, rotate and scale drag a gizmo on the selected object.",
    side: "right",
  },
  {
    id: "pane-menus",
    target: ".pane-controls",
    title: "Per-pane display",
    body: "Every pane carries its own shading, camera and overlays, so a split view can compare two of them side by side.",
    side: "bottom",
  },
  {
    id: "canvas",
    target: ".node-canvas-host",
    title: "The node graph",
    body: "Solarxy is parametric: this graph builds the scene, and nothing you make here is baked. Change a value upstream and everything downstream recooks.",
    side: "left",
  },
  {
    id: "palette",
    target: ".node-canvas-host",
    title: "Press Tab to add a node",
    body: "The palette opens at your cursor and the node lands there. Every node carries its own documentation: select one and press I to read it.",
    side: "left",
  },
  {
    id: "properties",
    target: ".properties-panel-body",
    title: "Parameters",
    body: "The selected node's parameters. Drag a number to scrub it, or hold Ctrl while dragging to snap.",
    side: "left",
  },
  {
    id: "review",
    target: ".review-panel",
    title: "Review",
    body: "Pin annotations directly onto geometry and they travel with the scene file. Useful when someone else has to look at what you made.",
    side: "left",
  },
];
