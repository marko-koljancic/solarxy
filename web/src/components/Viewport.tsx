// The 3D viewport: a single WebGPU canvas driven by the Rust renderer.
// React never draws into it; it only forwards pointer gestures to the
// camera and Rust-side picking, and runs the rAF cook+render loop.

import { useEffect, useRef } from "react";
import { bootSession, dispatch, getClient, hasPendingRecovery, runFrame } from "../engine/session";
import type { EngineEvent } from "../engine/types";
import { useMirror } from "../store/mirror";

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

  // Pointer camera control: left = orbit, middle/right = pan, wheel = dolly.
  // A press with no drag is a pick.
  const drag = useRef<{ x: number; y: number; button: number; moved: boolean } | null>(null);

  return (
    <canvas
      ref={canvasRef}
      className="viewport-canvas"
      onPointerDown={(e) => {
        (e.target as HTMLElement).setPointerCapture(e.pointerId);
        drag.current = { x: e.clientX, y: e.clientY, button: e.button, moved: false };
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const dx = e.clientX - d.x;
        const dy = e.clientY - d.y;
        if (Math.abs(dx) + Math.abs(dy) > 2) d.moved = true;
        d.x = e.clientX;
        d.y = e.clientY;
        try {
          if (d.button === 0) getClient().orbit(dx, dy);
          else getClient().pan(dx, dy);
        } catch {
          /* not booted */
        }
      }}
      onPointerUp={(e) => {
        const d = drag.current;
        drag.current = null;
        if (!d || d.moved) return;
        // A click: pick the geo node under the cursor.
        try {
          const rect = (e.target as HTMLElement).getBoundingClientRect();
          const dpr = window.devicePixelRatio || 1;
          const px = (e.clientX - rect.left) * dpr;
          const py = (e.clientY - rect.top) * dpr;
          const hit = getClient().pick(px, py);
          if (hit !== undefined) {
            // Picking sync: show the root canvas and select the producing geo
            // node so the parameter panel follows.
            useMirror.getState().setCurrent("root");
            dispatch({ type: "setSelection", ctx: "root", ids: [hit] });
          }
        } catch {
          /* not booted */
        }
      }}
      onContextMenu={(e) => e.preventDefault()}
      onWheel={(e) => {
        try {
          getClient().dolly(-e.deltaY * 0.002);
        } catch {
          /* not booted */
        }
      }}
    />
  );
}
