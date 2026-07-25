// Drag-and-resize state for the shared Modal (and the modeless node-info
// card): header drag via pointer capture (the proven NodeInfoModal
// pattern), a bottom-right resize handle, viewport clamping, and per-id
// session size memory. The element starts centered by the backdrop's
// flexbox; the first drag or resize converts it to absolute coordinates
// (the backdrop is a fixed full-viewport box, so absolute equals
// viewport coordinates).

import { useCallback, useEffect, useRef, useState } from "react";

export interface ModalBounds {
  x: number;
  y: number;
  w: number | null;
  h: number | null;
}

/** Session-scoped bounds memory, keyed by modal id. Deliberately a module
 * map, not persisted prefs: a modal's size is a working arrangement, not
 * configuration. */
const remembered = new Map<string, ModalBounds>();

const MARGIN = 8;
/** How much of a dragged modal must stay reachable. */
const KEEP = 80;

/** Keeps a dragged box reachable: some of it always stays on screen.
 * Exported for tests. */
export function clampPos(x: number, y: number, w: number): { x: number; y: number } {
  return {
    x: Math.min(Math.max(x, MARGIN + KEEP - w), window.innerWidth - KEEP),
    y: Math.min(Math.max(y, MARGIN), window.innerHeight - KEEP * 0.5),
  };
}

export function useDragResize(opts: {
  id?: string;
  minWidth?: number;
  minHeight?: number;
}): {
  ref: React.RefObject<HTMLDivElement | null>;
  /** Inline style: empty while centered, absolute once moved or sized. */
  style: React.CSSProperties;
  headerProps: Pick<
    React.HTMLAttributes<HTMLElement>,
    "onPointerDown" | "onPointerMove" | "onPointerUp"
  >;
  resizeProps: Pick<
    React.HTMLAttributes<HTMLElement>,
    "onPointerDown" | "onPointerMove" | "onPointerUp"
  >;
} {
  const { id, minWidth = 280, minHeight = 140 } = opts;
  const ref = useRef<HTMLDivElement>(null);
  const [bounds, setBounds] = useState<ModalBounds | null>(() => {
    const saved = id ? remembered.get(id) : undefined;
    return saved ? { ...saved } : null;
  });
  const drag = useRef<{ dx: number; dy: number } | null>(null);
  const resize = useRef<{ x: number; y: number; w: number; h: number } | null>(null);

  // Persist on every settle (unmount included).
  useEffect(() => {
    if (id && bounds) remembered.set(id, bounds);
  }, [id, bounds]);

  /** The element's current viewport box, as absolute bounds. */
  const currentBounds = useCallback((): ModalBounds => {
    const rect = ref.current?.getBoundingClientRect();
    if (!rect) return { x: MARGIN, y: MARGIN, w: null, h: null };
    return { x: rect.left, y: rect.top, w: bounds?.w ?? null, h: bounds?.h ?? null };
  }, [bounds]);

  const headerProps = {
    onPointerDown: (e: React.PointerEvent) => {
      // Buttons in the titlebar stay clickable, not draggable.
      if ((e.target as Element).closest("button, input, select, a")) return;
      const b = currentBounds();
      setBounds(b);
      drag.current = { dx: e.clientX - b.x, dy: e.clientY - b.y };
      (e.target as Element).setPointerCapture(e.pointerId);
    },
    onPointerMove: (e: React.PointerEvent) => {
      if (!drag.current) return;
      setBounds((prev) => {
        if (!prev) return prev;
        const d = drag.current;
        if (!d) return prev;
        const next = clampPos(e.clientX - d.dx, e.clientY - d.dy, prev.w ?? 400);
        return { ...prev, ...next };
      });
    },
    onPointerUp: () => {
      drag.current = null;
    },
  };

  const resizeProps = {
    onPointerDown: (e: React.PointerEvent) => {
      const rect = ref.current?.getBoundingClientRect();
      if (!rect) return;
      // Lock the top-left corner; grow from the bottom-right.
      setBounds({ x: rect.left, y: rect.top, w: rect.width, h: rect.height });
      resize.current = { x: e.clientX, y: e.clientY, w: rect.width, h: rect.height };
      (e.target as Element).setPointerCapture(e.pointerId);
      e.preventDefault();
      e.stopPropagation();
    },
    onPointerMove: (e: React.PointerEvent) => {
      if (!resize.current) return;
      setBounds((prev) => {
        const r = resize.current;
        if (!prev || !r) return prev;
        const w = Math.min(
          Math.max(minWidth, r.w + (e.clientX - r.x)),
          window.innerWidth - prev.x - MARGIN,
        );
        const h = Math.min(
          Math.max(minHeight, r.h + (e.clientY - r.y)),
          window.innerHeight - prev.y - MARGIN,
        );
        return { ...prev, w, h };
      });
    },
    onPointerUp: () => {
      resize.current = null;
    },
  };

  const style: React.CSSProperties = bounds
    ? {
        position: "absolute",
        left: bounds.x,
        top: bounds.y,
        margin: 0,
        ...(bounds.w !== null ? { width: bounds.w } : {}),
        ...(bounds.h !== null ? { height: bounds.h } : {}),
        ...(bounds.w !== null || bounds.h !== null ? { maxWidth: "none", maxHeight: "none" } : {}),
      }
    : {};

  return { ref, style, headerProps, resizeProps };
}
