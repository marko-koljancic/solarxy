import { beforeEach, describe, expect, it } from "vitest";
import type { DocumentSnapshot, EventBatch, GraphMirror } from "../engine/types";
import { selectGraph, useMirror } from "./mirror";

function emptyGraph(): GraphMirror {
  return { nodes: [], edges: [], activeOutput: null, selection: [] };
}

function node(id: number, typeId = "box"): GraphMirror["nodes"][number] {
  return { id, typeId, typeVersion: 1, params: {}, position: [0, 0], bypassed: false };
}

beforeEach(() => {
  useMirror.setState({ revision: 0, contexts: { root: emptyGraph() }, cook: {}, cookMode: "auto" });
});

describe("mirror store", () => {
  it("applies granular events to build the mirror", () => {
    const s = useMirror.getState();
    const batch: EventBatch = {
      revision: 1,
      events: [
        { type: "nodeAdded", ctx: "root", node: node(1, "geo") },
        { type: "nodeAdded", ctx: "root", node: node(2, "point_light") },
        {
          type: "edgeAdded",
          ctx: "root",
          edge: { id: 10, from: 1, fromPort: "geometry", to: 2, toPort: "geometry" },
        },
        { type: "paramChanged", ctx: "root", node: 1, key: "width", value: { kind: "literal", type: "float", value: 3 } },
        { type: "activeOutputChanged", ctx: "root", node: 1 },
      ],
    };
    expect(s.applyBatch(batch)).toBe(false);
    const g = selectGraph(useMirror.getState(), "root");
    expect(g.nodes.map((n) => n.id)).toEqual([1, 2]);
    expect(g.edges).toHaveLength(1);
    expect(g.activeOutput).toBe(1);
    expect(g.nodes[0].params["width"]).toEqual({ kind: "literal", type: "float", value: 3 });
    expect(useMirror.getState().revision).toBe(1);
  });

  it("cook events update the cook map without a revision bump", () => {
    const s = useMirror.getState();
    s.applyBatch({ revision: 1, events: [{ type: "nodeAdded", ctx: "root", node: node(1) }] });
    // A cook batch carries the SAME revision (cook does not bump it).
    const needs = useMirror.getState().applyBatch({
      revision: 1,
      events: [
        { type: "cookStatus", node: 1, status: { state: "ok", ms: 0.4 } },
        { type: "nodeStats", node: 1, points: 24, prims: 12, meshes: 1 },
      ],
    });
    expect(needs).toBe(false);
    expect(useMirror.getState().cook[1]).toEqual({ status: { state: "ok", ms: 0.4 }, points: 24, prims: 12, meshes: 1 });
  });

  it("requests a resnapshot on a revision gap and skips the events", () => {
    const s = useMirror.getState();
    s.applyBatch({ revision: 1, events: [{ type: "nodeAdded", ctx: "root", node: node(1) }] });
    // Jump from revision 1 to 5: three batches were missed.
    const needs = useMirror.getState().applyBatch({
      revision: 5,
      events: [{ type: "nodeAdded", ctx: "root", node: node(99) }],
    });
    expect(needs).toBe(true);
    // The gapped batch's events are NOT applied (a resnapshot will follow).
    expect(selectGraph(useMirror.getState(), "root").nodes.map((n) => n.id)).toEqual([1]);
  });

  it("requests a resnapshot on documentReplaced", () => {
    const needs = useMirror.getState().applyBatch({
      revision: 2,
      events: [{ type: "documentReplaced" }],
    });
    expect(needs).toBe(true);
  });

  it("rebuilds the whole mirror from a snapshot, including subflows", () => {
    const snap: DocumentSnapshot = {
      root: { nodes: [node(1, "geo")], edges: [], activeOutput: null, selection: [] },
      subflows: { "1": { nodes: [node(2, "box")], edges: [], activeOutput: 2, selection: [] } },
      annotations: [],
    };
    useMirror.getState().replaceFromSnapshot(snap, 7);
    const st = useMirror.getState();
    expect(st.revision).toBe(7);
    expect(selectGraph(st, "root").nodes[0].typeId).toBe("geo");
    expect(selectGraph(st, { subflow: 1 }).activeOutput).toBe(2);
  });
});
