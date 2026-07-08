// The zero-frontend-change contract, as a test. A node type the frontend has
// never seen (fabricated into a RegistrySnapshot) must be fully interpretable
// by the same registry-driven helpers the palette and parameter panel use:
// it appears in the context-filtered palette, its ports color/coerce, and
// every one of its params maps to a widget. No per-node code exists anywhere.

import { describe, expect, it } from "vitest";
import type { NodeTypeSnapshot, RegistrySnapshot } from "../engine/types";
import {
  DATA_TYPE_COLOR,
  coercionKind,
  connectionLegal,
  descriptorFor,
  isSupportedParamType,
  portDataType,
} from "./datatypes";

/** A node the frontend has no knowledge of, using diverse existing types. */
const PROBE: NodeTypeSnapshot = {
  typeId: "probe",
  version: 1,
  displayName: "Probe",
  category: "primitives",
  rootContext: false,
  subflowContext: true,
  inputs: [
    { key: "geometry", label: "Geometry", dataType: "geometry", variadic: false, required: false, min: 0, isDefault: true, doc: "" },
  ],
  outputs: [
    { key: "geometry", label: "Geometry", dataType: "geometry", variadic: false, required: false, min: 0, isDefault: true, doc: "" },
  ],
  params: [
    { key: "size", label: "Size", group: "geometry", paramType: "float", enumVariants: [], accept: [], default: 1, hard: [0.01, 100], soft: [0.1, 10], step: 0.1, unit: "meters", doc: "" },
    { key: "segments", label: "Segments", group: "geometry", paramType: "int", enumVariants: [], accept: [], default: 3, hard: [1, 64], soft: null, step: 1, unit: "none", doc: "" },
    { key: "capped", label: "Capped", group: "geometry", paramType: "bool", enumVariants: [], accept: [], default: true, hard: null, soft: null, step: null, unit: "none", doc: "" },
    { key: "mode", label: "Mode", group: "shape", paramType: "enum", enumVariants: [["a", "Alpha"], ["b", "Beta"]], accept: [], default: "a", hard: null, soft: null, step: null, unit: "none", doc: "" },
    { key: "offset", label: "Offset", group: "shape", paramType: "vec3", enumVariants: [], accept: [], default: [0, 0, 0], hard: null, soft: null, step: 0.01, unit: "none", doc: "" },
    { key: "tint", label: "Tint", group: "shape", paramType: "color", enumVariants: [], accept: [], default: [1, 1, 1, 1], hard: null, soft: null, step: null, unit: "none", doc: "" },
  ],
  bypass: { mode: "mute" },
  doc: "A fabricated node the frontend has never seen.",
  searchAliases: ["probe", "novel"],
};

/** A minimal snapshot: just the real coercion cells the probe needs, plus the
 * probe. (The real snapshot carries the full matrix.) */
const SNAP: RegistrySnapshot = {
  nodes: [PROBE],
  coercions: [
    { from: "geometry", to: "geometry", kind: "same" },
    { from: "float", to: "int", kind: "lossy" },
    { from: "int", to: "float", kind: "lossless" },
  ],
};

describe("extensibility: a novel node renders from the snapshot alone", () => {
  it("is discoverable and context-filtered like any node", () => {
    expect(descriptorFor(SNAP, "probe")?.displayName).toBe("Probe");
    // A subflow palette (pure filter) includes it; a root palette does not.
    expect(SNAP.nodes.filter((n) => n.subflowContext).map((n) => n.typeId)).toContain("probe");
    expect(SNAP.nodes.filter((n) => n.rootContext)).toHaveLength(0);
  });

  it("has typed handles the frontend can color + validate", () => {
    const out = portDataType(SNAP, "probe", "geometry", "output");
    expect(out).toBe("geometry");
    expect(DATA_TYPE_COLOR[out!]).toBeDefined();
    // Probe -> Probe geometry is a legal (same) connection.
    expect(connectionLegal(SNAP, "probe", "geometry", "probe", "geometry").legal).toBe(true);
    // The matrix still classifies lossy/lossless for the frontend rings.
    expect(coercionKind(SNAP, "float", "int")).toBe("lossy");
    expect(coercionKind(SNAP, "int", "float")).toBe("lossless");
  });

  it("renders a widget for every one of its params (no unsupported type)", () => {
    const probe = descriptorFor(SNAP, "probe")!;
    for (const p of probe.params) {
      expect(isSupportedParamType(p.paramType), `param ${p.key} type ${p.paramType}`).toBe(true);
    }
    // And the panel would group them by `group`, preserving order.
    const groups = new Set(probe.params.map((p) => p.group));
    expect([...groups]).toEqual(["geometry", "shape"]);
  });
});
