// The live dockview api, held module-side so the keymap, the menus, and the
// Desks store can act on the dock without prop-drilling it through the tree.
// Everything that mutates the dock goes through here, so the failure modes
// (a stale serialized layout wedging the instance) are handled in exactly one
// place.

import type { DockviewApi, SerializedDockview } from "dockview-react";
import { applyRecipe, DEFAULT_RECIPE, type DeskLayout, type LayoutRecipe } from "./layouts";
import { pushToast } from "../store/toasts";

let api: DockviewApi | null = null;

export function setDockApi(next: DockviewApi | null): void {
  api = next;
}

export function getDockApi(): DockviewApi | null {
  return api;
}

/** Serializes the live arrangement. Maximize is NOT captured: `SerializedDockview`
 * has no maximized field (verified against dockview's types and asserted in the
 * spike), so a maximized dock serializes as its underlying grid, which is
 * exactly the "maximize stays transient" contract. */
export function captureLayout(): SerializedDockview | null {
  return api ? api.toJSON() : null;
}

/** Applies a saved arrangement, falling back to the default recipe if the blob
 * is stale or corrupt.
 *
 * `fromJSON` throws on a malformed or stale layout and leaves the dock with ZERO
 * panels (dockview #341; reproduced in the spike and again against the real
 * shell, where it took the WebGPU canvas out of the DOM with it). So the catch
 * is not defensive programming, it is the documented recovery path.
 *
 * The rebuild is DEFERRED to a fresh task on purpose: rebuilding synchronously
 * inside the catch does not survive, because dockview is still unwinding its own
 * failed `fromJSON` and wipes the panels we just added. Measured: a synchronous
 * `applyRecipe` here left the dock empty and the canvas detached, while the same
 * calls from a clean stack rebuilt it perfectly. */
export function applyLayout(layout: DeskLayout): void {
  if (!api) return;
  if (layout.kind === "recipe") {
    applyRecipe(api, layout.recipe);
    return;
  }
  try {
    api.fromJSON(layout.json);
  } catch (err) {
    console.error("dock layout failed to restore", err);
    setTimeout(() => {
      const dock = api;
      if (!dock) return;
      applyRecipe(dock, DEFAULT_RECIPE);
      pushToast("Saved panel layout could not be restored; reset to Default", "warn");
    }, 0);
  }
}

/** The boot path: restore the persisted layout, else build the default. */
export function restoreLayout(saved: SerializedDockview | null, fallback: LayoutRecipe): void {
  if (!api) return;
  if (saved) {
    applyLayout({ kind: "serialized", json: saved });
    return;
  }
  applyRecipe(api, fallback);
}

// ---- maximize (transient; never captured by a desk) ----

export function hasMaximizedPanel(): boolean {
  return api?.hasMaximizedGroup() ?? false;
}

export function exitMaximized(): void {
  api?.exitMaximizedGroup();
}

/** Maximizes the group owning `id`, or restores if it is already maximized. */
export function toggleMaximize(id: string): void {
  if (!api) return;
  const panel = api.getPanel(id);
  if (!panel) return;
  if (panel.api.isMaximized()) {
    api.exitMaximizedGroup();
  } else {
    api.maximizeGroup(panel);
  }
}

// ---- the asset panels (item 2; added and removed on demand) ----

export function isAssetsPanelOpen(): boolean {
  return api?.getPanel("assets") !== undefined;
}

/** Adds the Assets panel (tabbed beside Properties, the Review pattern) or
 * removes it. */
export function setAssetsPanelOpen(open: boolean): void {
  if (!api) return;
  const existing = api.getPanel("assets");
  if (open) {
    if (existing) {
      existing.api.setActive();
      return;
    }
    const properties = api.getPanel("properties");
    api.addPanel({
      id: "assets",
      component: "assets",
      title: "Assets",
      position: properties ? { referenceGroup: properties.api.group } : undefined,
    });
  } else if (existing) {
    api.removePanel(existing);
  }
}

/** Opens (or focuses) the asset-preview panel, tabbed beside the Assets
 * panel so the grid and the preview sit together. */
export function openAssetPreviewPanel(title: string): void {
  if (!api) return;
  const existing = api.getPanel("assetPreview");
  if (existing) {
    existing.setTitle(title);
    existing.api.setActive();
    return;
  }
  const anchor = api.getPanel("assets") ?? api.getPanel("properties");
  api.addPanel({
    id: "assetPreview",
    component: "assetPreview",
    title,
    position: anchor ? { referenceGroup: anchor.api.group } : undefined,
  });
}

// ---- the texture viewer panel (phase 19; added and removed on demand) ----

export function isTexturePanelOpen(): boolean {
  return api?.getPanel("texture") !== undefined;
}

/** Adds the Texture viewer panel (tabbed beside Properties, the Assets
 * pattern) or removes it. */
export function setTexturePanelOpen(open: boolean): void {
  if (!api) return;
  const existing = api.getPanel("texture");
  if (open) {
    if (existing) {
      existing.api.setActive();
      return;
    }
    const properties = api.getPanel("properties");
    api.addPanel({
      id: "texture",
      component: "texture",
      title: "Texture",
      position: properties ? { referenceGroup: properties.api.group } : undefined,
    });
  } else if (existing) {
    api.removePanel(existing);
  }
}

// ---- the review panel (added and removed on demand, N) ----

export function isReviewPanelOpen(): boolean {
  return api?.getPanel("review") !== undefined;
}

/** Adds the Review panel (tabbed beside Properties) or removes it. The panel's
 * presence in the dock IS the open state; the review store mirrors it. */
export function setReviewPanelOpen(open: boolean): void {
  if (!api) return;
  const existing = api.getPanel("review");
  if (open) {
    if (existing) {
      existing.api.setActive();
      return;
    }
    const properties = api.getPanel("properties");
    api.addPanel({
      id: "review",
      component: "review",
      title: "Review",
      position: properties ? { referenceGroup: properties.api.group } : undefined,
    });
  } else if (existing) {
    api.removePanel(existing);
  }
}
