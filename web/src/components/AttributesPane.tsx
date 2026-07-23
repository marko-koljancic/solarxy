// The Attributes pane: a read-only spreadsheet over the watched node's
// cooked geometry (first selected node, else the display-flag node).
// Point and Primitive domain tabs; rows are virtualized and fetched in
// pages through the engine's attribute_table query, so a 100k-point
// scatter scrolls without ever materializing more than the visible
// window. Refreshes on selection change, on the watched node's cook
// completing, and on tab switch; never polls.

import { useEffect, useMemo, useRef, useState } from "react";
import { getClient } from "../engine/session";
import type { AttrDomain, AttributePage, AttributeSummary } from "../engine/types";
import { nodeLabel } from "../flow/nodeLabel";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { fmtCell, headerCells, pageWindow, watchedNode } from "./attributesTable";

const ROW_H = 22;
const PAGE_SIZE = 128;
const PAGE_CACHE_CAP = 16;

export function AttributesPane() {
  const registry = useMirror((s) => s.registry);
  const ctx = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const node = watchedNode(graph.selection, graph.activeOutput);
  const nodeMirror = graph.nodes.find((n) => n.id === node) ?? null;
  // The cook status object is replaced when a cook lands, so subscribing
  // to it is the refresh trigger for a recooked watched node.
  const cookStatus = useMirror((s) => (node === null ? undefined : s.cook[node]?.status));

  const [domain, setDomain] = useState<AttrDomain>("point");
  const [summary, setSummary] = useState<AttributeSummary | undefined>();
  const [, setFetchTick] = useState(0);

  const pagesRef = useRef<Map<number, AttributePage>>(new Map());
  const scrollRef = useRef<HTMLDivElement>(null);
  const [scroll, setScroll] = useState({ top: 0, height: 300 });

  // Selection / cook / context / domain changes invalidate everything
  // cached (the page cache is keyed by page index alone, so a domain
  // switch MUST clear it or the other domain's pages would serve).
  useEffect(() => {
    pagesRef.current.clear();
    setSummary(node === null ? undefined : getClient().attributeSummary(node));
  }, [node, cookStatus, ctx, domain]);

  const total = domain === "point" ? (summary?.points ?? 0) : (summary?.primitiveElements ?? 0);
  const window_ = pageWindow(scroll.top, scroll.height, ROW_H, total, PAGE_SIZE);

  // Fetch any pages the window needs but the cache lacks (synchronous
  // wasm calls, a few KB each); cap the cache LRU-style.
  useEffect(() => {
    if (node === null) return;
    const cache = pagesRef.current;
    let fetched = false;
    for (const p of window_.pages) {
      if (cache.has(p)) continue;
      const page = getClient().attributeTable(node, domain, p * PAGE_SIZE, PAGE_SIZE);
      if (page) {
        cache.set(p, page);
        fetched = true;
      }
    }
    while (cache.size > PAGE_CACHE_CAP) {
      const oldest = cache.keys().next().value;
      if (oldest === undefined) break;
      cache.delete(oldest);
    }
    if (fetched) setFetchTick((t) => t + 1);
  }, [node, domain, window_.first, window_.last, cookStatus]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const measure = () => setScroll({ top: el.scrollTop, height: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [node === null]);

  const columns = useMemo(() => {
    const first = pagesRef.current.get(window_.pages[0] ?? 0);
    return first?.columns ?? [];
  }, [window_.pages, summary, domain, cookStatus]);

  if (node === null) {
    return <div className="attr-pane-empty">No node selected and no display flag set.</div>;
  }
  if (!summary) {
    return <div className="attr-pane-empty">No cooked geometry on this node yet.</div>;
  }

  const heads = headerCells(columns);
  const rows: { index: number; cells: (number | null)[] | null }[] = [];
  for (let i = window_.first; i < window_.last; i += 1) {
    const page = pagesRef.current.get(Math.floor(i / PAGE_SIZE));
    rows.push({ index: i, cells: page?.rows[i - page.offset] ?? null });
  }

  const label = nodeMirror
    ? nodeLabel(nodeMirror, descriptorFor(registry, nodeMirror.typeId))
    : `node ${node}`;
  const emptyDomain =
    total === 0
      ? domain === "point"
        ? "No points."
        : "No primitive attributes on this geometry."
      : null;

  return (
    <div className="attr-pane">
      <div className="attr-pane-header">
        <span className="attr-pane-node" title="The watched node">
          {label}
        </span>
        <div className="param-tabs attr-pane-tabs" role="tablist">
          {(["point", "primitive"] as const).map((d) => (
            <button
              key={d}
              role="tab"
              aria-selected={domain === d}
              className={`param-tab${domain === d ? " active" : ""}`}
              onClick={() => setDomain(d)}
            >
              {d === "point" ? "Point" : "Primitive"}
            </button>
          ))}
        </div>
        <span className="attr-pane-count">
          {total.toLocaleString()} {domain === "point" ? "points" : "prims"}
        </span>
      </div>
      {emptyDomain ? (
        <div className="attr-pane-empty">{emptyDomain}</div>
      ) : (
        <div
          className="attr-pane-scroll"
          ref={scrollRef}
          onScroll={(e) =>
            setScroll({ top: e.currentTarget.scrollTop, height: e.currentTarget.clientHeight })
          }
        >
          <div className="attr-table-head" role="row">
            <span className="attr-cell attr-cell-num">#</span>
            {heads.map((h) => (
              <span key={h} className="attr-cell">
                {h}
              </span>
            ))}
          </div>
          <div className="attr-table-body" style={{ height: total * ROW_H }}>
            {rows.map((r) => (
              <div
                key={r.index}
                className={`attr-row${r.index % 2 === 1 ? " striped" : ""}`}
                role="row"
                style={{ transform: `translateY(${r.index * ROW_H}px)` }}
              >
                <span className="attr-cell attr-cell-num">{r.index}</span>
                {r.cells
                  ? r.cells.map((v, i) => (
                      <span key={i} className="attr-cell">
                        {fmtCell(v)}
                      </span>
                    ))
                  : heads.map((h) => (
                      <span key={h} className="attr-cell attr-cell-pending">
                        &middot;
                      </span>
                    ))}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
