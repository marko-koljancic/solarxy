// The node palette: a pure interpreter of the registry snapshot, restyled
// to the Minimystix two-column command palette (categories left, nodes
// right, search on top, backdrop blur) with full keyboard navigation
// (arrows / Enter / Escape). Entries and categories derive from the
// descriptors, filtered by the current context; picking one dispatches
// AddNode. Opens on the "+" button or Tab.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { NodeGlyph } from "./NodeGlyph";
import { Popover, renderDoc } from "./Popover";
import { dispatch } from "../engine/session";
import type { NodeTypeSnapshot } from "../engine/types";
import { screenToFlow } from "../flow/flowProjection";
import { MARGIN_PX, palettePlacement, type Point } from "../flow/palettePlacement";
import { compareCategories, contextKind } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { useUi } from "../store/ui";

const ALL_CATEGORY = "All";

/** The pane the palette must open inside and place nodes into. */
const paneRect = (): DOMRect | null =>
  document.querySelector(".node-canvas-host")?.getBoundingClientRect() ?? null;

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
  // Open state lives in the ui store: the Add menu's
  // "Search Nodes..." entry toggles it from outside this component.
  const open = useUi((s) => s.paletteOpen);
  const setOpen = (o: boolean) => useUi.getState().setPaletteOpen(o);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState(ALL_CATEGORY);
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const paletteRef = useRef<HTMLDivElement>(null);
  // The pointer at the moment the palette opened. A ref, not state: it is
  // written on every pointermove and must not re-render the app to do it.
  const pointer = useRef<Point | null>(null);
  const [at, setAt] = useState<Point | null>(null);
  const [paneHeight, setPaneHeight] = useState<number | null>(null);

  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      pointer.current = { x: e.clientX, y: e.clientY };
    };
    window.addEventListener("pointermove", onMove, { passive: true });
    return () => window.removeEventListener("pointermove", onMove);
  }, []);

  // Tab opens the palette; Escape closes it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = (e.target as HTMLElement)?.tagName?.match(/INPUT|TEXTAREA|SELECT/);
      const ui = useUi.getState();
      if (e.key === "Tab" && !typing) {
        e.preventDefault();
        ui.setPaletteOpen(!ui.paletteOpen);
      } else if (e.key === "Escape") {
        ui.setPaletteOpen(false);
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

  const rootNodes = useMirror((s) => selectGraph(s, "root").nodes);
  const kind = contextKind(registry, current, rootNodes);
  const contextNodes = useMemo(
    () => (registry?.nodes ?? []).filter((n) => n.contexts.includes(kind)),
    [registry, kind],
  );

  // Id/label pairs: filtering stays keyed by the stable snake_case id, the
  // buttons render the Title Case label from the snapshot. The snapshot
  // lists nodes alphabetically by type id, so the curated CATEGORY_ORDER
  // decides presentation; plain alphabetical would scatter related
  // categories.
  const categories = useMemo(() => {
    const labels = new Map(contextNodes.map((n) => [n.category, n.categoryLabel]));
    return [
      { id: ALL_CATEGORY, label: ALL_CATEGORY },
      ...[...labels.entries()]
        .sort(([a], [b]) => compareCategories(a, b))
        .map(([id, label]) => ({ id, label })),
    ];
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

  // Placement, from the MEASURED panel, before paint.
  //
  // This owns `at` outright rather than refining a guess set elsewhere: a
  // layout effect runs BEFORE the plain effects, so a guess written in a
  // `useEffect` would land after this and win, which is exactly the bug that
  // put the panel's foot 44px below the pane.
  //
  // The height cannot be known up front — the list grows with the registry
  // and is capped against the pane — so this settles over at most two passes:
  // measure uncapped, publish the pane height, re-measure capped. The
  // half-pixel guard stops it there instead of looping.
  useLayoutEffect(() => {
    if (!open) {
      setAt(null);
      return;
    }
    const pane = paneRect();
    const panel = paletteRef.current?.getBoundingClientRect();
    if (!pane || !panel) return;
    setPaneHeight(pane.height);
    const next = palettePlacement(pointer.current, pane, panel);
    setAt((prev) =>
      prev && Math.abs(prev.x - next.x) < 0.5 && Math.abs(prev.y - next.y) < 0.5 ? prev : next,
    );
  }, [open, paneHeight, visible.length, category]);

  const add = (typeId: string) => {
    dispatch({ type: "addNode", ctx: current, nodeType: typeId, position: addPosition() });
    setOpen(false);
  };

  /** Where the new node lands, in graph coordinates.
   *
   * At the pointer that opened the palette, which is the Blender/Houdini
   * contract: the node appears where you were looking. It used to derive from
   * the node COUNT in a fixed 5-column grid, so a new node could land anywhere
   * except where your attention was — and, past five nodes, on top of an
   * existing one.
   *
   * Falls back to the old grid only when there is no projection to ask (list
   * view has no ReactFlow mounted). */
  const addPosition = (): [number, number] => {
    const flow = at ? screenToFlow(at) : null;
    if (flow) return [flow.x, flow.y];
    const n = graph.nodes.length;
    return [80 + (n % 5) * 44, 80 + Math.floor(n / 5) * 90];
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
      setCategory((c) => {
        const i = categories.findIndex((cat) => cat.id === c);
        return categories[(i + 1) % categories.length].id;
      });
      setCursor(0);
    } else if (e.key === "ArrowLeft" && !query) {
      e.preventDefault();
      setCategory((c) => {
        const i = categories.findIndex((cat) => cat.id === c);
        return categories[(i - 1 + categories.length) % categories.length].id;
      });
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
      {open && (
        <div className="palette-backdrop" onClick={() => setOpen(false)}>
          <div
            ref={paletteRef}
            className="palette"
            // Positioned from JS against the node pane, not from CSS against
            // the viewport. The backdrop is `position: fixed`, so it is the
            // containing block: any static offsets here resolve against the
            // window and ignore the pane entirely.
            //
            // The height cap is the pane's, not the viewport's: a 60vh panel
            // does not fit a short Nodes panel, and a palette taller than the
            // pane cannot be placed inside it at all.
            style={{
              ...(at ? { left: `${at.x}px`, top: `${at.y}px` } : {}),
              ...(paneHeight ? { maxHeight: `${paneHeight - 2 * MARGIN_PX}px` } : {}),
            }}
            onClick={(e) => e.stopPropagation()}
          >
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
                    key={cat.id}
                    className={`palette-cat-btn${cat.id === category ? " active" : ""}`}
                    onClick={() => {
                      setCategory(cat.id);
                      setCursor(0);
                      inputRef.current?.focus();
                    }}
                  >
                    {cat.label}
                  </button>
                ))}
              </div>
              <div className="palette-list" ref={listRef}>
                {visible.length === 0 && (
                  <div className="palette-empty">No nodes for this context.</div>
                )}
                {visible.map((n, i) => (
                  <Popover key={n.typeId} title={n.displayName} content={renderDoc(n.doc)}>
                    <button
                      data-index={i}
                      className={`palette-item${i === cursor ? " cursor" : ""}`}
                      onClick={() => add(n.typeId)}
                      onMouseEnter={() => setCursor(i)}
                    >
                      <span className="palette-item-name">
                        <NodeGlyph desc={n} />
                        {n.displayName}
                      </span>
                      <span className="palette-item-cat">{n.categoryLabel}</span>
                    </button>
                  </Popover>
                ))}
              </div>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
