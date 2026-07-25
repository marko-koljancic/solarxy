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

/** Maximizes the group owning `id`, or restores if it is already maximized.
 * Grid groups only: dockview maximize has no meaning for floating or popout
 * groups, and calling it on one is not supported. */
export function toggleMaximize(id: string): void {
  if (!api) return;
  const panel = api.getPanel(id);
  if (!panel) return;
  if (panel.api.isMaximized()) {
    api.exitMaximizedGroup();
  } else if (panel.group.api.location.type === "grid") {
    api.maximizeGroup(panel);
  }
}

// ---- on-demand panels (presence in the dock IS the open state) ----

type PanelPosition = Parameters<DockviewApi["addPanel"]>[0]["position"];

/** The shared open/close body: focus if present, add at `position()` if
 * absent, remove on close. Panel id doubles as the component name (the
 * PANEL_IDS convention). */
function setPanelOpen(id: string, title: string, open: boolean, position: () => PanelPosition): void {
  if (!api) return;
  const existing = api.getPanel(id);
  if (open) {
    if (existing) {
      existing.api.setActive();
      return;
    }
    api.addPanel({ id, component: id, title, position: position() });
  } else if (existing) {
    api.removePanel(existing);
  }
}

/** Tabbed beside Properties when it exists (the aux-panel pattern), else
 * wherever dockview appends. */
function besideProperties(): PanelPosition {
  const properties = api?.getPanel("properties");
  return properties ? { referenceGroup: properties.api.group } : undefined;
}

export function isAssetsPanelOpen(): boolean {
  return api?.getPanel("assets") !== undefined;
}

/** Adds the Assets panel (tabbed beside Properties, the Review pattern) or
 * removes it. */
export function setAssetsPanelOpen(open: boolean): void {
  setPanelOpen("assets", "Assets", open, besideProperties);
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

export function isTexturePanelOpen(): boolean {
  return api?.getPanel("texture") !== undefined;
}

/** Adds the Texture viewer panel (tabbed beside Properties, the Assets
 * pattern) or removes it. */
export function setTexturePanelOpen(open: boolean): void {
  setPanelOpen("texture", "Texture", open, besideProperties);
}

export function isAttributesPanelOpen(): boolean {
  return api?.getPanel("attributes") !== undefined;
}

/** Adds the Attributes spreadsheet (tabbed beside Properties, the Assets
 * pattern) or removes it. */
export function setAttributesPanelOpen(open: boolean): void {
  setPanelOpen("attributes", "Attributes", open, besideProperties);
}

export function isTreePanelOpen(): boolean {
  return api?.getPanel("tree") !== undefined;
}

/** Adds the scene Tree panel (tabbed beside Properties, the aux-panel
 * pattern) or removes it. */
export function setTreePanelOpen(open: boolean): void {
  setPanelOpen("tree", "Tree", open, besideProperties);
}

export function isReviewPanelOpen(): boolean {
  return api?.getPanel("review") !== undefined;
}

/** Adds the Review panel (tabbed beside Properties) or removes it. The panel's
 * presence in the dock IS the open state; the review store mirrors it. */
export function setReviewPanelOpen(open: boolean): void {
  setPanelOpen("review", "Review", open, besideProperties);
}

// ---- the core panels, reopenable after their tab is closed (feedback:
// closing Properties or Nodes used to be recoverable only by applying a
// desk) ----

export function isNodesPanelOpen(): boolean {
  return api?.getPanel("nodes") !== undefined;
}

/** Reopens the Nodes panel beside the viewport (the recipe's arrangement),
 * falling back to wherever dockview appends when the viewport is gone too. */
export function setNodesPanelOpen(open: boolean): void {
  setPanelOpen("nodes", "Nodes", open, () => {
    const viewport = api?.getPanel("viewport");
    return viewport ? { direction: "right", referencePanel: viewport } : undefined;
  });
}

export function isPropertiesPanelOpen(): boolean {
  return api?.getPanel("properties") !== undefined;
}

/** Reopens the Properties panel below Nodes (mirroring the default recipe),
 * else beside the viewport, else wherever dockview appends. */
export function setPropertiesPanelOpen(open: boolean): void {
  setPanelOpen("properties", "Properties", open, () => {
    const nodes = api?.getPanel("nodes");
    if (nodes) return { direction: "below", referencePanel: nodes };
    const viewport = api?.getPanel("viewport");
    return viewport ? { direction: "right", referencePanel: viewport } : undefined;
  });
}
