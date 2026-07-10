// The right-docked properties column (Phase 7b D3 desks): a full-height
// panel beside the split with a left drag-resize handle. The bottom
// drawer's sibling; which one renders is the ui store's propertiesDock.

import { useCallback } from "react";
import { clampDrawerWidth, useUi } from "../../store/ui";

interface PropertiesSideProps {
  title: string;
  children: React.ReactNode;
}

export function PropertiesSide({ title, children }: PropertiesSideProps) {
  const width = useUi((s) => s.drawerWidth);

  const onHandleDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = useUi.getState().drawerWidth;
    const onMove = (ev: PointerEvent) => {
      // Dragging left grows the panel.
      useUi.getState().setDrawerWidth(clampDrawerWidth(startWidth + (startX - ev.clientX)));
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
    <div className="side-panel" style={{ width }}>
      <div className="side-panel-resize" onPointerDown={onHandleDown} />
      <div className="side-panel-content">
        <div className="side-panel-header">
          <span className="drawer-title">Properties</span>
          <span className="drawer-context">{title}</span>
        </div>
        <div className="side-panel-body">{children}</div>
      </div>
    </div>
  );
}
