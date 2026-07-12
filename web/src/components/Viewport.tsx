// The 3D viewport: a single WebGPU canvas driven by the Rust renderer.
// React never draws into it; it forwards pointer gestures to the host's
// pane-aware camera routing, runs the rAF cook+render loop, and floats
// one DOM toolbar per pane over the canvas (UX spec: panels are DOM, the
// canvas is one WebGPU surface with Rust-side pane hit-testing).

import { useEffect, useRef } from "react";
import {
  bootSession,
  dispatch,
  getClient,
  reanchorAnnotation,
  runFrame,
} from "../engine/session";
import { useMirror } from "../store/mirror";
import { useReview } from "../store/review";
import { useUi } from "../store/ui";
import { useViewState } from "../store/viewState";
import { pushToast } from "../store/toasts";
import { PaneToolbars } from "./PaneToolbar";
import { ReviewOverlay } from "./review/ReviewOverlay";
import { ReviewPanel } from "./review/ReviewPanel";
import { ReviewPopup } from "./review/ReviewPopup";
import { ViewportMenuBar } from "./ViewportMenuBar";

export function Viewport() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const setSelection = useMirror((s) => s.contexts); // subscribe so re-renders stay live
  void setSelection;
  // Crosshair while review mode is active (a className change; the canvas
  // element itself is never remounted).
  const reviewMode = useReview((s) => s.reviewMode);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let raf = 0;
    let mounted = true;

    bootSession(canvas)
      .catch((err: unknown) => {
        // WebGPU unavailable or wasm init failed: the boot overlay shows
        // the message (the full unsupported-browser page is Phase 8).
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

    // Keep the surface sized to the canvas backing store.
    const ro = new ResizeObserver(() => {
      if (!canvas || !mounted) return;
      const dpr = window.devicePixelRatio || 1;
      const w = Math.max(1, Math.floor(canvas.clientWidth * dpr));
      const h = Math.max(1, Math.floor(canvas.clientHeight * dpr));
      canvas.width = w;
      canvas.height = h;
      try {
        getClient().resize(w, h);
      } catch {
        /* not booted yet */
      }
    });
    ro.observe(canvas);

    return () => {
      mounted = false;
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, []);

  // Pointer routing: coordinates go to the host in canvas CSS px; the host
  // hit-tests the pane and drives that pane's camera controller. A press
  // with no drag is a pick; a double-click enters the picked geo's subflow.
  const drag = useRef<{ moved: boolean; downAt: number } | null>(null);

  const canvasPos = (e: React.PointerEvent | React.MouseEvent) => {
    const rect = (canvasRef.current as HTMLElement).getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  };

  const pickAt = (e: React.MouseEvent): number | undefined => {
    const p = canvasPos(e);
    return getClient().pick(p.x, p.y);
  };

  // The menu bar sits above the canvas; every overlay (pane toolbars,
  // review pins, popup, panel) lives inside the canvas host so their
  // absolute canvas-CSS-px coordinates keep their origin at the canvas
  // top-left. The canvas itself keeps a stable tree position (never
  // remounted; the WebGPU surface would be lost).
  return (
    <>
      <ViewportMenuBar />
      <div className="viewport-canvas-host">
      <canvas
        ref={canvasRef}
        className={`viewport-canvas${reviewMode ? " review-cursor" : ""}`}
        onPointerEnter={() => useViewState.getState().setPointerOverViewport(true)}
        onPointerLeave={() => useViewState.getState().setPointerOverViewport(false)}
        onPointerDown={(e) => {
          (e.target as HTMLElement).setPointerCapture(e.pointerId);
          drag.current = { moved: false, downAt: performance.now() };
          try {
            const p = canvasPos(e);
            getClient().pointerDown(p.x, p.y, e.button);
          } catch {
            /* not booted */
          }
        }}
        onPointerMove={(e) => {
          if (drag.current && (Math.abs(e.movementX) + Math.abs(e.movementY) > 1)) {
            drag.current.moved = true;
          }
          try {
            const p = canvasPos(e);
            getClient().pointerMove(p.x, p.y);
          } catch {
            /* not booted */
          }
        }}
        onPointerUp={(e) => {
          const d = drag.current;
          drag.current = null;
          try {
            getClient().pointerUp(e.button);
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
            const hit = pickAt(e);
            if (hit !== undefined) {
              useMirror.getState().setCurrent("root");
              dispatch({ type: "setSelection", ctx: "root", ids: [hit] });
            }
          } catch {
            /* not booted */
          }
        }}
        onDoubleClick={(e) => {
          // Enter the picked geo's subflow (decision 24).
          try {
            const hit = pickAt(e);
            if (hit !== undefined) {
              useMirror.getState().setCurrent({ subflow: hit });
            }
          } catch {
            /* not booted */
          }
        }}
        onContextMenu={(e) => e.preventDefault()}
        onWheel={(e) => {
          try {
            getClient().wheel(-e.deltaY * 0.01);
          } catch {
            /* not booted */
          }
        }}
      />
      <PaneToolbars />
      <ReviewOverlay />
      <ReviewPopup />
      <ReviewPanel />
      </div>
    </>
  );
}
