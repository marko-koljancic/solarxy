import { describe, expect, it } from "vitest";
import {
  EXPRESSION_TYPES,
  acceptsExpression,
  formatResolved,
  paramExpression,
  seedExpression,
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
    bypassed: false,
  };
}

describe("which params accept an expression", () => {
  it("covers exactly the numeric types", () => {
    // Held to the Rust set by expression_types_match_the_frontend in
    // crates/solarxy-core/tests/tokens_drift.rs.
    expect([...EXPRESSION_TYPES].sort()).toEqual([
      "bool",
      "color",
      "float",
      "int",
      "vec2",
      "vec3",
      "vec4",
    ]);
  });

  it("refuses the types an expression could never produce a value for", () => {
    // There is no string type in the value lattice, and an asset or node
    // reference is an identity rather than a number.
    for (const t of ["text", "attributeName", "enum", "assetRef", "nodePath", "action"]) {
      expect(acceptsExpression(t), t).toBe(false);
    }
  });
});

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

describe("seeding a freshly opened field", () => {
  it("starts from the value the param already had", () => {
    // Opening blank would be a parse error, badging the node the instant
    // the user clicked the affordance.
    expect(seedExpression(2.5)).toBe("2.5");
    expect(seedExpression(0)).toBe("0");
  });

  it("spells a vector with set()", () => {
    expect(seedExpression([1, 2, 3])).toBe("set(1, 2, 3)");
    expect(seedExpression([1, 0, 0, 1])).toBe("set(1, 0, 0, 1)");
  });

  it("spells a bool as a comparison, since the grammar has no literals", () => {
    expect(seedExpression(true)).toBe("1 > 0");
    expect(seedExpression(false)).toBe("0 > 1");
  });

  it("never emits something unparseable for junk", () => {
    expect(seedExpression(undefined)).toBe("0");
    expect(seedExpression(Number.NaN)).toBe("0");
    expect(seedExpression(Number.POSITIVE_INFINITY)).toBe("0");
    expect(seedExpression("nonsense")).toBe("0");
  });
});

describe("the resolved-value readout", () => {
  it("rounds without leaving trailing zeros", () => {
    expect(formatResolved({ type: "float", value: 1 / 3 })).toBe("0.333333");
    expect(formatResolved({ type: "float", value: 2.5 })).toBe("2.5");
    expect(formatResolved({ type: "float", value: 2 })).toBe("2");
  });

  it("shows vectors component-wise", () => {
    expect(formatResolved({ type: "vec3", value: [1, 2, 3] })).toBe("1, 2, 3");
    expect(formatResolved({ type: "color", value: [1, 0, 0, 1] })).toBe("1, 0, 0, 1");
  });

  it("shows a bool as a word", () => {
    expect(formatResolved({ type: "bool", value: true })).toBe("true");
    expect(formatResolved({ type: "bool", value: false })).toBe("false");
  });

  it("survives the non-finite values IEEE division produces", () => {
    // `1 / 0` is an infinity by design (one bad element must not blank a
    // scene), so the readout has to render one rather than crash.
    expect(formatResolved({ type: "float", value: Number.POSITIVE_INFINITY })).toBe(
      "Infinity",
    );
    expect(formatResolved({ type: "float", value: Number.NaN })).toBe("NaN");
  });
});
