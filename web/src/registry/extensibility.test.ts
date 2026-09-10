// The zero-frontend-change contract, as a test. A node type the frontend has
// never seen (fabricated into a RegistrySnapshot) must be fully interpretable
// by the same registry-driven helpers the palette and parameter panel use:
// it appears in the context-filtered palette, its ports color/coerce, and
// every one of its params maps to a widget. No per-node code exists anywhere.

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type {
  NodeTypeSnapshot,
  PresentationTables,
  RegistrySnapshot,
} from "../engine/types";
import { GLYPH_PATHS, glyphPath, nodeRole } from "../flow/nodeVisual";
import {
  coercionKind,
  compareCategories,
  connectionLegal,
  descriptorFor,
  isSupportedParamType,
  portDataType,
  SUPPORTED_PARAM_TYPES,
} from "./datatypes";

/** A node the frontend has no knowledge of, using diverse existing types. */
const PROBE: NodeTypeSnapshot = {
  typeId: "probe",
  version: 1,
  displayName: "Probe",
  category: "generators",
  categoryLabel: "Generators",
  contexts: ["sop"],
  opens: null,
  inputs: [
    { key: "geometry", label: "Geometry", dataType: "geometry", variadic: false, required: false, min: 0, isDefault: true, doc: "" },
    { key: "detail_map", label: "Detail Map", dataType: "image", variadic: false, required: false, min: 0, isDefault: false, doc: "" },
  ],
  outputs: [
    { key: "geometry", label: "Geometry", dataType: "geometry", variadic: false, required: false, min: 0, isDefault: true, doc: "" },
  ],
  params: [
    { key: "size", label: "Size", group: "geometry", paramType: "float", enumVariants: [], accept: [], default: 1, hard: [0.01, 100], soft: [0.1, 10], step: 0.1, unit: "meters", acceptsExpression: true, doc: "" },
    { key: "segments", label: "Segments", group: "geometry", paramType: "int", enumVariants: [], accept: [], default: 3, hard: [1, 64], soft: null, step: 1, unit: "none", acceptsExpression: true, doc: "" },
    { key: "capped", label: "Capped", group: "geometry", paramType: "bool", enumVariants: [], accept: [], default: true, hard: null, soft: null, step: null, unit: "none", acceptsExpression: true, doc: "" },
    { key: "mode", label: "Mode", group: "shape", paramType: "enum", enumVariants: [["a", "Alpha"], ["b", "Beta"]], accept: [], default: "a", hard: null, soft: null, step: null, unit: "none", acceptsExpression: false, doc: "" },
    { key: "offset", label: "Offset", group: "shape", paramType: "vec3", enumVariants: [], accept: [], default: [0, 0, 0], hard: null, soft: null, step: 0.01, unit: "none", acceptsExpression: true, doc: "" },
    { key: "tint", label: "Tint", group: "shape", paramType: "color", enumVariants: [], accept: [], default: [1, 1, 1, 1], hard: null, soft: null, step: null, unit: "none", drivenByPort: "detail_map", acceptsExpression: true, doc: "" },
    { key: "material", label: "Material", group: "shape", paramType: "nodePath", nodePath: { kind: "opens", opens: "mat" }, enumVariants: [], accept: [], default: null, hard: null, soft: null, step: null, unit: "none", acceptsExpression: false, doc: "" },
    { key: "lane", label: "Lane", group: "shape", paramType: "attributeName", enumVariants: [], accept: [], default: "color", hard: null, soft: null, step: null, unit: "none", acceptsExpression: false, doc: "" },
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

/** A stand-in for the presentation tables the engine serves at boot.
 *
 * Its CONTENT proves nothing and is not asserted: which hue, which shape
 * and which family a category falls back to are pinned in
 * `solarxy-studio` against the real thirteen data types and the real
 * fifteen categories. What this fixture is for is the plumbing -- that
 * the browser reads these answers instead of holding a second copy -- so
 * it carries the shape of the real thing and only the few entries the
 * cases below reach for. */
const TABLES: PresentationTables = {
  dataTypes: {
    geometry: { token: "wire-geometry", shape: "round" },
    image: { token: "wire-image", shape: "hexagon" },
  },
  categories: {
    container: { order: 0, glyph: "sopnet", role: "container" },
    generators: { order: 1, glyph: "box", role: "standard" },
    lights: { order: 9, glyph: "point", role: "light" },
  },
} as unknown as PresentationTables;

describe("extensibility: a novel node renders from the snapshot alone", () => {
  it("is discoverable and context-filtered like any node", () => {
    expect(descriptorFor(SNAP, "probe")?.displayName).toBe("Probe");
    // A geo-network palette (pure kind filter) includes it; the root
    // (obj) palette does not. The kinds come from the typed-context
    // vocabulary; a node declaring a NEW kind is still just a
    // filter match away.
    expect(SNAP.nodes.filter((n) => n.contexts.includes("sop")).map((n) => n.typeId)).toContain(
      "probe",
    );
    expect(SNAP.nodes.filter((n) => n.contexts.includes("obj"))).toHaveLength(0);
  });

  // A canvas's kind used to be derived here, by looking up the owning
  // container's descriptor and guessing when the owner was unknown. The
  // engine stamps a network with its kind when the container creates it,
  // and the mirror carries it, so there is no derivation left to test.

  it("has typed handles the frontend can color + validate", () => {
    const out = portDataType(SNAP, "probe", "geometry", "output");
    expect(out).toBe("geometry");
    // Probe -> Probe geometry is a legal (same) connection.
    expect(connectionLegal(SNAP, "probe", "geometry", "probe", "geometry").legal).toBe(true);
    // The matrix still classifies lossy/lossless for the frontend rings.
    expect(coercionKind(SNAP, "float", "int")).toBe("lossy");
    expect(coercionKind(SNAP, "int", "float")).toBe("lossless");
    // Image wires only into Image; nothing coerces across.
    expect(coercionKind(SNAP, "image", "image")).toBe("same");
    expect(coercionKind(SNAP, "image", "geometry")).toBeNull();
    expect(coercionKind(SNAP, "float", "image")).toBeNull();
  });

  // How a data type is DRAWN is no longer decided here, so it is no longer
  // asserted here. That hue says family and shape separates the family,
  // that no two types are drawn identically, and that the vectors count
  // their components are all pinned in `solarxy-studio` against the real
  // thirteen types and the real coercion matrix. This file's fixture
  // carries four matrix cells, and a distinctness claim read off four
  // fabricated cells would pass while checking almost nothing -- which is
  // exactly what the first draft of this test did.
  //
  // What is left here is that the browser READS those answers.
  it("draws a port from the tables rather than from a copy", () => {
    const map = portDataType(SNAP, "probe", "detail_map", "input");
    expect(map).toBe("image");
    expect(TABLES.dataTypes[map!].shape).toBe("hexagon");
    // A token, never a value: a shell that authored its own hue would put
    // a literal here and nothing would hold it to the palette.
    expect(TABLES.dataTypes[map!].token).toMatch(/^wire-/);
  });

  it("orders categories by the engine's order, and degrades past its end", () => {
    expect(compareCategories(TABLES, "container", "generators")).toBeLessThan(0);
    expect(compareCategories(TABLES, "lights", "container")).toBeGreaterThan(0);
    // A category this build has not heard of sorts after the known ones.
    expect(compareCategories(TABLES, "hologram", "container")).toBeGreaterThan(0);
    expect(compareCategories(TABLES, "hologram", "phantom")).toBeLessThan(0);
  });

  it("the map-overrides-factor link is plain snapshot data", () => {
    // The panel's dim predicate needs only the param's drivenByPort and
    // the node's edges, never per-node code.
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

  it("always resolves drawable node art from glyph + role hints", () => {
    const probe = descriptorFor(SNAP, "probe")!;
    // "probe" is a glyph key with no frontend art: the category fallback
    // (generators -> box) must produce a real path, never a broken icon.
    expect(GLYPH_PATHS[probe.glyph]).toBeUndefined();
    expect(glyphPath(probe, TABLES)).toBe(GLYPH_PATHS.box);
    // A declared, known role resolves as-is.
    expect(nodeRole(probe, TABLES)).toBe("standard");
    // A role variant NEWER than this frontend (arrives as an unknown
    // string over the boundary) falls back by category, not by crash.
    const future = { ...probe, role: "hologram" as never };
    expect(nodeRole(future, TABLES)).toBe("standard");
    // And a declared glyph WITH art wins over the fallback.
    const merged = { ...probe, glyph: "merge" };
    expect(glyphPath(merged, TABLES)).toBe(GLYPH_PATHS.merge);
  });
});

describe("the widget list is this shell's, and it is not stale", () => {
  // `SUPPORTED_PARAM_TYPES` says which param types this panel draws. The
  // engine has no opinion about that, so it stays here -- but it is a
  // claim about a switch statement thirty lines away, and until 0.10.0
  // nothing held the two together. It had drifted by three entries: the
  // panel had been rendering `action`, `assetRef` and `multilineText` for
  // releases while this list said it could not, and the only reader was
  // the fabricated probe above, whose params all happened to be listed.
  //
  // Read off the real registry, which is the file the panel is driven by.
  const registry = JSON.parse(
    readFileSync(new URL("../../../schemas/registry.json", import.meta.url), "utf8"),
  ) as RegistrySnapshot;

  it("covers every param type any registered node declares", () => {
    const declared = new Set(registry.nodes.flatMap((n) => n.params.map((p) => p.paramType)));
    expect(declared.size).toBeGreaterThan(10);
    for (const t of [...declared].sort()) {
      expect(isSupportedParamType(t), `no widget for param type ${t}`).toBe(true);
    }
  });

  it("claims no widget it has no reason to draw", () => {
    // The other direction, so the list cannot be padded into passing: a
    // type nothing declares is either dead or a widget waiting for a node
    // that never came.
    const declared = new Set(registry.nodes.flatMap((n) => n.params.map((p) => p.paramType)));
    for (const t of SUPPORTED_PARAM_TYPES) {
      expect(declared.has(t), `${t} is listed but no node declares it`).toBe(true);
    }
  });
});
