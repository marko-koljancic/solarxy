// The main horizontal split (3D viewport left, node editor right) with the
// Minimystix hand-rolled draggable divider, clamped to 20-80 percent. The
// percentage lives in the ui store (persisted). When the viewport is
// maximized the right pane and divider unmount entirely.

import { useCallback, useRef } from "react";
import { clampSplit, useUi } from "../../store/ui";

interface SplitPaneProps {
  left: React.ReactNode;
  right: React.ReactNode;
}

export function SplitPane({ left, right }: SplitPaneProps) {
  const splitPct = useUi((s) => s.splitPct);
  const maximized = useUi((s) => s.viewportMaximized);
  const containerRef = useRef<HTMLDivElement>(null);

  const onDividerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    const container = containerRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const onMove = (ev: PointerEvent) => {
      const pct = ((ev.clientX - rect.left) / rect.width) * 100;
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
  }, []);

  return (
    <div ref={containerRef} className="split-pane">
      <div className="split-left" style={{ width: maximized ? "100%" : `${splitPct}%` }}>
        {left}
      </div>
      {!maximized && (
        <>
          <div className="split-divider" onPointerDown={onDividerDown} />
          <div className="split-right" style={{ width: `calc(${100 - splitPct}% - var(--divider-size))` }}>
            {right}
          </div>
        </>
      )}
    </div>
  );
}
