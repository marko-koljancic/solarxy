// The re-seed reconciliation. The defect it exists for: a wholesale rebuild
// dropped the measured size the canvas library writes onto its node objects,
// so a node dragged right after any graph edit had no dimensions and the
// library warned it was not initialized.

import { describe, expect, it } from "vitest";
import { reconcileNodes, type SeededNode } from "./seedReconcile";

/** A live node as the canvas holds it: the seeded fields plus the library's
 * own bookkeeping, of which `measured` is the one the drag reads. */
interface LiveNode extends SeededNode {
  measured?: { width: number; height: number };
  dragging?: boolean;
}

const mirrorNode = (id: number) => ({ id, typeId: "box" });

function seeded(id: number, over: Partial<LiveNode> = {}): LiveNode {
  return {
    id: String(id),
    type: "solarxy",
    position: { x: id * 10, y: 0 },
    selected: false,
    data: { node: mirrorNode(id), isDisplay: false },
    ...over,
  };
}

function live(id: number, over: Partial<LiveNode> = {}): LiveNode {
  return { ...seeded(id), measured: { width: 180, height: 40 }, ...over };
}

describe("reconcileNodes", () => {
  it("returns the live array itself for an equivalent seed", () => {
    const a = live(1);
    const b = live(2);
    const prev = [a, b];
    const next = reconcileNodes(prev, [
      seeded(1, { data: a.data }),
      seeded(2, { data: b.data }),
    ]);
    expect(next).toBe(prev);
  });

  it("keeps the measured size underneath a seeded change", () => {
    const a = live(1);
    const next = reconcileNodes([a], [seeded(1, { selected: true, data: a.data })]);
    expect(next).not.toBe([a]);
    expect(next[0].measured).toEqual({ width: 180, height: 40 });
    expect(next[0].selected).toBe(true);
  });

  it("touches only the nodes the seed changed; the rest keep identity", () => {
    const a = live(1);
    const b = live(2);
    const next = reconcileNodes(
      [a, b],
      [seeded(1, { data: a.data }), seeded(2, { selected: true, data: b.data })],
    );
    expect(next[0]).toBe(a);
    expect(next[1]).not.toBe(b);
    expect(next[1].measured).toEqual(b.measured);
  });

  it("lets the seeded position overwrite the canvas's, since position is engine-owned", () => {
    const a = live(1, { position: { x: 999, y: 999 } });
    const next = reconcileNodes([a], [seeded(1, { data: a.data })]);
    expect(next[0].position).toEqual({ x: 10, y: 0 });
    expect(next[0].measured).toEqual(a.measured);
  });

  it("passes an added node through and drops a removed one", () => {
    const a = live(1);
    const next = reconcileNodes([a], [seeded(1, { data: a.data }), seeded(2)]);
    expect(next).toHaveLength(2);
    expect(next[0]).toBe(a);
    expect(next[1].measured).toBeUndefined();
    const after = reconcileNodes(next, [seeded(2, { data: next[1].data })]);
    expect(after).toHaveLength(1);
    expect(after[0].id).toBe("2");
  });

  it("follows the seed's order when nodes reorder", () => {
    const a = live(1);
    const b = live(2);
    const next = reconcileNodes(
      [a, b],
      [seeded(2, { data: b.data }), seeded(1, { data: a.data })],
    );
    expect(next.map((n) => n.id)).toEqual(["2", "1"]);
    expect(next[0]).toBe(b);
    expect(next[1]).toBe(a);
  });

  it("treats a new mirror node object as a change, by reference", () => {
    const a = live(1);
    const next = reconcileNodes([a], [seeded(1)]);
    expect(next[0]).not.toBe(a);
    expect(next[0].measured).toEqual(a.measured);
  });
});
