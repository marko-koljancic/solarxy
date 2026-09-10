// What is left of the Attributes pane's helpers after the shared rules
// moved into `solarxy-studio`.
//
// The CELL and HEADER formatting are gone: the engine formats a page where
// the values are, because the cell rule runs per visible cell while the
// table scrolls, which is far too often to ask across the boundary one at
// a time.
//
// Two stay, and for different reasons.
//
// `pageWindow` is virtualization: which rows to materialize from a scroll
// offset and a viewport height. That is a fact about a scrolling
// container, not about a document, and the two shells scroll with
// different machinery.
//
// `watchedNode` is the one rule this release did NOT single-source, and
// the reason is stated rather than hidden. It exists in Rust too, as
// `solarxy_studio::attributes::watched_node`, which the desktop pane will
// read. Crossing the WebAssembly boundary to compute `selection[0] ??
// activeOutput` from two values the caller already holds is not a trade
// worth making, and the rule is one line whose behaviour is pinned on
// both sides. If it ever grows, it moves.

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
