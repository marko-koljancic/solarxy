// Middle-mouse precision drag for numeric fields, carried from Minimystix
// Horizontal drag scrubs the value at the selected
// precision decade, vertical drag selects the decade on a floating overlay
// (6px deadzone, 12px hysteresis, rows of 28px), Escape cancels, window
// blur cancels, rAF-throttled. Values flow through the caller's
// preview/commit lanes: preview during the drag, exactly one commit on
// release (or a revert preview on cancel).

import { useCallback, useEffect, useRef, useState } from "react";

export const PRECISION_DECADES = [1, 0.1, 0.01, 0.001, 0.0001, 0.00001] as const;
export const ROW_HEIGHT = 28;
export const DEADZONE = 6;
export const HYSTERESIS = 12;
const DEFAULT_INDEX = 2; // 0.01
const SENSITIVITY = 0.5;

/** Pure decade selection: vertical distance from the drag origin picks a
 * row (relative to the default index), gated by the deadzone and, once a
 * row is selected, by the hysteresis distance from the last change. Returns
 * the new index and the y that produced it (for the next hysteresis gate). */
export function selectDecade(
  deltaY: number,
  mouseY: number,
  currentIndex: number,
  lastChangeY: number,
  decadeCount: number = PRECISION_DECADES.length,
): { index: number; changeY: number } {
  if (Math.abs(deltaY) < DEADZONE) return { index: currentIndex, changeY: lastChangeY };
  const rawRowIndex = Math.floor(deltaY / ROW_HEIGHT);
  const candidate = Math.max(0, Math.min(decadeCount - 1, rawRowIndex + DEFAULT_INDEX));
  if (candidate !== currentIndex && Math.abs(mouseY - lastChangeY) >= HYSTERESIS) {
    return { index: candidate, changeY: mouseY };
  }
  return { index: currentIndex, changeY: lastChangeY };
}

/** Pure value scrub: horizontal delta times the decade times sensitivity,
 * clamped to the hard range and snapped for ints. */
export function scrubValue(
  original: number,
  deltaX: number,
  decade: number,
  opts: { min?: number; max?: number; int?: boolean } = {},
): number {
  let v = original + deltaX * decade * SENSITIVITY;
  if (opts.int) v = Math.round(v);
  if (opts.min !== undefined) v = Math.max(opts.min, v);
  if (opts.max !== undefined) v = Math.min(opts.max, v);
  return v;
}

export interface PrecisionDragState {
  dragging: boolean;
  decadeIndex: number;
  overlay: { x: number; y: number };
}

export interface PrecisionDragConfig {
  min?: number;
  max?: number;
  int?: boolean;
  onPreview: (v: number) => void;
  onCommit: (v: number) => void;
}

/** Binds `onMouseDown` (middle button) on a field. During the drag the
 * hook previews values; release commits once; Escape or window blur
 * reverts via a preview of the original value. */
export function usePrecisionDrag(value: number, config: PrecisionDragConfig) {
  const [state, setState] = useState<PrecisionDragState>({
    dragging: false,
    decadeIndex: DEFAULT_INDEX,
    overlay: { x: 0, y: 0 },
  });
  const drag = useRef({
    origin: { x: 0, y: 0 },
    lastChangeY: 0,
    originalValue: 0,
    currentValue: 0,
    decadeIndex: DEFAULT_INDEX,
    raf: 0,
  });
  const cfg = useRef(config);
  cfg.current = config;

  const stop = useCallback((commit: boolean) => {
    setState((prev) => {
      if (!prev.dragging) return prev;
      if (commit) cfg.current.onCommit(drag.current.currentValue);
      else cfg.current.onPreview(drag.current.originalValue);
      return { ...prev, dragging: false };
    });
    cancelAnimationFrame(drag.current.raf);
    document.body.classList.remove("precision-drag-active");
  }, []);

  useEffect(() => {
    if (!state.dragging) return;
    const onMove = (e: MouseEvent) => {
      cancelAnimationFrame(drag.current.raf);
      drag.current.raf = requestAnimationFrame(() => {
        const d = drag.current;
        const sel = selectDecade(
          e.clientY - d.origin.y,
          e.clientY,
          d.decadeIndex,
          d.lastChangeY,
        );
        d.decadeIndex = sel.index;
        d.lastChangeY = sel.changeY;
        const c = cfg.current;
        d.currentValue = scrubValue(
          d.originalValue,
          e.clientX - d.origin.x,
          PRECISION_DECADES[sel.index],
          { min: c.min, max: c.max, int: c.int },
        );
        setState((prev) =>
          prev.decadeIndex === sel.index ? prev : { ...prev, decadeIndex: sel.index },
        );
        c.onPreview(d.currentValue);
      });
    };
    const onUp = () => stop(true);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        stop(false);
      }
    };
    const onBlur = () => stop(false);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [state.dragging, stop]);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 1) return;
      e.preventDefault();
      e.stopPropagation();
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      drag.current = {
        origin: { x: e.clientX, y: e.clientY },
        lastChangeY: e.clientY,
        originalValue: value,
        currentValue: value,
        decadeIndex: DEFAULT_INDEX,
        raf: 0,
      };
      setState({
        dragging: true,
        decadeIndex: DEFAULT_INDEX,
        overlay: { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 },
      });
      document.body.classList.add("precision-drag-active");
    },
    [value],
  );

  return { bind: { onMouseDown }, state };
}
