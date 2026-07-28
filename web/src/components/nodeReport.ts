// Presentation for the node info card's pull-read report, plus the
// connection summary it derives entirely from the mirror.
//
// Split out from the card so the formatting rules are testable: "unknown"
// must never become a 1970 date, a microsecond figure must not read as
// "0.0 ms", and a bounds box has to survive a degenerate (single-point)
// input.

import type { GraphMirror, NodeMirror, NodeReport, NodeTypeSnapshot } from "../engine/types";

/** A duration in microseconds, at a scale a human reads.
 *
 * Microseconds below a millisecond because that is where most nodes live
 * and `0.0 ms` says nothing; seconds above a thousand milliseconds because
 * `4200.0 ms` is worse than `4.2 s`. */
export function formatDuration(us: number): string {
  if (!Number.isFinite(us) || us < 0) return "unknown";
  if (us === 0) return "0";
  if (us < 1000) return `${Math.round(us)} us`;
  const ms = us / 1000;
  if (ms < 1000) return `${ms.toFixed(ms < 10 ? 2 : 1)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** A Unix-millisecond stamp as an absolute local time, or `"unknown"`.
 *
 * Null is a real answer here (a scene saved before 0.8.1, or a host with no
 * epoch clock) and must stay visibly unknown: rendering it as a date would
 * claim every node in an old scene was created in January 1970. */
export function formatTimestamp(ms: number | null, now: number): string {
  if (ms === null || !Number.isFinite(ms)) return "unknown";
  const then = new Date(ms);
  const absolute = then.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  const rel = relativeTime(ms, now);
  return rel ? `${absolute} (${rel})` : absolute;
}

/** A short "5 minutes ago" for a recent stamp, or "" when far enough back
 * that the absolute date carries it on its own. */
export function relativeTime(ms: number, now: number): string {
  const delta = now - ms;
  if (!Number.isFinite(delta) || delta < 0) return "";
  const secs = Math.floor(delta / 1000);
  if (secs < 10) return "just now";
  if (secs < 60) return `${secs} seconds ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins} minute${mins === 1 ? "" : "s"} ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days} day${days === 1 ? "" : "s"} ago`;
  return "";
}

/** A bounds box as size then centre, which is what someone actually wants
 * to know; the raw min/max pair is available on hover via the title. */
export function formatBounds(b: NodeReport["bounds"]): string | null {
  if (!b) return null;
  const [minX, minY, minZ, maxX, maxY, maxZ] = b;
  if (![minX, minY, minZ, maxX, maxY, maxZ].every(Number.isFinite)) return null;
  const size = [maxX - minX, maxY - minY, maxZ - minZ];
  const centre = [(minX + maxX) / 2, (minY + maxY) / 2, (minZ + maxZ) / 2];
  const n = (v: number) => (Math.abs(v) < 1e-4 ? "0" : v.toFixed(3).replace(/\.?0+$/, ""));
  return `${size.map(n).join(" x ")} at ${centre.map(n).join(", ")}`;
}

/** One side of the connection summary. */
export interface ConnectionSummary {
  /** `port -> the node names feeding it`, in edge order. */
  inputs: { port: string; from: string[] }[];
  /** `port -> the node names it feeds`. */
  outputs: { port: string; to: string[] }[];
  /** How many nodes this one feeds, directly. */
  downstream: number;
  /** How many feed it, directly. */
  upstream: number;
}

/** Who is wired to this node, by name.
 *
 * Derived entirely from the mirror the canvas already holds: no engine
 * change, and it stays correct through undo for free. */
export function connectionSummary(
  graph: GraphMirror,
  node: NodeMirror,
  label: (n: NodeMirror) => string,
): ConnectionSummary {
  const byId = new Map(graph.nodes.map((n) => [n.id, n]));
  const name = (id: number) => {
    const n = byId.get(id);
    return n ? label(n) : `node ${id}`;
  };

  const inputs = new Map<string, string[]>();
  const outputs = new Map<string, string[]>();
  const upstreamIds = new Set<number>();
  const downstreamIds = new Set<number>();

  for (const e of graph.edges) {
    if (e.to === node.id) {
      const list = inputs.get(e.toPort) ?? [];
      list.push(name(e.from));
      inputs.set(e.toPort, list);
      upstreamIds.add(e.from);
    }
    if (e.from === node.id) {
      const list = outputs.get(e.fromPort) ?? [];
      list.push(name(e.to));
      outputs.set(e.fromPort, list);
      downstreamIds.add(e.to);
    }
  }

  return {
    inputs: [...inputs].map(([port, from]) => ({ port, from })),
    outputs: [...outputs].map(([port, to]) => ({ port, to })),
    upstream: upstreamIds.size,
    downstream: downstreamIds.size,
  };
}

/** The node's path, the form `ch()` expressions and Copy Node Path use. */
export function describeKind(desc: NodeTypeSnapshot | undefined): string | null {
  if (!desc) return null;
  const bits = [desc.categoryLabel];
  if (desc.contexts.length > 0) bits.push(`in ${desc.contexts.join(", ")}`);
  if (desc.opens) bits.push(`opens a ${desc.opens} network`);
  return bits.join(" · ");
}
