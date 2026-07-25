// The lane-pick type coupling: which sibling enum a dropdown pick may
// retype. Deliberately narrow (key `type`, same group, matching variant)
// so the panel stays a generic interpreter everywhere else.

import { describe, expect, it } from "vitest";
import type { NodeTypeSnapshot, ParamSnapshot } from "../../engine/types";
import { siblingTypeParam } from "./AttributeNameField";

const param = (over: Partial<ParamSnapshot>): ParamSnapshot =>
  ({
    key: "attr_name",
    label: "Name",
    group: "attribute",
    paramType: "attributeName",
    enumVariants: [],
    accept: [],
    default: "color",
    hard: null,
    soft: null,
    step: null,
    unit: "none",
    doc: "",
    ...over,
  }) as ParamSnapshot;

const desc = (params: ParamSnapshot[]): NodeTypeSnapshot =>
  ({ typeId: "probe", params }) as unknown as NodeTypeSnapshot;

const nameSpec = param({});
const typeSpec = param({
  key: "type",
  paramType: "enum",
  enumVariants: [
    ["float", "Float"],
    ["vec3", "Vec3"],
    ["vec4", "Vec4"],
  ],
});

describe("siblingTypeParam", () => {
  it("finds the same-group type enum with a matching variant", () => {
    expect(siblingTypeParam(desc([nameSpec, typeSpec]), nameSpec, "vec3")?.key).toBe("type");
  });

  it("yields null when no variant matches the lane's ty", () => {
    // attribute_randomize declares no vec2 variant: picking `uv` fills
    // the name only.
    expect(siblingTypeParam(desc([nameSpec, typeSpec]), nameSpec, "vec2")).toBeNull();
  });

  it("ignores a type param in another group", () => {
    const other = param({
      key: "type",
      group: "projection",
      paramType: "enum",
      enumVariants: [["vec3", "Vec3"]],
    });
    expect(siblingTypeParam(desc([nameSpec, other]), nameSpec, "vec3")).toBeNull();
  });

  it("survives a missing descriptor", () => {
    expect(siblingTypeParam(undefined, nameSpec, "vec3")).toBeNull();
  });
});
