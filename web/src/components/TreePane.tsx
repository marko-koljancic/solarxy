// The Tree panel: a searchable outline of the whole scene, every context
// from the root down, with collapse/expand, double-click select-and-reveal
// and container dive.
//
// The derivation is the ENGINE'S. It used to be a fold over the mirror
// here, and the desktop's Node Tree had reimplemented that fold line for
// line; both now read one rule. The fold, the search and the collapse-all
// set arrive together in a single call, memoized on the mirror's contexts
// so it runs when the document moves rather than when this renders.

import { useMemo, useState } from "react";
import { dispatch, getClient } from "../engine/session";
import { ctxKey } from "../engine/types";
import { diveIntoSubflow } from "../flow/nodeActions";
import {
  IconChevronDown,
  IconChevronRight,
  IconChevronsDown,
  IconChevronsUp,
} from "../icons";
import { useMirror } from "../store/mirror";
import { NodeGlyph } from "./NodeGlyph";
import type { TreeRow } from "../engine/types";
import { descriptorFor } from "../registry/datatypes";

/** The container-context tints, the exact tokens the canvas tints
 * container tiles with, so the tree's color language matches the graph. */
const CONTAINER_TINT: Record<string, string> = {
  sop: "var(--node-cat-container-sop)",
  cop: "var(--node-cat-container-cop)",
  mat: "var(--node-cat-container-mat)",
};

export function TreePane() {
  const registry = useMirror((s) => s.registry);
  const contexts = useMirror((s) => s.contexts);
  const [query, setQuery] = useState("");
  // COLLAPSED keys, not expanded: the empty-set default means the whole
  // tree opens expanded, and nodes created later arrive expanded too.
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());

  // The engine folds the document and runs the search in one call. It is
  // memoized on the mirror's contexts, so it runs when the document moves
  // rather than when this component renders.
  const outline = useMemo(
    () => getClient().sceneOutline(query),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `contexts` is
    // the mirror's identity for "the document changed"; the fold reads the
    // engine, not this object.
    [contexts, query],
  );
  const rows = outline.rows;
  // The engine answers with arrays; the render wants membership tests.
  const search = useMemo(
    () => ({
      matches: new Set(outline.search?.matches ?? []),
      expand: new Set(outline.search?.expand ?? []),
    }),
    [outline],
  );
  const searching = query.trim().length > 0;

  const toggle = (key: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const select = (row: TreeRow) =>
    dispatch({ type: "setSelection", ctx: row.ctx, ids: [row.node] });

  const open = (row: TreeRow) => {
    if (row.opens !== null) {
      diveIntoSubflow(row.node);
      return;
    }
    // Select-and-reveal: current first, so the canvas mounts the right
    // graph before the selection paints.
    useMirror.getState().setCurrent(row.ctx);
    select(row);
  };

  const renderRow = (row: TreeRow): React.ReactNode => {
    // While searching, the visible rows are the matches plus their
    // ancestors (force-expanded); the manual expansion set is untouched,
    // so clearing the query restores it.
    if (searching && !search.matches.has(row.key) && !search.expand.has(row.key)) return null;
    const isOpen = searching ? search.expand.has(row.key) : !collapsed.has(row.key);
    const selected =
      contexts[ctxKey(row.ctx)]?.selection.includes(row.node) ?? false;
    const tint = row.opens !== null ? CONTAINER_TINT[row.opens] : undefined;
    return (
      <li key={row.key}>
        <div
          className={`tree-row${selected ? " selected" : ""}${searching && search.matches.has(row.key) ? " match" : ""}`}
          style={{ paddingLeft: `${row.depth * 14 + 4}px` }}
          onClick={() => select(row)}
          onDoubleClick={(e) => {
            e.stopPropagation();
            open(row);
          }}
        >
          {tint && <span className="tree-ctx-chip" style={{ background: tint }} aria-hidden />}
          {row.children.length > 0 ? (
            <button
              type="button"
              className="tree-chevron"
              aria-label={isOpen ? "Collapse" : "Expand"}
              aria-expanded={isOpen}
              onClick={(e) => {
                e.stopPropagation();
                toggle(row.key);
              }}
            >
              {isOpen ? <IconChevronDown size={11} /> : <IconChevronRight size={11} />}
            </button>
          ) : (
            <span className="tree-chevron spacer" aria-hidden />
          )}
          <NodeGlyph desc={descriptorFor(registry, row.typeId)} size={13} />
          <span className="tree-label">{row.label}</span>
          <span className="tree-type">{row.typeId}</span>
          {row.isDisplay && <span className="tree-display-dot" title="display flag" />}
        </div>
        {isOpen && row.children.length > 0 && <ul>{row.children.map(renderRow)}</ul>}
      </li>
    );
  };

  const body =
    rows.length === 0 ? (
      <div className="tree-empty">{registry ? "No nodes in the scene yet." : "No scene yet."}</div>
    ) : searching && search.matches.size === 0 ? (
      <div className="tree-empty">No nodes match &quot;{query.trim()}&quot;.</div>
    ) : (
      <ul className="tree-list">{rows.map(renderRow)}</ul>
    );

  return (
    <div className="tree-pane">
      <div className="tree-search">
        <input
          className="input-field"
          type="search"
          placeholder="Search nodes..."
          aria-label="Search nodes"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button
          type="button"
          className="tree-fold-btn"
          title="Expand all"
          aria-label="Expand all"
          onClick={() => setCollapsed(new Set())}
        >
          <IconChevronsDown size={12} />
        </button>
        <button
          type="button"
          className="tree-fold-btn"
          title="Collapse all"
          aria-label="Collapse all"
          onClick={() => setCollapsed(new Set(outline.branches))}
        >
          <IconChevronsUp size={12} />
        </button>
      </div>
      <div className="tree-body">{body}</div>
    </div>
  );
}
