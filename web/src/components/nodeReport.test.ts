// The node info card's presentation rules. The load-bearing one is that
// "unknown" survives: a scene saved before 0.8.1 carries no timestamps, and
// rendering those as dates would tell the user every node in it was created
// in January 1970.

import { describe, expect, it } from "vitest";
import type { GraphMirror, NodeMirror } from "../engine/types";
import {
  connectionSummary,
  formatBounds,
  formatDuration,
  formatTimestamp,
  relativeTime,
} from "./nodeReport";

describe("formatDuration", () => {
  it("keeps sub-millisecond cooks in microseconds", () => {
    // The whole reason this exists: the badge says "0.0 ms" for these.
    expect(formatDuration(340)).toBe("340 us");
    expect(formatDuration(999)).toBe("999 us");
  });

  it("switches to milliseconds and then seconds as the scale grows", () => {
    expect(formatDuration(1500)).toBe("1.50 ms");
    expect(formatDuration(42_000)).toBe("42.0 ms");
    expect(formatDuration(4_200_000)).toBe("4.20 s");
  });

  it("refuses to invent a figure for junk", () => {
    expect(formatDuration(Number.NaN)).toBe("unknown");
    expect(formatDuration(-1)).toBe("unknown");
    expect(formatDuration(0)).toBe("0");
  });
});

describe("formatTimestamp", () => {
  const now = Date.UTC(2026, 6, 28, 12, 0, 0);

  it("says unknown for a scene that predates timestamps", () => {
    // NOT "1 Jan 1970". A null stamp is a real answer and must read as one.
    expect(formatTimestamp(null, now)).toBe("unknown");
  });

  it("says unknown rather than rendering a non-finite stamp", () => {
    expect(formatTimestamp(Number.NaN, now)).toBe("unknown");
  });

  it("renders a real stamp with a relative hint", () => {
    const out = formatTimestamp(now - 5 * 60_000, now);
    expect(out).toContain("5 minutes ago");
    expect(out).not.toBe("unknown");
  });

  it("drops the relative hint once the absolute date carries it", () => {
    const out = formatTimestamp(now - 200 * 24 * 3600_000, now);
    expect(out).not.toContain("ago");
  });
});

describe("relativeTime", () => {
  const now = 1_000_000_000;

  it("scales through the units and singularizes", () => {
    expect(relativeTime(now - 3000, now)).toBe("just now");
    expect(relativeTime(now - 30_000, now)).toBe("30 seconds ago");
    expect(relativeTime(now - 60_000, now)).toBe("1 minute ago");
    expect(relativeTime(now - 7_200_000, now)).toBe("2 hours ago");
    expect(relativeTime(now - 2 * 86_400_000, now)).toBe("2 days ago");
  });

  it("says nothing for a stamp in the future", () => {
    // A system-clock change can produce one; it must not read as
    // "-3 seconds ago".
    expect(relativeTime(now + 5000, now)).toBe("");
  });
});

describe("formatBounds", () => {
  it("reports size then centre", () => {
    expect(formatBounds([-1, -1, -1, 1, 1, 1])).toBe("2 x 2 x 2 at 0, 0, 0");
  });

  it("handles a degenerate box (a single point) without producing NaN", () => {
    expect(formatBounds([3, 4, 5, 3, 4, 5])).toBe("0 x 0 x 0 at 3, 4, 5");
  });

  it("returns null when there are no bounds to show", () => {
    expect(formatBounds(null)).toBeNull();
    expect(formatBounds([0, 0, 0, Number.NaN, 1, 1])).toBeNull();
  });
});

describe("connectionSummary", () => {
  const node = (id: number): NodeMirror =>
    ({ id, typeId: "box", params: {}, position: [0, 0], bypassed: false }) as unknown as NodeMirror;

  const graph: GraphMirror = {
    nodes: [node(1), node(2), node(3), node(4)],
    edges: [
      { id: 10, from: 1, fromPort: "geometry", to: 3, toPort: "geometry" },
      { id: 11, from: 2, fromPort: "geometry", to: 3, toPort: "geometry" },
      { id: 12, from: 3, fromPort: "geometry", to: 4, toPort: "geometry" },
    ],
    activeOutput: null,
    selection: [],
  };
  const label = (n: NodeMirror) => `n${n.id}`;

  it("groups sources by the port they feed, in edge order", () => {
    const s = connectionSummary(graph, node(3), label);
    expect(s.inputs).toEqual([{ port: "geometry", from: ["n1", "n2"] }]);
  });

  it("counts distinct neighbours, not edges", () => {
    const s = connectionSummary(graph, node(3), label);
    expect(s.upstream).toBe(2);
    expect(s.downstream).toBe(1);
  });

  it("reports an isolated node as empty rather than throwing", () => {
    const s = connectionSummary(
      { ...graph, edges: [] },
      node(3),
      label,
    );
    expect(s.inputs).toEqual([]);
    expect(s.outputs).toEqual([]);
    expect(s.upstream).toBe(0);
  });

  it("names a source that is no longer in the graph rather than crashing", () => {
    // A mirror mid-update can carry an edge whose endpoint has gone.
    const s = connectionSummary(
      { ...graph, nodes: [node(3)] },
      node(3),
      label,
    );
    expect(s.inputs[0].from).toEqual(["node 1", "node 2"]);
  });
});
