// The marker position channel: pin elements register here (keyed
// "pane:id"), and the rAF loop patches their transforms imperatively from
// `review_markers()` every frame. React renders marker structure only on
// `reviewChanged`; positions never trigger a re-render.

import type { MarkerScreen } from "./types";

const elements = new Map<string, HTMLElement>();

export function markerKey(pane: number, id: number): string {
  return `${pane}:${id}`;
}

/** Registers (or unregisters, with null) a pin element. Fresh pins start
 * hidden until the first position patch lands. */
export function registerMarker(key: string, el: HTMLElement | null): void {
  if (el) {
    el.style.visibility = "hidden";
    elements.set(key, el);
  } else {
    elements.delete(key);
  }
}

/** Applies one frame's positions: registered pins present in the list are
 * placed and shown; the rest (culled, behind the camera, off-pane) hide. */
export function applyMarkerPositions(markers: MarkerScreen[]): void {
  if (elements.size === 0) return;
  const placed = new Set<string>();
  for (const m of markers) {
    const key = markerKey(m.pane, m.id);
    const el = elements.get(key);
    if (!el) continue;
    el.style.transform = `translate3d(${m.x}px, ${m.y}px, 0)`;
    el.style.visibility = "visible";
    placed.add(key);
  }
  for (const [key, el] of elements) {
    if (!placed.has(key)) el.style.visibility = "hidden";
  }
}

/** Hides every registered pin (markers toggled off / review data cleared). */
export function hideAllMarkers(): void {
  for (const el of elements.values()) el.style.visibility = "hidden";
}
