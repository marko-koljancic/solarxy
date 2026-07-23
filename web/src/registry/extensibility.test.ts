// The zero-frontend-change contract, as a test. A node type the frontend has
// never seen (fabricated into a RegistrySnapshot) must be fully interpretable
// by the same registry-driven helpers the palette and parameter panel use:
// it appears in the context-filtered palette, its ports color/coerce, and
// every one of its params maps to a widget. No per-node code exists anywhere.

import { describe, expect, it } from "vitest";
import type { NodeTypeSnapshot, RegistrySnapshot } from "../engine/types";
import { GLYPH_PATHS, glyphPath, nodeRole } from "../flow/nodeVisual";
import {
  DATA_TYPE_COLOR,
  coercionKind,
  connectionLegal,
  contextKind,
  dataTypeShape,
  descriptorFor,
  isSupportedParamType,
  portDataType,
} from "./datatypes";

/** A node the frontend has no knowledge of, using diverse existing types. */
const PROBE: NodeTypeSnapshot = {
  typeId: "probe",
  version: 1,
  displayName: "Probe",
  category: "generators",
  categoryLabel: "Generators",
  contexts: ["geo"],
  opens: null,
  inputs: [
    { key: "geometry", label: "Geometry", dataType: "geometry", variadic: false, required: false, min: 0, isDefault: true, doc: "" },
    { key: "detail_map", label: "Detail Map", dataType: "image", variadic: false, required: false, min: 0, isDefault: false, doc: "" },
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
    { key: "tint", label: "Tint", group: "shape", paramType: "color", enumVariants: [], accept: [], default: [1, 1, 1, 1], hard: null, soft: null, step: null, unit: "none", drivenByPort: "detail_map", doc: "" },
    { key: "material", label: "Material", group: "shape", paramType: "nodePath", nodePath: { kind: "opens", opens: "mat" }, enumVariants: [], accept: [], default: null, hard: null, soft: null, step: null, unit: "none", doc: "" },
    { key: "lane", label: "Lane", group: "shape", paramType: "attributeName", enumVariants: [], accept: [], default: "color", hard: null, soft: null, step: null, unit: "none", doc: "" },
  ],
  bypass: { mode: "mute" },
  doc: "A fabricated node the frontend has never seen.",
  searchAliases: ["probe", "novel"],
  // Identity hints: a glyph key the frontend has NO art for,
  // so the category fallback is what the tests below exercise.
  glyph: "probe",
  role: "standard",
};

/** A minimal snapshot: just the real coercion cells the probe needs, plus the
 * probe. (The real snapshot carries the full matrix.) */
const SNAP: RegistrySnapshot = {
  nodes: [PROBE],
  coercions: [
    { from: "geometry", to: "geometry", kind: "same" },
    { from: "float", to: "int", kind: "lossy" },
    { from: "int", to: "float", kind: "lossless" },
    { from: "image", to: "image", kind: "same" },
  ],
};

describe("extensibility: a novel node renders from the snapshot alone", () => {
  it("is discoverable and context-filtered like any node", () => {
    expect(descriptorFor(SNAP, "probe")?.displayName).toBe("Probe");
    // A geo-network palette (pure kind filter) includes it; the root
    // (obj) palette does not. The kinds come from the typed-context
    // vocabulary; a node declaring a NEW kind is still just a
    // filter match away.
    expect(SNAP.nodes.filter((n) => n.contexts.includes("geo")).map((n) => n.typeId)).toContain(
      "probe",
    );
    expect(SNAP.nodes.filter((n) => n.contexts.includes("obj"))).toHaveLength(0);
  });

  it("derives a canvas's kind from its owner's descriptor, not its type id", () => {
    // The root canvas is always obj.
    expect(contextKind(SNAP, "root", [])).toBe("obj");
    // A container the frontend has never seen: its child canvas's kind is
    // whatever the descriptor opens.
    const container: NodeTypeSnapshot = {
      ...PROBE,
      typeId: "texnet_probe",
      contexts: ["obj"],
      opens: "tex",
      inputs: [],
      outputs: [],
    };
    const snap: RegistrySnapshot = { nodes: [PROBE, container], coercions: SNAP.coercions };
    const ownerNodes = [
      { id: 7, typeId: "texnet_probe", typeVersion: 1, params: {}, position: [0, 0] as [number, number], bypassed: false, portOrder: {} },
    ];
    expect(contextKind(snap, { subflow: 7 }, ownerNodes)).toBe("tex");
    // An unknown owner falls back to geo (the only pre-context child kind).
    expect(contextKind(snap, { subflow: 99 }, ownerNodes)).toBe("geo");
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

  it("speaks the Image vocabulary", () => {
    const map = portDataType(SNAP, "probe", "detail_map", "input");
    expect(map).toBe("image");
    // Distinct hue and the resource (hexagon) shape.
    expect(DATA_TYPE_COLOR[map!]).toBeDefined();
    expect(dataTypeShape(map!)).toBe("hexagon");
    // Image wires only into Image; nothing coerces across.
    expect(coercionKind(SNAP, "image", "image")).toBe("same");
    expect(coercionKind(SNAP, "image", "geometry")).toBeNull();
    expect(coercionKind(SNAP, "float", "image")).toBeNull();
    // The map-overrides-factor link is plain snapshot data: the panel's
    // dim predicate needs only the param's drivenByPort and the node's
    // edges, never per-node code.
    const probe = descriptorFor(SNAP, "probe")!;
    const tint = probe.params.find((p) => p.key === "tint")!;
    expect(tint.drivenByPort).toBe("detail_map");
    expect(probe.inputs.some((i) => i.key === tint.drivenByPort)).toBe(true);
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

  it("always resolves drawable node art (revamp D-18: glyph + role hints)", () => {
    const probe = descriptorFor(SNAP, "probe")!;
    // "probe" is a glyph key with no frontend art: the category fallback
    // (generators -> box) must produce a real path, never a broken icon.
    expect(GLYPH_PATHS[probe.glyph]).toBeUndefined();
    expect(glyphPath(probe)).toBe(GLYPH_PATHS.box);
    // A declared, known role resolves as-is.
    expect(nodeRole(probe)).toBe("standard");
    // A role variant NEWER than this frontend (arrives as an unknown
    // string over the boundary) falls back by category, not by crash.
    const future = { ...probe, role: "hologram" as never };
    expect(nodeRole(future)).toBe("standard");
    // And a declared glyph WITH art wins over the fallback.
    const merged = { ...probe, glyph: "merge" };
    expect(glyphPath(merged)).toBe(GLYPH_PATHS.merge);
  });
});
