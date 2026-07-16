// The 3D viewport: a single WebGPU canvas driven by the Rust renderer.
// React never draws into it; it forwards pointer gestures to the host's
// pane-aware camera routing, runs the rAF cook+render loop, and floats
// one DOM toolbar per pane over the canvas (UX spec: panels are DOM, the
// canvas is one WebGPU surface with Rust-side pane hit-testing).
//
// Phase 10: the canvas element is no longer JSX. It is a module singleton
// (engine/canvas.ts) that this panel ADOPTS, because dockview's `fromJSON`
// (every desk apply) rebuilds panel content and would otherwise recreate the
// canvas and lose the WebGPU surface. Consequence: pointer handlers are
// attached imperatively rather than as React props.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  applyViewportBatch,
  bootSession,
  dispatch,
  getClient,
  reanchorAnnotation,
  runFrame,
} from "../engine/session";
import { adoptViewportCanvas, viewportCanvas } from "../engine/canvas";
import { useMirror } from "../store/mirror";
import { useReview } from "../store/review";
import { useUi } from "../store/ui";
import { useViewState } from "../store/viewState";
import { pushToast } from "../store/toasts";
import { PaneToolbars } from "./PaneToolbar";
import { GizmoReadout } from "./GizmoReadout";
import { ToolColumn } from "./ToolColumn";
import { ViewportContextMenu } from "./ViewportContextMenu";
import { ReviewOverlay } from "./review/ReviewOverlay";
import { ReviewPopup } from "./review/ReviewPopup";
import { ViewportMenuBar } from "./ViewportMenuBar";

/** Canvas-relative CSS px, the coordinate space the Rust host expects. */
function canvasPos(e: PointerEvent | MouseEvent): { x: number; y: number } {
  const rect = viewportCanvas().getBoundingClientRect();
  return { x: e.clientX - rect.left, y: e.clientY - rect.top };
}

/** Bit 0 = snap. A bitfield rather than a bool so shift-for-precision can land
 * later without changing the wasm signature.
 *
 * metaKey counts too: on macOS Ctrl-click is the secondary click, so Cmd is the
 * modifier a Mac user's hand actually reaches for. */
const MOD_SNAP = 1 << 0;
function pointerMods(e: PointerEvent): number {
  return e.ctrlKey || e.metaKey ? MOD_SNAP : 0;
}


export function Viewport() {
  const hostRef = useRef<HTMLDivElement>(null);
  // The right-click context menu: screen-space anchor, or null when closed.
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number } | null>(null);
  // Crosshair while review mode is active. A class on a canvas React does not
  // render, so it is applied imperatively.
  const reviewMode = useReview((s) => s.reviewMode);

  // Adopt the canvas into this panel. Runs on every render (not just mount) so
  // a dockview re-parent that swaps the host div is picked up immediately;
  // adoption is a no-op when the host is unchanged.
  useLayoutEffect(() => {
    if (hostRef.current) adoptViewportCanvas(hostRef.current);
  });

  useEffect(() => {
    viewportCanvas().classList.toggle("review-cursor", reviewMode);
  }, [reviewMode]);

  useEffect(() => {
    const canvas = viewportCanvas();
    let raf = 0;
    let mounted = true;

    bootSession(canvas)
      .catch((err: unknown) => {
        // WebGPU unavailable or wasm init failed: the boot overlay shows
        // the message (the full unsupported-browser page is Phase 16).
        useUi
          .getState()
          .setBootError(err instanceof Error ? err.message : String(err));
        throw err;
      })
      .then(() => {
        if (!mounted) return;

        // The scene starts EMPTY (maintainer decision, Phase 7b): the
        // canvas teaching hint carries the first-run experience.
        let last = performance.now();
        let frameFailures = 0;
        const loop = (t: number) => {
          try {
            // runFrame reconciles the canvas size with the surface before
            // cooking (engine/canvas.ts), so a dockview resize or re-parent can
            // never leave the Rust pane rects disagreeing with the DOM.
            runFrame(t - last);
            frameFailures = 0;
          } catch (err) {
            // A thrown frame must never silently kill the loop (a lost
            // device surfaces here). Log, toast once, and keep pumping so
            // a recovered surface resumes rendering.
            frameFailures += 1;
            if (frameFailures === 1) {
              console.error("frame failed", err);
              pushToast(
                `Rendering error: ${err instanceof Error ? err.message : err}`,
                "error",
              );
            }
          }
          last = t;
          raf = requestAnimationFrame(loop);
        };
        raf = requestAnimationFrame(loop);
      });

    return () => {
      mounted = false;
      cancelAnimationFrame(raf);
    };
  }, []);

  // Pointer routing: coordinates go to the host in canvas CSS px; the host
  // hit-tests the pane and drives that pane's camera controller. A press
  // with no drag is a pick; a double-click enters the picked geo's subflow.
  useEffect(() => {
    const canvas = viewportCanvas();
    let drag: { moved: boolean } | null = null;

    const onEnter = () => useViewState.getState().setPointerOverViewport(true);
    const onLeave = () => useViewState.getState().setPointerOverViewport(false);

    const onPointerDown = (e: PointerEvent) => {
      // Capture so a drag that leaves the canvas keeps delivering moves. Guarded:
      // it throws for a pointer id the browser does not consider active, and an
      // uncaught throw HERE would kill the whole press (no pick, no gizmo grab).
      try {
        canvas.setPointerCapture(e.pointerId);
      } catch {
        /* not a capturable pointer; the gesture still works, it just is not captured */
      }
      drag = { moved: false };
      try {
        const p = canvasPos(e);
        // Non-null when the press started a gizmo drag on the APPEND path, which
        // mints a transform node; the mirror must see it immediately.
        applyViewportBatch(getClient().pointerDown(p.x, p.y, e.button));
      } catch {
        /* not booted */
      }
    };

    const onPointerMove = (e: PointerEvent) => {
      if (drag && Math.abs(e.movementX) + Math.abs(e.movementY) > 1) drag.moved = true;
      try {
        const p = canvasPos(e);
        getClient().pointerMove(p.x, p.y, pointerMods(e));
      } catch {
        /* not booted */
      }
    };

    const onPointerUp = (e: PointerEvent) => {
      const d = drag;
      drag = null;
      try {
        // A gizmo drag commits here and returns its batch. When it does, the
        // press belonged to the gizmo, so the click ladder below must NOT also
        // run: a drag is not a pick.
        const gizmoBatch = getClient().pointerUp(e.button);
        if (gizmoBatch) {
          applyViewportBatch(gizmoBatch);
          return;
        }
        if (!d || d.moved || e.button !== 0) return;
        const review = useReview.getState();
        // Click ladder: a pending re-anchor consumes the click (hit or
        // miss); review mode pins a draft on a hit; else normal picking.
        if (review.reanchorTarget !== null) {
          const p = canvasPos(e);
          const pick = getClient().pickDetailed(p.x, p.y);
          if (pick) {
            reanchorAnnotation(review.reanchorTarget, pick);
            review.setReanchorTarget(null);
            pushToast("Marker re-placed", "info");
          } else {
            pushToast("Click on geometry to re-place (Esc cancels)", "warn");
          }
          return;
        }
        if (review.reviewMode) {
          const p = canvasPos(e);
          const pick = getClient().pickDetailed(p.x, p.y);
          if (pick) {
            review.setDraft({
              pick,
              screen: { x: p.x, y: p.y },
              text: "",
              category: "question",
            });
            return;
          }
          // A miss in review mode falls through to normal picking.
        }
        // A click: pick the geo node under the cursor (picking sync).
        const p = canvasPos(e);
        const hit = getClient().pick(p.x, p.y);
        if (hit !== undefined) {
          useMirror.getState().setCurrent("root");
          dispatch({ type: "setSelection", ctx: "root", ids: [hit] });
        }
      } catch {
        /* not booted */
      }
    };

    const onDoubleClick = (e: MouseEvent) => {
      // Enter the picked geo's subflow (decision 24).
      try {
        const p = canvasPos(e);
        const hit = getClient().pick(p.x, p.y);
        if (hit !== undefined) useMirror.getState().setCurrent({ subflow: hit });
      } catch {
        /* not booted */
      }
    };

    // Right-click opens the viewport context menu (the camera never used the
    // right button). setCtxMenu from useState is stable, so referencing it in
    // this mount-only effect is safe.
    const onContextMenu = (e: MouseEvent) => {
      e.preventDefault();
      setCtxMenu({ x: e.clientX, y: e.clientY });
    };

    const onWheel = (e: WheelEvent) => {
      try {
        getClient().wheel(-e.deltaY * 0.01);
      } catch {
        /* not booted */
      }
    };

    canvas.addEventListener("pointerenter", onEnter);
    canvas.addEventListener("pointerleave", onLeave);
    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("dblclick", onDoubleClick);
    canvas.addEventListener("contextmenu", onContextMenu);
    canvas.addEventListener("wheel", onWheel);
    return () => {
      canvas.removeEventListener("pointerenter", onEnter);
      canvas.removeEventListener("pointerleave", onLeave);
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerup", onPointerUp);
      canvas.removeEventListener("dblclick", onDoubleClick);
      canvas.removeEventListener("contextmenu", onContextMenu);
      canvas.removeEventListener("wheel", onWheel);
    };
  }, []);

  // The menu bar sits above the canvas; every canvas overlay (pane toolbars,
  // review pins, popup) lives inside the canvas host so their absolute
  // canvas-CSS-px coordinates keep their origin at the canvas top-left. The
  // ReviewPanel became its own dock panel in Phase 10 and is no longer here.
  return (
    <div className="viewport-pane">
      <ViewportMenuBar />
      <div className="viewport-canvas-host" ref={hostRef}>
        <ToolColumn />
        <GizmoReadout />
        <PaneToolbars />
        <ReviewOverlay />
        <ReviewPopup />
      </div>
      {ctxMenu && (
        <ViewportContextMenu x={ctxMenu.x} y={ctxMenu.y} onClose={() => setCtxMenu(null)} />
      )}
    </div>
  );
}
