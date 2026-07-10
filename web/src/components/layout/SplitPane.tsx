// The main horizontal split (3D viewport + node editor) with the
// Minimystix hand-rolled draggable divider, clamped to 20-80 percent. The
// percentage lives in the ui store (persisted) and always means the
// VIEWPORT share, whichever side it sits on (desks can swap the sides,
// Phase 7b D3). When the viewport is maximized the panel side and divider
// unmount entirely.

import { useCallback, useRef } from "react";
import { clampSplit, useUi, type ViewportSide } from "../../store/ui";

interface SplitPaneProps {
  viewport: React.ReactNode;
  panel: React.ReactNode;
  side: ViewportSide;
}

export function SplitPane({ viewport, panel, side }: SplitPaneProps) {
  const splitPct = useUi((s) => s.splitPct);
  const maximized = useUi((s) => s.viewportMaximized);
  const containerRef = useRef<HTMLDivElement>(null);

  const onDividerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      const container = containerRef.current;
      if (!container) return;
      const rect = container.getBoundingClientRect();
      const onMove = (ev: PointerEvent) => {
        const pointerPct = ((ev.clientX - rect.left) / rect.width) * 100;
        // splitPct is the viewport share: mirror it when the viewport
        // sits on the right.
        const pct = side === "left" ? pointerPct : 100 - pointerPct;
        useUi.getState().setSplitPct(clampSplit(pct));
      };
      const onUp = () => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        document.body.classList.remove("col-resizing");
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      document.body.classList.add("col-resizing");
    },
    [side],
  );

  // Stable keys are load-bearing: on a side swap React must MOVE these DOM
  // nodes, not rebuild them by index, or the WebGPU canvas would be
  // recreated and the host's surface lost.
  const viewportEl = (
    <div
      key="viewport"
      className="split-viewport"
      style={{ width: maximized ? "100%" : `${splitPct}%` }}
    >
      {viewport}
    </div>
  );
  const panelEl = !maximized && (
    <div
      key="panel"
      className="split-panel"
      style={{ width: `calc(${100 - splitPct}% - var(--divider-size))` }}
    >
      {panel}
    </div>
  );
  const divider = !maximized && (
    <div key="divider" className="split-divider" onPointerDown={onDividerDown} />
  );

  return (
    <div ref={containerRef} className="split-pane">
      {side === "left" ? [viewportEl, divider, panelEl] : [panelEl, divider, viewportEl]}
    </div>
  );
}
