// Pure helpers behind the Attributes pane: watched-node resolution, the
// virtualization window math, and cell formatting. Split from the
// component so the logic is unit-testable without a DOM.

import type { AttrColumn } from "../engine/types";

/** The node the pane watches: the first selected node, else the
 * display-flag node, else nothing. */
export function watchedNode(selection: number[], activeOutput: number | null): number | null {
  return selection[0] ?? activeOutput ?? null;
}

/** The row window a scroll position needs, padded by `overscan` rows, and
 * the page indices (of `pageSize`-row pages) covering it. */
export function pageWindow(
  scrollTop: number,
  viewportHeight: number,
  rowHeight: number,
  total: number,
  pageSize: number,
  overscan = 8,
): { first: number; last: number; pages: number[] } {
  if (total === 0) return { first: 0, last: 0, pages: [] };
  // A stale scrollTop can outlive a shrinking total (recook, node swap);
  // clamping `first` keeps the window inside the data until the container
  // snaps its scroll position back.
  const first = Math.min(Math.max(0, Math.floor(scrollTop / rowHeight) - overscan), total);
  const last = Math.min(total, Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscan);
  const pages: number[] = [];
  for (let p = Math.floor(first / pageSize); p * pageSize < last; p += 1) pages.push(p);
  return { first, last, pages };
}

/** Fixed-decimal cell text: 4 places, `-0` normalized, missing lanes a
 * plain hyphen. */
export function fmtCell(v: number | null): string {
  if (v === null || Number.isNaN(v)) return "-";
  const fixed = v.toFixed(4);
  return fixed === "-0.0000" ? "0.0000" : fixed;
}

/** Flat header cells for a column set: single-component columns keep the
 * lane name, vector lanes fan out as `.x .y .z .w`. */
export function headerCells(columns: AttrColumn[]): string[] {
  const suffix = ["x", "y", "z", "w"];
  return columns.flatMap((c) =>
    c.components === 1
      ? [c.key]
      : Array.from({ length: c.components }, (_, i) => `${c.key}.${suffix[i]}`),
  );
}
