// The bottom properties drawer under the node editor: collapsible via the
// header chevron, drag-resizable 100-600px (Minimystix PropertiesDrawer).
// Contents stay mounted while collapsed so field state survives.

import { useCallback } from "react";
import { IconChevronDown, IconChevronRight } from "../../icons";
import { clampDrawer, useUi } from "../../store/ui";

const HEADER_PX = 30;

interface PropertiesDrawerProps {
  title: string;
  children: React.ReactNode;
}

export function PropertiesDrawer({ title, children }: PropertiesDrawerProps) {
  const height = useUi((s) => s.drawerHeight);
  const collapsed = useUi((s) => s.drawerCollapsed);

  const onHandleDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startHeight = useUi.getState().drawerHeight;
    const onMove = (ev: PointerEvent) => {
      // Dragging up grows the drawer.
      useUi.getState().setDrawerHeight(clampDrawer(startHeight + (startY - ev.clientY)));
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.classList.remove("row-resizing");
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    document.body.classList.add("row-resizing");
  }, []);

  return (
    <div
      className={`drawer${collapsed ? " collapsed" : ""}`}
      style={{ height: collapsed ? HEADER_PX : height }}
    >
      {!collapsed && <div className="drawer-resize" onPointerDown={onHandleDown} />}
      <button
        type="button"
        className="drawer-header"
        onClick={() => useUi.getState().toggleDrawerCollapsed()}
        title={collapsed ? "Expand properties" : "Collapse properties"}
      >
        {collapsed ? <IconChevronRight size={12} /> : <IconChevronDown size={12} />}
        <span className="drawer-title">Properties</span>
        <span className="drawer-context">{title}</span>
      </button>
      <div className="drawer-body" hidden={collapsed}>
        {children}
      </div>
    </div>
  );
}
