// The pure layout mapping: dagre center-to-top-left conversion, layer
// ordering, and the note-node exclusion in layoutInputs.

import { describe, expect, it } from "vitest";
import type { GraphMirror } from "../engine/types";
import { computeDagreLayout, computeElkLayout, layoutInputs } from "./layout";

const DIMS = { width: 120, height: 60 };
const measure = () => DIMS;

function chainGraph(): GraphMirror {
  return {
    nodes: [
      { id: 1, typeId: "box", typeVersion: 1, params: {}, position: [0, 0], bypassed: false },
      { id: 2, typeId: "transform", typeVersion: 1, params: {}, position: [5, 5], bypassed: false },
      { id: 3, typeId: "note", typeVersion: 1, params: {}, position: [9, 9], bypassed: false },
    ],
    edges: [{ id: 10, from: 1, fromPort: "out", to: 2, toPort: "in" }],
    activeOutput: 2,
    selection: [],
  };
}

describe("layoutInputs", () => {
  it("excludes note nodes and their edges", () => {
    const { nodes, edges } = layoutInputs(chainGraph(), measure);
    expect(nodes.map((n) => n.id)).toEqual([1, 2]);
    expect(edges).toEqual([[1, 2]]);
  });
});

describe("computeDagreLayout", () => {
  it("stacks a chain top-to-bottom with ranksep spacing, top-left coords", () => {
    const moves = computeDagreLayout(
      [
        { id: 1, ...DIMS },
        { id: 2, ...DIMS },
      ],
      [[1, 2]],
    );
    const byId = new Map(moves);
    const a = byId.get(1);
    const b = byId.get(2);
    if (!a || !b) throw new Error("missing move");
    // Same column (TB flow), child exactly one rank below its parent.
    expect(a[0]).toBe(b[0]);
    expect(b[1] - a[1]).toBe(DIMS.height + 100);
    // Dagre centers converted to top-left: coordinates land on the origin
    // column, so x is never negative for a single-column chain.
    expect(a[0]).toBeGreaterThanOrEqual(0);
    expect(a[1]).toBeGreaterThanOrEqual(0);
  });
});

describe("computeElkLayout", () => {
  it("orders a chain downward and returns a move per node", async () => {
    const moves = await computeElkLayout(
      [
        { id: 1, ...DIMS },
        { id: 2, ...DIMS },
      ],
      [[1, 2]],
    );
    expect(moves).toHaveLength(2);
    const byId = new Map(moves);
    const a = byId.get(1);
    const b = byId.get(2);
    if (!a || !b) throw new Error("missing move");
    expect(b[1]).toBeGreaterThan(a[1]);
  });
});
