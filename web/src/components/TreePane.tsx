// The Tree panel: a searchable outline of the whole scene, every context
// from the root down, with collapse/expand, double-click select-and-reveal
// and container dive. A pure mirror consumer: the derivation lives in
// treeModel.ts and rebuilds whenever the mirror replaces its objects, so
// the tree tracks every document change (including cook-driven display
// flag moves) with no subscription of its own.

import { useMemo, useState } from "react";
import { dispatch } from "../engine/session";
import { ctxKey } from "../engine/types";
import { diveIntoSubflow } from "../flow/nodeActions";
import { IconChevronDown, IconChevronRight } from "../icons";
import { useMirror } from "../store/mirror";
import { NodeGlyph } from "./NodeGlyph";
import { buildSceneTree, searchTree, type TreeRow } from "./treeModel";

/** The container-context tints, the exact tokens the canvas tints
 * container tiles with, so the tree's color language matches the graph. */
const CONTAINER_TINT: Record<string, string> = {
  geo: "var(--node-cat-container-geo)",
  tex: "var(--node-cat-container-tex)",
  mat: "var(--node-cat-container-mat)",
};

export function TreePane() {
  const registry = useMirror((s) => s.registry);
  const contexts = useMirror((s) => s.contexts);
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());

  const rows = useMemo(() => buildSceneTree(registry, contexts), [registry, contexts]);
  const search = useMemo(() => searchTree(rows, query), [rows, query]);
  const searching = query.trim().length > 0;

  const toggle = (key: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const select = (row: TreeRow) =>
    dispatch({ type: "setSelection", ctx: row.ctx, ids: [row.node.id] });

  const open = (row: TreeRow) => {
    if (row.opens !== null) {
      diveIntoSubflow(row.node.id);
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
    const isOpen = searching ? search.expand.has(row.key) : expanded.has(row.key);
    const selected =
      contexts[ctxKey(row.ctx)]?.selection.includes(row.node.id) ?? false;
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
          <NodeGlyph desc={row.desc} size={13} />
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
      </div>
      <div className="tree-body">{body}</div>
    </div>
  );
}
