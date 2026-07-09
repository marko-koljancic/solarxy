// The 3D viewport: a single WebGPU canvas driven by the Rust renderer.
// React never draws into it; it forwards pointer gestures to the host's
// pane-aware camera routing, runs the rAF cook+render loop, and floats
// one DOM toolbar per pane over the canvas (UX spec: panels are DOM, the
// canvas is one WebGPU surface with Rust-side pane hit-testing).

import { useEffect, useRef } from "react";
import { bootSession, dispatch, getClient, hasPendingRecovery, runFrame } from "../engine/session";
import type { EngineEvent } from "../engine/types";
import { useMirror } from "../store/mirror";
import { useViewState } from "../store/viewState";
import { PaneToolbars } from "./PaneToolbar";

/** Narrows a batch to the first nodeAdded event's node id. */
function firstAddedId(events: EngineEvent[]): number | undefined {
  const ev = events.find(
    (e): e is Extract<EngineEvent, { type: "nodeAdded" }> => e.type === "nodeAdded",
  );
  return ev?.node.id;
}

export function Viewport() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const setSelection = useMirror((s) => s.contexts); // subscribe so re-renders stay live
  void setSelection;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let raf = 0;
    let mounted = true;

    bootSession(canvas).then(() => {
      if (!mounted) return;

      // Seed demo content on a truly fresh boot only (no prior autosave to
      // recover and an empty document). Real content comes from the palette.
      if (useMirror.getState().contexts["root"].nodes.length === 0 && !hasPendingRecovery()) {
        const b = dispatch({ type: "addNode", ctx: "root", nodeType: "geo", position: [60, 60] });
        const geoId = firstAddedId(b.events);
        if (geoId !== undefined) {
          dispatch({ type: "addNode", ctx: { subflow: geoId }, nodeType: "box", position: [80, 80] });
        }
        dispatch({ type: "addNode", ctx: "root", nodeType: "directional_light", position: [60, 220] });
      }

      let last = performance.now();
      const loop = (t: number) => {
        runFrame(t - last);
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

  return (
    <>
      <canvas
        ref={canvasRef}
        className="viewport-canvas"
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
    </>
  );
}
