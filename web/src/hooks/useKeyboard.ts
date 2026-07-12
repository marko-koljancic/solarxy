// Global editor keyboard shortcuts, driven by the typed keymap table
// (web/src/input/keymap.ts, UX spec section 16). Contexts follow the
// cursor-hover focus model: viewport bindings (1-7 inspection, F1-F5
// layouts, F fit) fire only while the pointer is over the 3D region.
// Ignored while typing in a field. The palette handles its own Tab
// binding (it owns open state); everything else dispatches here.

import { useEffect } from "react";
import {
  cameraCommand,
  copySelection,
  dispatch,
  duplicateSelection,
  explicitSave,
  paste,
  setActivePane,
  setPaneSettings,
  setViewLayout,
} from "../engine/session";
import { cycleAutoLayout } from "../flow/layout";
import { eventKeys, lookupBinding } from "../input/keymap";
import { useMirror } from "../store/mirror";
import { useReview } from "../store/review";
import { EDGE_STYLE_LABELS, useUi } from "../store/ui";
import { useViewState } from "../store/viewState";
import { pushToast } from "../store/toasts";
import type { PaneDisplaySettings, ViewLayout } from "../engine/types";

function typing(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el?.tagName?.match(/INPUT|TEXTAREA|SELECT/) || el?.isContentEditable === true;
}

/** Patches the active pane's settings (3D mode + the given fields). */
function patchActivePane(patch: Partial<PaneDisplaySettings>, toast: string): void {
  const view = useViewState.getState().view;
  if (!view) return;
  const pane = view.activePane;
  setPaneSettings(pane, { ...view.paneSettings[pane], paneMode: "Scene3D", ...patch });
  pushToast(toast, "info");
}

function applyLayout(layout: ViewLayout): void {
  setViewLayout(layout);
}

export function useKeyboard(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typing(e.target)) return;
      const vs = useViewState.getState();
      const context = vs.pointerOverViewport
        ? "viewport"
        : vs.pointerOverCanvas
          ? "canvas"
          : "global";
      const binding = lookupBinding(eventKeys(e), context);
      if (!binding) return;

      const s = useMirror.getState();
      const ctx = s.current;
      const ctxKey = ctx === "root" ? "root" : `sub:${ctx.subflow}`;
      const selection = s.contexts[ctxKey]?.selection ?? [];

      switch (binding.id) {
        case "save":
          // Spec section 16: Cmd/Ctrl+S is intercepted and saves the scene.
          e.preventDefault();
          void explicitSave();
          break;
        case "shortcuts": {
          const ui = useUi.getState();
          ui.setShortcutsOpen(!ui.shortcutsOpen);
          break;
        }
        case "preferences": {
          e.preventDefault();
          const ui = useUi.getState();
          ui.setPrefsOpen(!ui.prefsOpen);
          break;
        }
        case "undo":
          e.preventDefault();
          dispatch({ type: "undo" });
          break;
        case "redo":
        case "redo-alt":
          e.preventDefault();
          dispatch({ type: "redo" });
          break;
        case "copy":
          copySelection();
          break;
        case "paste":
          paste();
          break;
        case "duplicate":
          e.preventDefault();
          duplicateSelection();
          break;
        case "cook":
          e.preventDefault();
          dispatch({ type: "cookNow" });
          break;
        case "display-flag": {
          // Subflow-only: move the display flag to the first selected node.
          if (ctx === "root" || !selection.length) break;
          dispatch({ type: "setActiveOutput", ctx, node: selection[0] });
          break;
        }
        case "rename": {
          // Inline rename on the first selected node; the mounted node view
          // (graph or list) consumes the request and opens its editor.
          if (!selection.length) break;
          e.preventDefault();
          useUi.getState().setRenameRequest(selection[0]);
          break;
        }
        case "bypass": {
          if (!selection.length) break;
          const first = s.contexts[ctxKey]?.nodes.find((n) => n.id === selection[0]);
          const bypassed = !(first?.bypassed ?? false);
          for (const id of selection) dispatch({ type: "setBypass", ctx, node: id, bypassed });
          break;
        }
        case "inspect-shaded":
          patchActivePane({ inspectionMode: "Shaded" }, "Inspection: Shaded");
          break;
        case "inspect-material":
          patchActivePane({ inspectionMode: "MaterialId" }, "Inspection: Material ID");
          break;
        case "inspect-texel":
          patchActivePane({ inspectionMode: "TexelDensity" }, "Inspection: Texel Density");
          break;
        case "inspect-depth":
          patchActivePane({ inspectionMode: "Depth" }, "Inspection: Depth");
          break;
        case "inspect-overdraw":
          patchActivePane({ inspectionMode: "Overdraw" }, "Inspection: Overdraw");
          break;
        case "inspect-ao":
          patchActivePane({ inspectionMode: "AoPreview" }, "Inspection: AO Preview");
          break;
        case "uv-pane-toggle": {
          // The UV pane renders in W4; the toggle round-trips already.
          const view = useViewState.getState().view;
          if (!view) break;
          const pane = view.activePane;
          const current = view.paneSettings[pane];
          const next = current.paneMode === "UvMap" ? "Scene3D" : "UvMap";
          setPaneSettings(pane, {
            ...current,
            paneMode: next,
            uvOffset: [0, 0],
            uvZoom: 1,
          });
          pushToast(next === "UvMap" ? "UV Map" : "3D View", "info");
          break;
        }
        case "layout-single":
          e.preventDefault();
          applyLayout("single");
          break;
        case "layout-split-v":
          e.preventDefault();
          applyLayout("splitVertical");
          break;
        case "layout-split-h":
          e.preventDefault();
          applyLayout("splitHorizontal");
          break;
        case "layout-quad":
          e.preventDefault();
          applyLayout("quad");
          break;
        case "layout-three":
          e.preventDefault();
          applyLayout("threeLeftBig");
          break;
        case "uv-overlap-toggle": {
          const view = useViewState.getState().view;
          if (!view) break;
          const pane = view.activePane;
          const current = view.paneSettings[pane];
          if (current.paneMode !== "UvMap") break;
          setPaneSettings(pane, { ...current, showUvOverlap: !current.showUvOverlap });
          pushToast(current.showUvOverlap ? "Overlap: Off" : "Overlap: On", "info");
          break;
        }
        case "fit": {
          const view = useViewState.getState().view;
          if (view) {
            setActivePane(view.activePane);
            cameraCommand(view.activePane, { kind: "fit" });
          }
          break;
        }
        case "screenshot": {
          const ui = useUi.getState();
          ui.setScreenshotOpen(!ui.screenshotOpen);
          break;
        }
        case "flow-grid":
          useUi.getState().toggleFlowChrome("showFlowGrid");
          break;
        case "flow-minimap":
          useUi.getState().toggleFlowChrome("showMinimap");
          break;
        case "flow-controls":
          useUi.getState().toggleFlowChrome("showFlowControls");
          break;
        case "edge-style-cycle": {
          const ui = useUi.getState();
          ui.cycleEdgeStyle();
          pushToast(`Connection style: ${EDGE_STYLE_LABELS[useUi.getState().edgeStyle]}`, "info");
          break;
        }
        case "layout-cycle": {
          e.preventDefault();
          const g = s.contexts[ctxKey];
          if (!g || g.nodes.length === 0) break;
          void cycleAutoLayout(ctx, g).then((algo) => {
            window.dispatchEvent(new Event("solarxy:fitView"));
            pushToast(`Auto-layout: ${algo === "dagre" ? "Dagre" : "ELK"}`, "info");
          });
          break;
        }
        case "review-mode": {
          const r = useReview.getState();
          r.setReviewMode(!r.reviewMode);
          pushToast(
            r.reviewMode ? "Review mode off" : "Review mode: click geometry to pin a note",
            "info",
          );
          break;
        }
        case "review-panel": {
          const r = useReview.getState();
          r.setPanelOpen(!r.panelOpen);
          break;
        }
        case "review-cancel": {
          // The cancel ladder: an open editor cancels first (its own key
          // handler covers the focused case; this covers unfocused), then
          // a pending re-anchor, then review mode. Otherwise leave Esc to
          // other consumers (palette, modals).
          const r = useReview.getState();
          if (r.draft) r.setDraft(null);
          else if (r.reanchorTarget !== null) {
            r.setReanchorTarget(null);
            pushToast("Re-anchor cancelled", "info");
          } else if (r.reviewMode) {
            r.setReviewMode(false);
            pushToast("Review mode off", "info");
          }
          break;
        }
        default:
          break; // "palette" is handled by the palette component itself.
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
