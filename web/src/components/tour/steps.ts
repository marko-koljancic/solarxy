// The tour catalog: the first-run overview plus the topic tours the Help
// submenu offers.
//
// Data, not JSX, so the steps are inspectable and testable: a step that
// points at a selector nothing renders is a bug a test can catch, rather
// than an empty spotlight a user finds.
//
// Every tour teaches the SHAPE of a workflow, not a task. Nothing here
// mutates the document: a tour that builds a scene for you leaves you
// with a scene you did not make and did not learn to make.

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

export interface TourDef {
  id: "overview" | "modeling" | "review";
  /** The Help submenu label. */
  title: string;
  /** Bumped when the steps change enough that a returning user should see
   * the tour again. Only the overview's version gates the first-run
   * auto-play (stored in prefs.onboarding). */
  version: number;
  steps: TourStep[];
}

export const OVERVIEW_TOUR: TourDef = {
  id: "overview",
  title: "Overview",
  version: 1,
  steps: [
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
  ],
};

export const MODELING_TOUR: TourDef = {
  id: "modeling",
  title: "Modeling Basics",
  version: 1,
  steps: [
    {
      id: "canvas",
      target: ".node-canvas-host",
      title: "Model with nodes",
      body: "Press Tab, add a Geo container, and double-click it to step inside: the graph in there IS the model. Generators make geometry, modifiers reshape it, and the display flag picks what renders.",
      side: "left",
    },
    {
      id: "properties",
      target: ".properties-panel-body",
      title: "Parameters drive everything",
      body: "Scrub any number and watch the viewport follow. Nothing is baked: you can come back to any node's values at any time.",
      side: "left",
    },
    {
      id: "tools",
      target: ".tool-column",
      title: "Gizmos write nodes",
      body: "Dragging an object with W, E or R writes into a transform node in its graph, so even viewport moves stay parametric and undoable.",
      side: "right",
    },
    {
      id: "pane-menus",
      target: ".pane-controls",
      title: "Inspect while you build",
      body: "The bracketed pane menus switch shading, wireframe, normals and bounds per pane. The Display menu tucks the detail under submenus.",
      side: "bottom",
    },
    {
      id: "attr-strip",
      target: ".attr-column",
      title: "See your attributes",
      body: "Pick a point lane and toggle value labels, vector arrows or point numbers. The gear opens scale and color options for the arrows.",
      side: "left",
    },
    {
      id: "save",
      target: ".menu-bar",
      title: "Save, and learn from samples",
      body: "Save Scene writes one self-contained .slxy file. File then Sample Scenes opens worked examples whose note nodes explain each workflow in place.",
      side: "bottom",
    },
  ],
};

export const REVIEW_TOUR: TourDef = {
  id: "review",
  title: "Review Workflow",
  version: 1,
  steps: [
    {
      id: "menu",
      target: ".menu-bar",
      title: "Review lives in the menu",
      body: "Toggle Review Mode from the Review menu, or press Shift+R. The amber dot up here shows when it is on.",
      side: "bottom",
    },
    {
      id: "pin",
      target: ".viewport-pane",
      title: "Pin notes on geometry",
      body: "In review mode, click a surface to drop an annotation right there. Pins anchor to the geometry and survive camera moves and recooks.",
      side: "right",
    },
    {
      id: "panel",
      target: ".review-panel",
      title: "Threads and resolution",
      body: "Every annotation lives here too: reply, filter by category, re-anchor a stale pin, and mark threads complete as they resolve.",
      side: "left",
    },
    {
      id: "validation",
      target: ".properties-panel-body",
      title: "Validation",
      body: "Wire a validate node after your geometry and the selected node grows a Validation tab; clicking an issue flies the camera to it.",
      side: "left",
    },
    {
      id: "share",
      target: ".node-canvas-host",
      title: "Share the file",
      body: "One .slxy carries the scene, its assets and the whole review conversation, so the person opening it sees exactly what you annotated.",
      side: "left",
    },
  ],
};

export const TOURS: TourDef[] = [OVERVIEW_TOUR, MODELING_TOUR, REVIEW_TOUR];

/** The tour for a replay request; an unknown or absent id falls back to
 * the overview (the pre-submenu event shape carried no id at all). */
export function tourById(id: unknown): TourDef {
  return TOURS.find((t) => t.id === id) ?? OVERVIEW_TOUR;
}
