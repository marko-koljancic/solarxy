// The Houdini-style hover radial (Phase 7b; revamp D-7 revised): six
// wedges on a 45-degree pitch with 39-degree sweeps, centred at E rename /
// NE display / NW dive (containers) / W info / SW bypass / SE delete.
// North and south stay OPEN so the node's wires remain visible through
// the ring. Active segments fill accent amber. Rendered in a body portal
// at the node's viewport position; the container is pointer-transparent
// so the node underneath stays fully interactive, and the ring closes
// when the pointer strays past a grace radius, on any outside pointerdown
// (drag/marquee/connect starts), or on Esc.
//
// Phase 10: the ring TRACKS its node. It renders inside the ReactFlowProvider
// (so it can subscribe to the viewport transform) but still portals to the body
// (so its stacking context is unchanged), and recomputes its anchor every render
// from the live transform instead of a stale open-time DOM rect. The band width
// and grace distance stay screen-space constants, so the ring reads the same at
// any zoom.

import { useStore } from "@xyflow/react";
import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { dispatch } from "../engine/session";
import { IconBypass, IconDive, IconEye, IconRename, IconTrash } from "../icons";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { useRadial, type RadialTarget } from "../store/radial";
import { useUi } from "../store/ui";
import { radialAnchor, type RadialAnchor } from "./radialAnchor";
import { hasVisibleParam, nodeVisible } from "./visibility";

/** Band width (D-7: outer 96 minus inner 58). */
const RING_WIDTH = 38;
/** The D-7 inner radius; clears a 112x28 body's half-diagonal at zoom 1.
 * Zoomed-in nodes grow, so the live inner radius is the larger of this
 * and the measured node clearance. */
const MIN_INNER_R = 58;
/** 45deg pitch minus 39deg sweep leaves the 6deg wedge gaps. */
const GAP_DEG = 6;
const GRACE_PX = 44;

interface Segment {
  key: string;
  /** Arc center angle, degrees; 0 = right, counterclockwise positive. */
  angle: number;
  span: number;
  icon: React.ReactNode;
  title: string;
  active?: boolean;
  disabled?: boolean;
  onPick: (t: RadialTarget) => void;
}

function polar(cx: number, cy: number, r: number, deg: number): [number, number] {
  const rad = (deg * Math.PI) / 180;
  return [cx + r * Math.cos(rad), cy - r * Math.sin(rad)];
}

/** One arc-band segment path (inner radius r0, outer r1). */
function arcPath(cx: number, cy: number, r0: number, r1: number, a0: number, a1: number): string {
  const [x0o, y0o] = polar(cx, cy, r1, a0);
  const [x1o, y1o] = polar(cx, cy, r1, a1);
  const [x0i, y0i] = polar(cx, cy, r0, a0);
  const [x1i, y1i] = polar(cx, cy, r0, a1);
  const large = Math.abs(a1 - a0) > 180 ? 1 : 0;
  return [
    `M ${x0o} ${y0o}`,
    `A ${r1} ${r1} 0 ${large} 0 ${x1o} ${y1o}`,
    `L ${x1i} ${y1i}`,
    `A ${r0} ${r0} 0 ${large} 1 ${x0i} ${y0i}`,
    "Z",
  ].join(" ");
}

/** The live screen anchor of the radial's node, or null if the node is gone
 * (deleted while the ring was open) or has not been measured yet. */
function useLiveAnchor(target: RadialTarget | null): RadialAnchor | null {
  // Subscribing to the transform is what makes the ring follow pan and zoom:
  // xyflow re-renders this component on every viewport change, and we re-measure
  // the node below. The value itself is only a render trigger.
  //
  // The selector MUST return a stable reference: `useStore` is backed by
  // useSyncExternalStore and compares snapshots with Object.is, so a selector
  // that builds a fresh object each call never settles and React spins the
  // render loop until the tab locks up. (It did.) A tuple field is stable.
  useStore((s) => s.transform);

  if (!target) return null;
  // Measure the node's live screen box. Deliberately the DOM rather than
  // xyflow's `measured`/`positionAbsolute`: those are internal bookkeeping and
  // are legitimately empty before a node has been measured (observed: a node
  // rendered at 48px on screen while `measured` was still `{}`), whereas the
  // rect is always the truth and already has pan and zoom baked in.
  const el = document.querySelector(`.react-flow__node[data-id="${target.nodeId}"]`);
  if (!el) return null;
  const rect = el.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return null;
  return radialAnchor(rect);
}

export function RadialMenu() {
  const target = useRadial((s) => s.target);
  const closeRadial = useRadial((s) => s.closeRadial);
  const openInfo = useRadial((s) => s.openInfo);
  const registry = useMirror((s) => s.registry);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const anchor = useLiveAnchor(target);

  useEffect(() => {
    if (!target) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRadial();
    };
    const onMove = (e: MouseEvent) => {
      // Measured against the ring's LIVE centre (it moves under the cursor
      // during a zoom), not a position captured when the ring opened.
      const a = anchorRef.current;
      if (!a) return;
      const dx = e.clientX - a.cx;
      const dy = e.clientY - a.cy;
      if (Math.hypot(dx, dy) > a.radius + 6 + RING_WIDTH + GRACE_PX) closeRadial();
    };
    const onDown = (e: PointerEvent) => {
      // Any press outside the ring (a node drag, marquee, connect) closes.
      if (!(e.target instanceof Element) || !e.target.closest(".radial-anchor")) closeRadial();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("pointerdown", onDown, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("pointerdown", onDown, true);
    };
  }, [target, closeRadial]);

  // The stray-close listener is registered once per target but must read the
  // CURRENT anchor, so it goes through a ref rather than a stale closure.
  const anchorRef = useRef<RadialAnchor | null>(anchor);
  anchorRef.current = anchor;

  if (!target || !anchor) return null;
  const t = target;

  // Live node state (Phase 10): bypass and display flags come from the mirror,
  // so the ring reflects the node as it is now, not as it was when the ring
  // opened.
  const node = graph.nodes.find((n) => n.id === t.nodeId);
  if (!node) return null;
  const desc = descriptorFor(registry, node.typeId);
  const isDisplay = graph.activeOutput === node.id;

  // The eye segment is context-dependent (Phase 8): in a subflow it is the
  // display flag (a radio selecting the container's output); at root it
  // toggles the node's `visible` param (additive per-node visibility),
  // gated on the descriptor declaring one (note gets no eye).
  const rootEye = t.ctx === "root" && hasVisibleParam(desc);

  // D-7 wedge layout: E rename, NE display, NW dive, W info, SW bypass,
  // SE delete; N and S open for the wires. Every wedge spans 45 minus the
  // shared gap.
  const segments: Segment[] = [
    {
      key: "rename",
      angle: 0,
      span: 45,
      icon: <IconRename size={13} />,
      title: "Rename (F2)",
      onPick: (tt) => {
        useUi.getState().setRenameRequest(tt.nodeId);
        closeRadial();
      },
    },
    {
      key: "info",
      angle: 180,
      span: 45,
      icon: <span className="radial-glyph">i</span>,
      title: "Node info",
      onPick: (tt) =>
        openInfo(
          tt.nodeId,
          tt.ctx,
          anchor.cx + anchor.radius + RING_WIDTH + 24,
          anchor.cy - 40,
        ),
    },
  ];
  if (t.ctx !== "root") {
    segments.push({
      key: "display",
      angle: 45,
      span: 45,
      icon: <IconEye size={13} />,
      title: "Set the display flag",
      active: isDisplay,
      onPick: (tt) => {
        dispatch({ type: "setActiveOutput", ctx: tt.ctx, node: tt.nodeId });
        closeRadial();
      },
    });
  } else if (rootEye) {
    segments.push({
      key: "visibility",
      angle: 45,
      span: 45,
      icon: <IconEye size={13} />,
      title: nodeVisible(node) ? "Hide (stays cooked)" : "Show",
      active: nodeVisible(node),
      onPick: (tt) => {
        dispatch({
          type: "setParam",
          ctx: tt.ctx,
          node: tt.nodeId,
          key: "visible",
          value: { kind: "literal", type: "bool", value: !nodeVisible(node) },
        });
        closeRadial();
      },
    });
  }
  segments.push(
    {
      key: "bypass",
      angle: 225,
      span: 45,
      icon: <IconBypass size={13} />,
      title: t.bypassable ? "Toggle bypass" : "Not bypassable",
      active: node.bypassed,
      disabled: !t.bypassable,
      onPick: (tt) => {
        if (tt.bypassable) {
          dispatch({
            type: "setBypass",
            ctx: tt.ctx,
            node: tt.nodeId,
            bypassed: !node.bypassed,
          });
        }
        closeRadial();
      },
    },
    {
      key: "delete",
      angle: 315,
      span: 45,
      icon: <IconTrash size={13} />,
      title: "Delete node",
      onPick: (tt) => {
        dispatch({ type: "removeNodes", ctx: tt.ctx, ids: [tt.nodeId] });
        closeRadial();
      },
    },
  );
  if (t.isContainer) {
    segments.push({
      key: "dive",
      angle: 135,
      span: 45,
      icon: <IconDive size={13} />,
      title: "Enter subflow",
      onPick: (tt) => {
        useMirror.getState().setCurrent({ subflow: tt.nodeId });
        closeRadial();
      },
    });
  }

  const r0 = Math.max(MIN_INNER_R, anchor.radius + 6);
  const r1 = r0 + RING_WIDTH;
  const size = (r1 + 8) * 2;
  const c = size / 2;

  return createPortal(
    <div
      className="radial-anchor"
      style={{ left: anchor.cx - c, top: anchor.cy - c, width: size, height: size }}
    >
      <svg width={size} height={size}>
        {segments.map((seg) => {
          const a0 = seg.angle - seg.span / 2 + GAP_DEG / 2;
          const a1 = seg.angle + seg.span / 2 - GAP_DEG / 2;
          return (
            <path
              key={seg.key}
              className={`radial-seg${seg.active ? " active" : ""}${seg.disabled ? " disabled" : ""}`}
              d={arcPath(c, c, r0, r1, a0, a1)}
              onClick={() => seg.onPick(t)}
            >
              <title>{seg.title}</title>
            </path>
          );
        })}
      </svg>
      {segments.map((seg) => {
        const [x, y] = polar(c, c, (r0 + r1) / 2, seg.angle);
        return (
          <span
            key={seg.key}
            className={`radial-icon${seg.active ? " active" : ""}${seg.disabled ? " disabled" : ""}`}
            style={{ left: x, top: y }}
            onClick={() => seg.onPick(t)}
            title={seg.title}
          >
            {seg.icon}
          </span>
        );
      })}
    </div>,
    document.body,
  );
}
