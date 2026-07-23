// The pure param-metadata rules behind the panel and the Properties menu
// bar: tab derivation, the group-to-keys mapping the tab reset uses, and
// showIf evaluation against stored values and defaults.

import { describe, expect, it } from "vitest";
import type { ParamSnapshot, ParamSource } from "../engine/types";
import {
  groupKeys,
  paramTabs,
  paramVisible,
  resolveActiveTab,
  VALIDATION_TAB,
} from "./paramVisibility";

function spec(partial: Partial<ParamSnapshot> & { key: string }): ParamSnapshot {
  return {
    label: partial.key,
    group: "general",
    paramType: "float",
    enumVariants: [],
    accept: [],
    default: 0,
    hard: null,
    soft: null,
    step: null,
    unit: "none",
    doc: "",
    ...partial,
  };
}

const TYPE = spec({ key: "type", paramType: "enum", default: "float", group: "attribute" });
const VALUE_F = spec({
  key: "value_float",
  group: "attribute",
  showIf: [{ param: "type", pred: { kind: "eq", value: "float" } }],
});
const VALUE_V3 = spec({
  key: "value_vec3",
  group: "attribute",
  paramType: "vec3",
  default: [0, 0, 0],
  showIf: [{ param: "type", pred: { kind: "eq", value: "vec3" } }],
});
const SPECS = [TYPE, VALUE_F, VALUE_V3];

const lit = (type: string, value: unknown) => ({ kind: "literal", type, value }) as ParamSource;

describe("paramTabs / resolveActiveTab / groupKeys", () => {
  it("orders general first and appends the validation tab only with a report", () => {
    const specs = [
      spec({ key: "a", group: "attribute" }),
      spec({ key: "b", group: "general" }),
      spec({ key: "c", group: "attribute" }),
    ];
    expect(paramTabs(specs, false)).toEqual(["general", "attribute"]);
    expect(paramTabs(specs, true)).toEqual(["general", "attribute", VALIDATION_TAB]);
  });

  it("falls back to the first tab when the stored one is gone", () => {
    expect(resolveActiveTab(["general", "attribute"], "attribute")).toBe("attribute");
    expect(resolveActiveTab(["general"], "attribute")).toBe("general");
    expect(resolveActiveTab([], "attribute")).toBeUndefined();
  });

  it("maps a group to its keys for the tab reset", () => {
    expect(groupKeys(SPECS, "attribute")).toEqual(["type", "value_float", "value_vec3"]);
    expect(groupKeys(SPECS, "nope")).toEqual([]);
  });
});

describe("paramVisible", () => {
  it("shows only the variant matching the stored enum value", () => {
    const params = { type: lit("enum", "vec3") };
    expect(paramVisible(VALUE_F, SPECS, params)).toBe(false);
    expect(paramVisible(VALUE_V3, SPECS, params)).toBe(true);
  });

  it("falls back to the referenced param's default when nothing is stored", () => {
    expect(paramVisible(VALUE_F, SPECS, {})).toBe(true);
    expect(paramVisible(VALUE_V3, SPECS, {})).toBe(false);
  });

  it("a spec without clauses is always visible", () => {
    expect(paramVisible(TYPE, SPECS, {})).toBe(true);
  });

  it("evaluates truthy, neq, and in", () => {
    const flag = spec({ key: "flag", paramType: "bool", default: false });
    const gated = spec({
      key: "gated",
      showIf: [{ param: "flag", pred: { kind: "truthy" } }],
    });
    expect(paramVisible(gated, [flag, gated], {})).toBe(false);
    expect(paramVisible(gated, [flag, gated], { flag: lit("bool", true) })).toBe(true);

    const neq = spec({
      key: "neq",
      showIf: [{ param: "type", pred: { kind: "neq", value: "float" } }],
    });
    expect(paramVisible(neq, SPECS, {})).toBe(false);
    expect(paramVisible(neq, SPECS, { type: lit("enum", "vec2") })).toBe(true);

    const oneOf = spec({
      key: "oneOf",
      showIf: [{ param: "type", pred: { kind: "in", values: ["vec2", "vec3"] } }],
    });
    expect(paramVisible(oneOf, SPECS, { type: lit("enum", "vec3") })).toBe(true);
    expect(paramVisible(oneOf, SPECS, { type: lit("enum", "float") })).toBe(false);
  });

  it("compares array values structurally (vec defaults)", () => {
    const anchor = spec({ key: "anchor", paramType: "vec3", default: [0, 1, 0] });
    const gated = spec({
      key: "gated",
      showIf: [{ param: "anchor", pred: { kind: "eq", value: [0, 1, 0] } }],
    });
    expect(paramVisible(gated, [anchor, gated], {})).toBe(true);
    expect(
      paramVisible(gated, [anchor, gated], { anchor: lit("vec3", [1, 1, 0]) }),
    ).toBe(false);
  });

  it("ANDs multiple clauses", () => {
    const both = spec({
      key: "both",
      showIf: [
        { param: "type", pred: { kind: "eq", value: "float" } },
        { param: "flag", pred: { kind: "truthy" } },
      ],
    });
    const flag = spec({ key: "flag", paramType: "bool", default: false });
    const specs = [...SPECS, flag, both];
    expect(paramVisible(both, specs, {})).toBe(false);
    expect(paramVisible(both, specs, { flag: lit("bool", true) })).toBe(true);
  });
});
