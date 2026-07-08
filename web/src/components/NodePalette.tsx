// The node palette: a pure interpreter of the registry snapshot. Entries,
// categories, and search all derive from the descriptors and are filtered
// by the current context (root vs subflow). Picking one dispatches AddNode.
// Opens on the "+" button or the Tab key.

import { useEffect, useMemo, useRef, useState } from "react";
import { dispatch } from "../engine/session";
import type { NodeTypeSnapshot } from "../engine/types";
import { selectGraph, useMirror } from "../store/mirror";

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
  const inputRef = useRef<HTMLInputElement>(null);

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
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const inRoot = current === "root";
  const entries = useMemo(() => {
    const nodes = (registry?.nodes ?? [])
      .filter((n) => (inRoot ? n.rootContext : n.subflowContext))
      .filter((n) => matches(n, query));
    // Group by category (alphabetical categories).
    const byCat = new Map<string, NodeTypeSnapshot[]>();
    for (const n of nodes) {
      const g = byCat.get(n.category) ?? [];
      g.push(n);
      byCat.set(n.category, g);
    }
    return [...byCat.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [registry, inRoot, query]);

  const add = (typeId: string) => {
    const n = graph.nodes.length;
    const position: [number, number] = [80 + (n % 5) * 44, 80 + Math.floor(n / 5) * 90];
    dispatch({ type: "addNode", ctx: current, nodeType: typeId, position });
    setOpen(false);
  };

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
              onChange={(e) => setQuery(e.target.value)}
            />
            <div className="palette-list">
              {entries.length === 0 && <div className="palette-empty">No nodes for this context.</div>}
              {entries.map(([cat, nodes]) => (
                <div key={cat} className="palette-cat">
                  <div className="palette-cat-title">{cat}</div>
                  {nodes.map((n) => (
                    <button key={n.typeId} className="palette-item" onClick={() => add(n.typeId)} title={n.doc}>
                      {n.displayName}
                    </button>
                  ))}
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
