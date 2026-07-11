import { describe, expect, it } from "vitest";

import type { NodeMirror, NodeTypeSnapshot, ParamSource } from "../engine/types";
import { nodeLabel } from "./nodeLabel";

const DESC = { displayName: "Box" } as NodeTypeSnapshot;

function node(params: Record<string, ParamSource> = {}): NodeMirror {
  return {
    id: 1,
    typeId: "box",
    typeVersion: 2,
    params,
    position: [0, 0],
    bypassed: false,
  };
}

const text = (value: string): ParamSource => ({ kind: "literal", type: "text", value });

describe("nodeLabel", () => {
  it("uses the name param when set and non-empty", () => {
    expect(nodeLabel(node({ name: text("Hero Crate") }), DESC)).toBe("Hero Crate");
  });

  it("falls back to the display name when name is absent", () => {
    expect(nodeLabel(node(), DESC)).toBe("Box");
  });

  it("falls back when name is empty or whitespace", () => {
    expect(nodeLabel(node({ name: text("") }), DESC)).toBe("Box");
    expect(nodeLabel(node({ name: text("   ") }), DESC)).toBe("Box");
  });

  it("falls back for an expression-valued name", () => {
    expect(nodeLabel(node({ name: { kind: "expression", expr: "..." } }), DESC)).toBe("Box");
  });

  it("falls back to the type id without a descriptor", () => {
    expect(nodeLabel(node(), undefined)).toBe("box");
  });
});
