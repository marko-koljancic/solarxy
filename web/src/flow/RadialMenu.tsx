// The Houdini-style hover radial: six
// wedges on a 60-degree pitch with 54-degree sweeps closing a full,
// symmetric circle, centred at E rename / NE display or visibility /
// NW dive / W info / SW bypass / SE delete. Conditional actions render
// disabled rather than absent so the circle stays complete; the enlarged
// inner radius keeps the ring clear of the node's side wings and lets the
// wires stay readable through the wedge gaps. Active segments fill accent
// amber. Rendered in a body portal at the node's viewport position; the
// container is pointer-transparent so the node underneath stays fully
// interactive, and the ring closes when the pointer strays past a grace
// radius, on any outside pointerdown (drag/marquee/connect starts), or
// on Esc.
//
// The ring TRACKS its node. It renders inside the ReactFlowProvider
// (so it can subscribe to the viewport transform) but still portals to the body
// (so its stacking context is unchanged), and recomputes its anchor every render
// from the live transform instead of a stale open-time DOM rect. The band width
// and grace distance stay screen-space constants, so the ring reads the same at
// any zoom.

import { useStore } from "@xyflow/react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { IconBypass, IconDisplay, IconDive, IconRename, IconTrash, IconVisibility } from "../icons";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { useRadial, type RadialTarget } from "../store/radial";
import {
  diveIntoSubflow,
  removeNode,
  requestRename,
  setDisplayFlag,
  toggleBypass,
  toggleVisibility,
} from "./nodeActions";
import { radialAnchor, type RadialAnchor } from "./radialAnchor";
import { hasVisibleParam, nodeVisible } from "./visibility";

/** Band width. */
const RING_WIDTH = 38;
/** The inner radius: clears a 112x32 body's half-width (56) with
 * air, so the ring never sits on the side wings. Zoomed-in nodes grow,
 * so the live inner radius is the larger of this and the measured node
 * clearance. */
const MIN_INNER_R = 72;
/** Clearance added to the measured node radius before the ring starts. */
const INNER_CLEARANCE = 10;
/** 60deg pitch minus 54deg sweep leaves the 6deg wedge gaps. */
const GAP_DEG = 6;
const GRACE_PX = 44;

/** The live inner radius for a measured node radius (shared by the ring
 * geometry and the stray-close distance so they cannot drift apart). */
function innerRadius(nodeRadius: number): number {
  return Math.max(MIN_INNER_R, nodeRadius + INNER_CLEARANCE);
}

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
  // Which wedge the pointer is over. React-side because the SVG path and
  // its HTML icon are sibling elements: CSS :hover on one cannot restyle
  // the other, and the hovered action must read as one unit.
  const [hoverKey, setHoverKey] = useState<string | null>(null);

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
      if (Math.hypot(dx, dy) > innerRadius(a.radius) + RING_WIDTH + GRACE_PX) closeRadial();
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

  // Live node state: bypass and display flags come from the mirror,
  // so the ring reflects the node as it is now, not as it was when the ring
  // opened.
  const node = graph.nodes.find((n) => n.id === t.nodeId);
  if (!node) return null;
  const desc = descriptorFor(registry, node.typeId);
  const isDisplay = graph.activeOutput === node.id;

  const r0 = innerRadius(anchor.radius);
  const r1 = r0 + RING_WIDTH;
  const size = (r1 + 8) * 2;
  const c = size / 2;

  // The NE wedge is context-dependent: in a subflow it is the
  // display flag (a radio selecting the container's output); at root it
  // toggles the node's `visible` param (additive per-node visibility),
  // gated on the descriptor declaring one (note gets a disabled wedge).
  const rootEye = t.ctx === "root" && hasVisibleParam(desc);
  const neSegment: Segment =
    t.ctx !== "root"
      ? {
          key: "display",
          angle: 60,
          span: 60,
          icon: <IconDisplay size={13} />,
          title: "Set the display flag",
          active: isDisplay,
          onPick: (tt) => {
            setDisplayFlag(tt.ctx, tt.nodeId);
            closeRadial();
          },
        }
      : {
          key: "visibility",
          angle: 60,
          span: 60,
          icon: <IconVisibility size={13} />,
          title: rootEye ? (nodeVisible(node) ? "Hide (stays cooked)" : "Show") : "No visibility toggle",
          active: rootEye && nodeVisible(node),
          disabled: !rootEye,
          onPick: (tt) => {
            if (rootEye) toggleVisibility(tt.ctx, node);
            closeRadial();
          },
        };

  // Wedge layout: six 60-degree wedges close the full circle, the
  // Mnemonics (E rename, NE display/visibility, NW dive, W info,
  // SW bypass, SE delete). Conditional actions render disabled rather
  // than absent so the ring stays symmetric.
  const segments: Segment[] = [
    {
      key: "rename",
      angle: 0,
      span: 60,
      icon: <IconRename size={13} />,
      title: "Rename (F2)",
      onPick: (tt) => {
        requestRename(tt.nodeId);
        closeRadial();
      },
    },
    neSegment,
    {
      key: "dive",
      angle: 120,
      span: 60,
      icon: <IconDive size={13} />,
      title: t.isContainer ? "Enter subflow" : "Not a container",
      disabled: !t.isContainer,
      onPick: (tt) => {
        if (tt.isContainer) diveIntoSubflow(tt.nodeId);
        closeRadial();
      },
    },
    {
      key: "info",
      angle: 180,
      span: 60,
      icon: <span className="radial-glyph">i</span>,
      title: "Node info",
      onPick: (tt) => openInfo(tt.nodeId, tt.ctx, anchor.cx + r1 + 24, anchor.cy - 40),
    },
    {
      key: "bypass",
      angle: 240,
      span: 60,
      icon: <IconBypass size={13} />,
      title: t.bypassable ? "Toggle bypass" : "Not bypassable",
      active: node.bypassed,
      disabled: !t.bypassable,
      onPick: (tt) => {
        if (tt.bypassable) toggleBypass(tt.ctx, node);
        closeRadial();
      },
    },
    {
      key: "delete",
      angle: 300,
      span: 60,
      icon: <IconTrash size={13} />,
      title: "Delete node",
      onPick: (tt) => {
        removeNode(tt.ctx, tt.nodeId);
        closeRadial();
      },
    },
  ];

  return createPortal(
    <div
      className="radial-anchor"
      style={{ left: anchor.cx - c, top: anchor.cy - c, width: size, height: size }}
    >
      <svg width={size} height={size}>
        {segments.map((seg) => {
          const a0 = seg.angle - seg.span / 2 + GAP_DEG / 2;
          const a1 = seg.angle + seg.span / 2 - GAP_DEG / 2;
          const hovered = !seg.disabled && seg.key === hoverKey;
          return (
            <path
              key={seg.key}
              className={`radial-seg seg-${seg.key}${seg.active ? " active" : ""}${seg.disabled ? " disabled" : ""}${hovered ? " hovered" : ""}`}
              d={arcPath(c, c, r0, r1, a0, a1)}
              onClick={() => seg.onPick(t)}
              onPointerEnter={() => setHoverKey(seg.disabled ? null : seg.key)}
              onPointerLeave={() => setHoverKey((k) => (k === seg.key ? null : k))}
            >
              <title>{seg.title}</title>
            </path>
          );
        })}
      </svg>
      {segments.map((seg) => {
        const [x, y] = polar(c, c, (r0 + r1) / 2, seg.angle);
        const hovered = !seg.disabled && seg.key === hoverKey;
        return (
          <span
            key={seg.key}
            className={`radial-icon seg-${seg.key}${seg.active ? " active" : ""}${seg.disabled ? " disabled" : ""}${hovered ? " hovered" : ""}`}
            style={{ left: x, top: y }}
            onClick={() => seg.onPick(t)}
            onPointerEnter={() => setHoverKey(seg.disabled ? null : seg.key)}
            onPointerLeave={() => setHoverKey((k) => (k === seg.key ? null : k))}
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
