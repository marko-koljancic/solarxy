// The dock: dockview owns the shell's geometry. Panels dock, float,
// tab and maximize; the arrangement persists and Desks capture it.
//
// Two invariants are enforced here and nowhere else:
//
//  1. The viewport panel is PINNED. `locked` only guards drop targets, so the
//     pin cancels the tab drag itself (verified in the spike). This mirrors the
//     desktop, where the Viewport egui_dock tab is `allowed_in_windows = false`.
//  2. The Review panel's presence in the dock IS its open state, so closing its
//     tab and pressing N are the same action; the review store mirrors it.

import { DockviewReact, type DockviewReadyEvent } from "dockview-react";
import { useCallback, useEffect } from "react";
import { setDockApi, restoreLayout } from "./api";
import { DOCK_COMPONENTS, DOCK_TAB_COMPONENTS } from "./panels";
import { DEFAULT_RECIPE, synthesizeRecipe } from "./layouts";
import { loadLegacyArrangement, useUi } from "../store/ui";
import { useReview } from "../store/review";

/** Layout writes are chatty during a divider drag; persist on the trailing edge. */
const PERSIST_DEBOUNCE_MS = 400;

export function Dock() {
  const onReady = useCallback((event: DockviewReadyEvent) => {
    const api = event.api;
    setDockApi(api);
    if (import.meta.env.DEV) {
      // Dev-only introspection hook (Chrome-automation verification), matching
      // the engine's `__solarxy`.
      (window as unknown as Record<string, unknown>).__dock = api;
    }

    // First boot after the migration has no dock layout but may have the old
    // shell's keys; the recipe reproduces that arrangement rather than dropping
    // the user into the default.
    const legacy = loadLegacyArrangement();
    restoreLayout(
      useUi.getState().dockLayout,
      legacy ? synthesizeRecipe(legacy) : DEFAULT_RECIPE,
    );

    // The pin: cancel any drag that would tear the viewport out of the dock.
    api.onWillDragPanel((e) => {
      if (e.panel.id === "viewport") {
        e.nativeEvent.preventDefault();
        e.nativeEvent.stopPropagation();
      }
    });

    // Mirror the Review panel's presence into the review store, so the N key,
    // the Review menu checkbox and the tab's close button all agree.
    const syncReview = () => {
      useReview.getState().setPanelOpen(api.getPanel("review") !== undefined);
    };
    api.onDidAddPanel(syncReview);
    api.onDidRemovePanel(syncReview);

    let timer: number | undefined;
    api.onDidLayoutChange(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        // Never persist an EMPTY layout. A failed `fromJSON` leaves the dock
        // with zero panels, and that transient fires a layout change like any
        // other: saving it would poison the next boot with a layout that has no
        // viewport, turning a one-off bad blob into a permanent one.
        if (api.panels.length === 0) return;
        useUi.getState().setDockLayout(api.toJSON());
      }, PERSIST_DEBOUNCE_MS);
    });
  }, []);

  useEffect(() => () => setDockApi(null), []);

  return (
    <DockviewReact
      className="solarxy-dock"
      components={DOCK_COMPONENTS}
      tabComponents={DOCK_TAB_COMPONENTS}
      defaultTabComponent={DOCK_TAB_COMPONENTS.colored}
      onReady={onReady}
      singleTabMode="fullwidth"
    />
  );
}
