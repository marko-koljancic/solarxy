// The per-node info-line heuristic: category-specific summaries plus the
// generic first-param fallback, all pure over snapshot + mirror shapes.
// Fresh nodes have a SPARSE params record (values come from the registry
// defaults), so both lanes are covered.

import { describe, expect, it } from "vitest";
import type { NodeMirror, NodeTypeSnapshot, ParamSnapshot } from "../engine/types";
import { fmtNumber, nodeInfoLine } from "./infoLine";

function param(
  key: string,
  paramType: string,
  group = "general",
  dflt: unknown = null,
): ParamSnapshot {
  return {
    key,
    label: key,
    group,
    paramType,
    enumVariants: [],
    accept: [],
    default: dflt,
    hard: null,
    soft: null,
    step: null,
    unit: "none",
    doc: "",
  };
}

function desc(
  category: NodeTypeSnapshot["category"],
  params: ParamSnapshot[],
): NodeTypeSnapshot {
  return {
    typeId: "t",
    version: 1,
    displayName: "T",
    category,
    categoryLabel: category[0].toUpperCase() + category.slice(1),
    rootContext: true,
    subflowContext: true,
    inputs: [],
    outputs: [],
    params,
    bypass: { mode: "mute" },
    doc: "",
    searchAliases: [],
  };
}

function node(params: NodeMirror["params"]): NodeMirror {
  return { id: 1, typeId: "t", typeVersion: 1, params, position: [0, 0], bypassed: false };
}

describe("nodeInfoLine", () => {
  it("shows light intensity from an explicit literal", () => {
    const d = desc("lights", [param("intensity", "float")]);
    const n = node({ intensity: { kind: "literal", type: "float", value: 1.5 } });
    expect(nodeInfoLine(d, n)).toBe("intensity 1.5");
  });

  it("falls back to the registry default when params are sparse", () => {
    const d = desc("lights", [param("intensity", "float", "light", 0.5)]);
    expect(nodeInfoLine(d, node({}))).toBe("intensity 0.5");
  });

  it("shows primitive dimensions with abbreviations, capped at three", () => {
    const d = desc("primitives", [
      param("width", "float", "geometry", 1),
      param("height", "float", "geometry", 2),
      param("depth", "float", "geometry", 1),
      param("radius", "float", "geometry", 9),
    ]);
    const n = node({ depth: { kind: "literal", type: "float", value: 0.25 } });
    expect(nodeInfoLine(d, n)).toBe("w 1  h 2  d 0.25");
  });

  it("shows the staged asset name for imports, hash prefix without one", () => {
    const d = desc("import", [param("source", "assetRef", "general", "")]);
    const hash = "abcdef0123456789";
    const n = node({ source: { kind: "literal", type: "asset", value: hash } });
    expect(nodeInfoLine(d, n, () => "dragon.obj")).toBe("dragon.obj");
    expect(nodeInfoLine(d, n)).toBe("abcdef0123…");
    expect(nodeInfoLine(d, node({}))).toBe("no file");
  });

  it("falls back to the first non-general numeric param", () => {
    const d = desc("modifiers", [
      param("seed", "int", "general", 4),
      param("angle", "float", "transform", 45),
    ]);
    expect(nodeInfoLine(d, node({}))).toBe("angle 45");
  });

  it("labels enums with the variant display name", () => {
    const spec = {
      ...param("mode", "enum", "options", "x"),
      enumVariants: [["x", "Exact"]] as [string, string][],
    };
    const d = desc("utility", [spec]);
    expect(nodeInfoLine(d, node({}))).toBe("Exact");
  });

  it("returns null with no matching params and skips expression sources", () => {
    expect(nodeInfoLine(desc("utility", []), node({}))).toBeNull();
    const d = desc("lights", [param("intensity", "float")]);
    const n = node({ intensity: { kind: "expression", expr: "1+1" } });
    expect(nodeInfoLine(d, n)).toBeNull();
  });
});

describe("fmtNumber", () => {
  it("trims to three decimals without trailing zeros", () => {
    expect(fmtNumber(1)).toBe("1");
    expect(fmtNumber(0.25)).toBe("0.25");
    expect(fmtNumber(1.23456)).toBe("1.235");
  });
});
