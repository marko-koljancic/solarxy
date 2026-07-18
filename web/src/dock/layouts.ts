// Dock layouts.
//
// A desk's arrangement is one of two things:
//
//   - a RECIPE: the handful of knobs the pre-docking shell had (which side the
//     viewport sits on, where properties dock, the split percentage). Presets
//     and migrated legacy desks use this, because a recipe survives a dockview
//     version bump and a hand-edit, while a serialized blob does not.
//   - a SERIALIZED dockview layout: what the user actually arranged. Only
//     user-saved desks and the live layout use this.
//
// Realizing a recipe is a few `addPanel` calls; realizing a serialized layout is
// `fromJSON`, which THROWS on a stale blob and leaves the dock with zero panels
// (dockview issue #341, reproduced in the spike). Every restore path must
// therefore be able to fall back to a recipe.

import type { DockviewApi, SerializedDockview } from "dockview-react";

export const PANEL_IDS = ["viewport", "nodes", "properties", "review", "assets", "assetPreview", "texture"] as const;
export type PanelId = (typeof PANEL_IDS)[number];

export type ViewportSide = "left" | "right";
export type PropertiesDock = "bottom" | "right";

/** The pre-docking arrangement knobs. Kept as a type because legacy desks and
 * legacy `solarxy.ui.*` keys are expressed in exactly these terms. */
export interface LayoutRecipe {
  viewportSide: ViewportSide;
  propertiesDock: PropertiesDock;
  /** The viewport's share of the width, in percent. */
  splitPct: number;
  /** Whether the Review panel is part of the arrangement. */
  review: boolean;
}

export type DeskLayout =
  | { kind: "recipe"; recipe: LayoutRecipe }
  | { kind: "serialized"; json: SerializedDockview };

export const DEFAULT_RECIPE: LayoutRecipe = {
  viewportSide: "left",
  propertiesDock: "bottom",
  splitPct: 55,
  review: false,
};

const SPLIT_MIN_PCT = 20;
const SPLIT_MAX_PCT = 80;

export function clampSplit(pct: number): number {
  if (!Number.isFinite(pct)) return DEFAULT_RECIPE.splitPct;
  return Math.min(SPLIT_MAX_PCT, Math.max(SPLIT_MIN_PCT, pct));
}

/** Coerces anything (a hand-edited desk, a legacy blob, `undefined`) into a
 * valid recipe. Pure, so it is unit-tested without a DOM. */
export function sanitizeRecipe(r: Partial<LayoutRecipe> | undefined): LayoutRecipe {
  return {
    viewportSide: r?.viewportSide === "right" ? "right" : "left",
    propertiesDock: r?.propertiesDock === "right" ? "right" : "bottom",
    splitPct: clampSplit(r?.splitPct ?? DEFAULT_RECIPE.splitPct),
    review: r?.review === true,
  };
}

/** The legacy (pre-Phase-10) arrangement fields, as they appear in a stored
 * desk from before the migration and in the retired `solarxy.ui.*` keys. */
export interface LegacyArrangement {
  viewportSide?: ViewportSide;
  propertiesDock?: PropertiesDock;
  splitPct?: number;
}

/** Maps a legacy arrangement onto the equivalent recipe. This is the whole of
 * the forward migration: the old shell could express nothing a recipe cannot. */
export function synthesizeRecipe(legacy: LegacyArrangement): LayoutRecipe {
  return sanitizeRecipe({
    viewportSide: legacy.viewportSide,
    propertiesDock: legacy.propertiesDock,
    splitPct: legacy.splitPct,
    review: false,
  });
}

/** Builds a live dockview layout from a recipe. The viewport is always added
 * first so it owns the root group. */
export function applyRecipe(api: DockviewApi, recipe: LayoutRecipe): void {
  const r = sanitizeRecipe(recipe);
  api.clear();

  const viewport = api.addPanel({
    id: "viewport",
    component: "viewport",
    tabComponent: "pinned", // no close button; the drag is cancelled in Dock.tsx
    title: "Viewport",
  });

  const nodes = api.addPanel({
    id: "nodes",
    component: "nodes",
    title: "Nodes",
    position: {
      direction: r.viewportSide === "left" ? "right" : "left",
      referencePanel: viewport,
    },
  });

  const properties = api.addPanel({
    id: "properties",
    component: "properties",
    title: "Properties",
    position:
      r.propertiesDock === "bottom"
        ? { direction: "below", referencePanel: nodes }
        : // A full-height column at the far edge, which is what the old
          // right-docked properties column was.
          { direction: r.viewportSide === "left" ? "right" : "left" },
  });

  if (r.review) {
    api.addPanel({
      id: "review",
      component: "review",
      title: "Review",
      position: { referenceGroup: properties.api.group },
    });
  }

  // The split percentage is the viewport's share of the width.
  const width = api.width;
  if (width > 0) {
    viewport.api.setSize({ width: Math.round((width * r.splitPct) / 100) });
  }
}
