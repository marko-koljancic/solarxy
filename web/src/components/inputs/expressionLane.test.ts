// What is left of the expression lane's tests after the rules moved into
// `solarxy-studio`.
//
// Which types accept an expression, the seed text, the readout's rounding
// and the commit comparison are all asserted in Rust now, against the same
// cases these carried. What is left is the parked map, which is this
// shell's own session memory, and the one-line mirror read beside it.

import { beforeEach, describe, expect, it } from "vitest";
import {
  clearParkedExpressions,
  discardParkedExpression,
  parkExpression,
  parkedExpression,
  paramExpression,
} from "./expressionLane";
import type { NodeMirror, ParamSnapshot } from "../../engine/types";

function spec(key: string, paramType = "float"): ParamSnapshot {
  return {
    key,
    label: key,
    group: "geometry",
    paramType,
    default: 1,
    hard: null,
    soft: null,
    step: null,
    unit: "none",
    enumVariants: [],
    accept: [],
    nodePath: null,
    showIf: [],
    drivenByPort: null,
    doc: "",
  } as unknown as ParamSnapshot;
}

function node(params: NodeMirror["params"]): NodeMirror {
  return {
    id: 1,
    typeId: "box",
    typeVersion: 1,
    params,
    position: [0, 0],
    bypassed: false, label: "n", visible: true, declaresVisibility: false,
  };
}


describe("reading the stored expression", () => {
  it("returns the text when the source is an expression", () => {
    const n = node({ width: { kind: "expression", expr: "1 + 1" } });
    expect(paramExpression(n, spec("width"))).toBe("1 + 1");
  });

  it("returns null for a literal or an unset param", () => {
    const n = node({ width: { kind: "literal", type: "float", value: 2 } });
    expect(paramExpression(n, spec("width"))).toBeNull();
    expect(paramExpression(node({}), spec("width"))).toBeNull();
  });
});


describe("parking an expression while the row shows its value", () => {
  beforeEach(clearParkedExpressions);

  it("hands back exactly what was parked, whitespace and all", () => {
    // The whole point: switching off used to replace the expression with
    // the number it resolved to, and switching back seeded a new one from
    // that number, so `$F < 121 ? 0 : 1` became a dead `0`.
    parkExpression("root", 4, "index", "$F < 121 ? 0 : 1");
    expect(parkedExpression("root", 4, "index")).toBe("$F < 121 ? 0 : 1");

    parkExpression("root", 4, "index", "  ch('../ctrl/size')  ");
    expect(parkedExpression("root", 4, "index")).toBe("  ch('../ctrl/size')  ");
  });

  it("reports nothing for a parameter that never had one switched off", () => {
    expect(parkedExpression("root", 4, "index")).toBeNull();
  });

  it("keys on the context as well as the node, since ids repeat per context", () => {
    parkExpression("root", 4, "index", "$F");
    expect(parkedExpression({ subflow: 1 }, 4, "index")).toBeNull();
    expect(parkedExpression("root", 5, "index")).toBeNull();
    expect(parkedExpression("root", 4, "other")).toBeNull();
    expect(parkedExpression("root", 4, "index")).toBe("$F");
  });

  it("forgets the expression when the clear control discards it", () => {
    parkExpression("root", 4, "index", "$F");
    discardParkedExpression("root", 4, "index");
    expect(parkedExpression("root", 4, "index")).toBeNull();
  });

  it("forgets everything when a document loads", () => {
    // Node ids are reused across documents, so without this an unrelated
    // node in the incoming scene inherits the outgoing scene's expression.
    parkExpression("root", 4, "index", "$F");
    parkExpression({ subflow: 9 }, 9, "scale", "$T * 2");
    clearParkedExpressions();
    expect(parkedExpression("root", 4, "index")).toBeNull();
    expect(parkedExpression({ subflow: 9 }, 9, "scale")).toBeNull();
  });
});

