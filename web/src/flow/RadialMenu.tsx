// The Houdini-style hover radial (Phase 7b, maintainer reference
// screenshot): dark arc segments with gaps surrounding the node. Segments:
// info (i) left, display-flag eye top-right, enter-subflow top-left
// (containers only), bypass bottom-left, delete bottom-right. Rendered in
// a body portal at the node's viewport position; the container is
// pointer-transparent so the node underneath stays fully interactive, and
// the ring closes when the pointer strays past a grace radius, on any
// outside pointerdown (drag/marquee/connect starts), or on Esc.

import { useEffect } from "react";
import { createPortal } from "react-dom";
import { dispatch } from "../engine/session";
import { IconBypass, IconDive, IconEye, IconTrash } from "../icons";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { useRadial, type RadialTarget } from "../store/radial";
import { hasVisibleParam, nodeVisible } from "./visibility";

const RING_WIDTH = 34;
const GAP_DEG = 9;
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

export function RadialMenu() {
  const target = useRadial((s) => s.target);
  const closeRadial = useRadial((s) => s.closeRadial);
  const openInfo = useRadial((s) => s.openInfo);
  const registry = useMirror((s) => s.registry);
  const rootGraph = useMirror((s) => selectGraph(s, "root"));

  useEffect(() => {
    if (!target) return;
    const r1 = target.radius + 6 + RING_WIDTH;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRadial();
    };
    const onMove = (e: MouseEvent) => {
      const dx = e.clientX - target.cx;
      const dy = e.clientY - target.cy;
      if (Math.hypot(dx, dy) > r1 + GRACE_PX) closeRadial();
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

  if (!target) return null;
  const t = target;

  // The eye segment is context-dependent (Phase 8): in a subflow it is the
  // display flag (a radio selecting the container's output); at root it
  // toggles the node's `visible` param (additive per-node visibility),
  // gated on the descriptor declaring one (note gets no eye).
  const rootNode = t.ctx === "root" ? rootGraph.nodes.find((n) => n.id === t.nodeId) : undefined;
  const rootEye = rootNode !== undefined && hasVisibleParam(descriptorFor(registry, rootNode.typeId));

  const segments: Segment[] = [
    {
      key: "info",
      angle: 180,
      span: 62,
      icon: <span className="radial-glyph">i</span>,
      title: "Node info",
      onPick: (tt) => openInfo(tt.nodeId, tt.ctx, tt.cx + tt.radius + RING_WIDTH + 24, tt.cy - 40),
    },
  ];
  if (t.ctx !== "root") {
    segments.push({
      key: "display",
      angle: 38,
      span: 62,
      icon: <IconEye size={13} />,
      title: "Set the display flag",
      active: t.isDisplay,
      onPick: (tt) => {
        dispatch({ type: "setActiveOutput", ctx: tt.ctx, node: tt.nodeId });
        closeRadial();
      },
    });
  } else if (rootEye && rootNode) {
    segments.push({
      key: "visibility",
      angle: 38,
      span: 62,
      icon: <IconEye size={13} />,
      title: nodeVisible(rootNode) ? "Hide (stays cooked)" : "Show",
      active: nodeVisible(rootNode),
      onPick: (tt) => {
        dispatch({
          type: "setParam",
          ctx: tt.ctx,
          node: tt.nodeId,
          key: "visible",
          value: { kind: "literal", type: "bool", value: !nodeVisible(rootNode) },
        });
        closeRadial();
      },
    });
  }
  segments.push(
    {
      key: "bypass",
      angle: 232,
      span: 62,
      icon: <IconBypass size={13} />,
      title: t.bypassable ? "Toggle bypass" : "Not bypassable",
      active: t.bypassed,
      disabled: !t.bypassable,
      onPick: (tt) => {
        if (tt.bypassable) {
          dispatch({ type: "setBypass", ctx: tt.ctx, node: tt.nodeId, bypassed: !tt.bypassed });
        }
        closeRadial();
      },
    },
    {
      key: "delete",
      angle: 308,
      span: 62,
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
      angle: 142,
      span: 42,
      icon: <IconDive size={13} />,
      title: "Enter subflow",
      onPick: (tt) => {
        useMirror.getState().setCurrent({ subflow: tt.nodeId });
        closeRadial();
      },
    });
  }

  const r0 = t.radius + 6;
  const r1 = r0 + RING_WIDTH;
  const size = (r1 + 8) * 2;
  const c = size / 2;

  return createPortal(
    <div
      className="radial-anchor"
      style={{ left: t.cx - c, top: t.cy - c, width: size, height: size }}
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
            className={`radial-icon${seg.disabled ? " disabled" : ""}`}
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
