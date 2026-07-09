// The node palette: a pure interpreter of the registry snapshot, restyled
// to the Minimystix two-column command palette (categories left, nodes
// right, search on top, backdrop blur) with full keyboard navigation
// (arrows / Enter / Escape). Entries and categories derive from the
// descriptors, filtered by the current context; picking one dispatches
// AddNode. Opens on the "+" button or Tab.

import { useEffect, useMemo, useRef, useState } from "react";
import { dispatch } from "../engine/session";
import type { NodeTypeSnapshot } from "../engine/types";
import { selectGraph, useMirror } from "../store/mirror";

const ALL_CATEGORY = "All";

/** A permissive fuzzy score: substring on name/id/aliases beats nothing. */
function matches(node: NodeTypeSnapshot, q: string): boolean {
  if (!q) return true;
  const hay = [node.displayName, node.typeId, ...node.searchAliases].join(" ").toLowerCase();
  return hay.includes(q.toLowerCase());
}

export function NodePalette() {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState(ALL_CATEGORY);
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Tab opens the palette; Escape closes it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = (e.target as HTMLElement)?.tagName?.match(/INPUT|TEXTAREA|SELECT/);
      if (e.key === "Tab" && !typing) {
        e.preventDefault();
        setOpen((o) => !o);
      } else if (e.key === "Escape") {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (open) {
      setQuery("");
      setCategory(ALL_CATEGORY);
      setCursor(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const inRoot = current === "root";
  const contextNodes = useMemo(
    () => (registry?.nodes ?? []).filter((n) => (inRoot ? n.rootContext : n.subflowContext)),
    [registry, inRoot],
  );

  const categories = useMemo(() => {
    const cats = [...new Set(contextNodes.map((n) => n.category))].sort();
    return [ALL_CATEGORY, ...cats];
  }, [contextNodes]);

  const visible = useMemo(
    () =>
      contextNodes
        .filter((n) => category === ALL_CATEGORY || n.category === category)
        .filter((n) => matches(n, query)),
    [contextNodes, category, query],
  );

  useEffect(() => {
    setCursor((c) => Math.min(c, Math.max(0, visible.length - 1)));
  }, [visible]);

  const add = (typeId: string) => {
    const n = graph.nodes.length;
    const position: [number, number] = [80 + (n % 5) * 44, 80 + Math.floor(n / 5) * 90];
    dispatch({ type: "addNode", ctx: current, nodeType: typeId, position });
    setOpen(false);
  };

  const onSearchKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => Math.min(visible.length - 1, c + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => Math.max(0, c - 1));
    } else if (e.key === "ArrowRight" && !query) {
      e.preventDefault();
      setCategory((c) => categories[(categories.indexOf(c) + 1) % categories.length]);
      setCursor(0);
    } else if (e.key === "ArrowLeft" && !query) {
      e.preventDefault();
      setCategory(
        (c) => categories[(categories.indexOf(c) - 1 + categories.length) % categories.length],
      );
      setCursor(0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const pick = visible[cursor];
      if (pick) add(pick.typeId);
    }
  };

  // Keep the cursor row in view.
  useEffect(() => {
    listRef.current
      ?.querySelector(`[data-index="${cursor}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  return (
    <>
      <button className="palette-trigger" onClick={() => setOpen((o) => !o)} title="Add node (Tab)">
        + Add
      </button>
      {open && (
        <div className="palette-backdrop" onClick={() => setOpen(false)}>
          <div className="palette" onClick={(e) => e.stopPropagation()}>
            <input
              ref={inputRef}
              className="palette-search"
              placeholder="Search nodes..."
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setCursor(0);
              }}
              onKeyDown={onSearchKey}
            />
            <div className="palette-columns">
              <div className="palette-cats">
                {categories.map((cat) => (
                  <button
                    key={cat}
                    className={`palette-cat-btn${cat === category ? " active" : ""}`}
                    onClick={() => {
                      setCategory(cat);
                      setCursor(0);
                      inputRef.current?.focus();
                    }}
                  >
                    {cat}
                  </button>
                ))}
              </div>
              <div className="palette-list" ref={listRef}>
                {visible.length === 0 && (
                  <div className="palette-empty">No nodes for this context.</div>
                )}
                {visible.map((n, i) => (
                  <button
                    key={n.typeId}
                    data-index={i}
                    className={`palette-item${i === cursor ? " cursor" : ""}`}
                    onClick={() => add(n.typeId)}
                    onMouseEnter={() => setCursor(i)}
                    title={n.doc}
                  >
                    <span>{n.displayName}</span>
                    <span className="palette-item-cat">{n.category}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
