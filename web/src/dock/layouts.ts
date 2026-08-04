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

export const PANEL_IDS = ["viewport", "nodes", "properties", "review", "assets", "assetPreview", "texture", "attributes", "tree", "text"] as const;
export type PanelId = (typeof PANEL_IDS)[number];

/** The one panel every arrangement must contain. It is pinned against dragging
 * and its tab has no close button, so the only way an arrangement loses it is a
 * blob that was stale, hand-edited, or written by a different build. A
 * restored layout without it used to leave a permanently dead app, because the
 * engine booted from this panel's effect and nothing else. Named here so the
 * pin and the restore guard state the same invariant. */
export const VIEWPORT_PANEL_ID = "viewport";

export type ViewportSide = "left" | "right";
export type PropertiesDock = "bottom" | "right";

/** The pre-docking arrangement knobs. Kept as a type because legacy desks and
 * legacy `solarxy.ui.*` keys are expressed in exactly these terms. The
 * optional fields arrived with the Technical / LookDev / UV desks (feedback
 * wave 4); absent means false, so every stored desk stays valid. */
export interface LayoutRecipe {
  viewportSide: ViewportSide;
  propertiesDock: PropertiesDock;
  /** The viewport's share of the width, in percent. */
  splitPct: number;
  /** Whether the Review panel is part of the arrangement. */
  review: boolean;
  /** Attributes spreadsheet docked below the nodes canvas. */
  attributes?: boolean;
  /** The attributes strip's height share of the dock, in percent. */
  attributesPct?: number;
  /** Texture viewer tabbed into the properties group. */
  texture?: boolean;
  /** Scene tree tabbed into the nodes group. */
  tree?: boolean;
}

export type DeskLayout =
  | { kind: "recipe"; recipe: LayoutRecipe }
  | { kind: "serialized"; json: SerializedDockview };

export const DEFAULT_RECIPE: LayoutRecipe = {
  viewportSide: "left",
  propertiesDock: "bottom",
  splitPct: 55,
  review: false,
  attributes: false,
  attributesPct: 30,
  texture: false,
  tree: false,
};

const SPLIT_MIN_PCT = 20;
const SPLIT_MAX_PCT = 80;

export function clampSplit(pct: number): number {
  if (!Number.isFinite(pct)) return DEFAULT_RECIPE.splitPct;
  return Math.min(SPLIT_MAX_PCT, Math.max(SPLIT_MIN_PCT, pct));
}

const ATTRIBUTES_PCT_DEFAULT = 30;
const ATTRIBUTES_PCT_MIN = 15;
const ATTRIBUTES_PCT_MAX = 50;

export function clampAttributesPct(pct: number): number {
  if (!Number.isFinite(pct)) return ATTRIBUTES_PCT_DEFAULT;
  return Math.min(ATTRIBUTES_PCT_MAX, Math.max(ATTRIBUTES_PCT_MIN, pct));
}

/** Coerces anything (a hand-edited desk, a legacy blob, `undefined`) into a
 * valid recipe. Pure, so it is unit-tested without a DOM. */
export function sanitizeRecipe(r: Partial<LayoutRecipe> | undefined): LayoutRecipe {
  return {
    viewportSide: r?.viewportSide === "right" ? "right" : "left",
    propertiesDock: r?.propertiesDock === "right" ? "right" : "bottom",
    splitPct: clampSplit(r?.splitPct ?? DEFAULT_RECIPE.splitPct),
    review: r?.review === true,
    attributes: r?.attributes === true,
    attributesPct: clampAttributesPct(r?.attributesPct ?? ATTRIBUTES_PCT_DEFAULT),
    texture: r?.texture === true,
    tree: r?.tree === true,
  };
}

/** The legacy arrangement fields, as stored before the shell moved to
 * dockview, appearing in an old desk and in the retired `solarxy.ui.*`
 * keys. */
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

  if (r.texture) {
    api.addPanel({
      id: "texture",
      component: "texture",
      title: "Texture",
      position: { referenceGroup: properties.api.group },
    });
    // The tab that names the group stays in front.
    properties.api.setActive();
  }

  if (r.tree) {
    api.addPanel({
      id: "tree",
      component: "tree",
      title: "Tree",
      position: { referenceGroup: nodes.api.group },
    });
    nodes.api.setActive();
  }

  const attributes = r.attributes
    ? api.addPanel({
        id: "attributes",
        component: "attributes",
        title: "Attributes",
        position: { direction: "below", referencePanel: nodes },
      })
    : null;

  // Sizing runs after every addPanel: dockview redistributes on each add,
  // so an earlier setSize would be clobbered by a later panel.
  const width = api.width;
  if (width > 0) {
    viewport.api.setSize({ width: Math.round((width * r.splitPct) / 100) });
  }
  const height = api.height;
  if (attributes && height > 0) {
    attributes.api.setSize({
      height: Math.round((height * (r.attributesPct ?? ATTRIBUTES_PCT_DEFAULT)) / 100),
    });
  }
}
