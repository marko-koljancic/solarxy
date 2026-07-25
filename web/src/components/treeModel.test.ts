import { describe, expect, it } from "vitest";
import type { GraphMirror, NodeMirror, RegistrySnapshot } from "../engine/types";
import { allBranchKeys, buildSceneTree, searchTree } from "./treeModel";

function node(id: number, typeId: string, name?: string): NodeMirror {
  return {
    id,
    typeId,
    typeVersion: 1,
    params: name
      ? { name: { kind: "literal", type: "text", value: name } }
      : {},
    position: [0, 0],
    bypassed: false,
  };
}

function graph(nodes: NodeMirror[], activeOutput: number | null = null): GraphMirror {
  return { nodes, edges: [], activeOutput, selection: [] };
}

/** A minimal registry: only the fields the tree consumes (opens, names). */
const REGISTRY = {
  nodes: [
    { typeId: "geo", displayName: "Geo", opens: "geo" },
    { typeId: "texnet", displayName: "Texture Network", opens: "tex" },
    { typeId: "box", displayName: "Box", opens: null },
    { typeId: "note", displayName: "Note", opens: null },
  ],
} as unknown as RegistrySnapshot;

const CONTEXTS: Record<string, GraphMirror> = {
  root: graph([node(1, "geo", "terrain"), node(2, "texnet", "maps"), node(3, "note")]),
  "sub:1": graph([node(4, "box"), node(5, "texnet", "inner")], 4),
  "sub:5": graph([node(6, "box", "deep")]),
  // An orphaned context (its owner was deleted): never reachable root-down.
  "sub:99": graph([node(7, "box")]),
};

describe("buildSceneTree", () => {
  it("builds root-down, preserving mirror order, with nested containers", () => {
    const rows = buildSceneTree(REGISTRY, CONTEXTS);
    expect(rows.map((r) => r.label)).toEqual(["terrain", "maps", "Note"]);
    expect(rows[0].children.map((r) => r.typeId)).toEqual(["box", "texnet"]);
    expect(rows[0].children[1].children.map((r) => r.label)).toEqual(["deep"]);
    expect(rows[0].depth).toBe(0);
    expect(rows[0].children[0].depth).toBe(1);
    // The orphaned sub:99 never appears.
    const all: string[] = [];
    const collect = (rs: typeof rows) =>
      rs.forEach((r) => {
        all.push(r.key);
        collect(r.children);
      });
    collect(rows);
    expect(all.some((k) => k.endsWith(":7"))).toBe(false);
  });

  it("marks containers, leaves, and the display flag", () => {
    const rows = buildSceneTree(REGISTRY, CONTEXTS);
    expect(rows[0].opens).toBe("geo");
    expect(rows[2].opens).toBeNull();
    const sub = rows[0].children;
    expect(sub[0].isDisplay).toBe(true);
    expect(sub[1].isDisplay).toBe(false);
    // A row's ctx is the context it LIVES in.
    expect(sub[0].ctx).toEqual({ subflow: 1 });
  });

  it("tolerates a container whose sub-context is not mirrored", () => {
    const rows = buildSceneTree(REGISTRY, {
      root: graph([node(1, "geo", "hollow")]),
    });
    expect(rows[0].opens).toBe("geo");
    expect(rows[0].children).toEqual([]);
  });

  it("terminates on a cyclic contexts map", () => {
    // Malformed: the container's subtree contains a node with its own id.
    const rows = buildSceneTree(REGISTRY, {
      root: graph([node(1, "geo")]),
      "sub:1": graph([node(1, "geo")]),
    });
    expect(rows.length).toBe(1);
    // The guard cuts the recursion rather than hanging; depth is capped.
    let depth = 0;
    let cursor = rows;
    while (cursor.length > 0) {
      depth += 1;
      cursor = cursor[0].children;
    }
    expect(depth).toBeLessThanOrEqual(64);
  });

  it("returns empty for an empty or missing root", () => {
    expect(buildSceneTree(REGISTRY, {})).toEqual([]);
    expect(buildSceneTree(null, { root: graph([node(1, "box")]) }).length).toBe(1);
  });
});

describe("searchTree", () => {
  const rows = buildSceneTree(REGISTRY, CONTEXTS);

  it("matches case-insensitively over label and type id", () => {
    const { matches } = searchTree(rows, "DEEP");
    expect([...matches]).toEqual(["sub:5:6"]);
    const byType = searchTree(rows, "texnet");
    expect(byType.matches.size).toBe(2);
  });

  it("returns the ancestor chain to expand", () => {
    const { expand } = searchTree(rows, "deep");
    expect(expand).toEqual(new Set(["root:1", "sub:1:5"]));
  });

  it("an empty query returns empty sets", () => {
    const { matches, expand } = searchTree(rows, "   ");
    expect(matches.size).toBe(0);
    expect(expand.size).toBe(0);
  });
});

describe("allBranchKeys", () => {
  it("collects only rows with children, at every depth", () => {
    const rows = buildSceneTree(REGISTRY, CONTEXTS);
    const keys = allBranchKeys(rows);
    // terrain (root container) and the nested inner texnet are branches;
    // maps has no mirrored sub-context, so it is a leaf, as are box/note.
    expect(keys).toEqual(new Set(["root:1", "sub:1:5"]));
  });

  it("is empty for an empty tree", () => {
    expect(allBranchKeys([]).size).toBe(0);
  });
});
