// The render node's action, as a mapping.
//
// Two things can silently disagree with the Rust side here and neither would
// throw: the sample count a quality preset means, and the spelling the engine
// crosses the boundary as. A wrong preset renders the wrong number of samples,
// which looks like the render being slow or noisy rather than like a bug; a
// wrong engine spelling falls through to the rasterizer, which produces a
// perfectly good image that is not the one asked for.
//
// The registry here is the shipped one, read off disk, so these are asserted
// against what the node actually declares rather than against a fixture that
// could drift from it.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import type { NodeMirror, RegistrySnapshot } from "../engine/types";
import { QUALITY_SAMPLES, stillRequestFor } from "./stillRequest";

const registry = JSON.parse(
  readFileSync(new URL("../../../schemas/registry.json", import.meta.url), "utf8"),
) as RegistrySnapshot;

/** A render node with nothing set, which is what a freshly added one is and
 * what a document saved before version 2 reopens as. */
function bare(params: Record<string, unknown> = {}): NodeMirror {
  return {
    id: 1,
    typeId: "render",
    name: "render1",
    position: [0, 0],
    params: Object.fromEntries(
      Object.entries(params).map(([k, v]) => [k, { kind: "literal", value: v }]),
    ),
  } as unknown as NodeMirror;
}

describe("the render node's still request", () => {
  it("reads every default off the registry rather than guessing", () => {
    const r = stillRequestFor(bare(), registry, null);
    expect(r.width).toBe(1920);
    expect(r.height).toBe(1080);
    // The node's own default quality, resolved through the preset table.
    expect(r.samples).toBe(64);
    expect(r.engine).toBe("raster");
    expect(r.denoise).toBe(false);
    expect(r.camera).toBeNull();
  });

  it("maps every quality the node declares to a sample count", () => {
    const spec = registry.nodes
      .find((n) => n.typeId === "render")
      ?.params.find((p) => p.key === "quality");
    // The JSON carries each variant as a `[value, label]` pair.
    const variants = (spec?.enumVariants ?? []).map((v) => v[0]);
    expect(variants.length).toBeGreaterThan(0);
    for (const value of variants) {
      expect(
        QUALITY_SAMPLES[value],
        `the node declares a quality "${value}" the frontend has no sample count for`,
      ).toBeGreaterThan(0);
    }
    // And nothing in the table that the node does not declare, which would be
    // a preset nobody can select.
    for (const key of Object.keys(QUALITY_SAMPLES)) {
      expect(variants, `the table has "${key}" and the node does not`).toContain(key);
    }
  });

  it("maps the presets to the counts the milestone specifies", () => {
    expect(stillRequestFor(bare({ quality: "draft" }), registry, null).samples).toBe(16);
    expect(stillRequestFor(bare({ quality: "good" }), registry, null).samples).toBe(64);
    expect(stillRequestFor(bare({ quality: "high" }), registry, null).samples).toBe(256);
    expect(stillRequestFor(bare({ quality: "reference" }), registry, null).samples).toBe(1024);
  });

  it("spells the traced engine the way the boundary expects", () => {
    // The node says "traced" and the host reads "pathTraced". The two differ,
    // so the translation has to happen exactly once and this is where.
    expect(stillRequestFor(bare({ engine: "traced" }), registry, null).engine).toBe("pathTraced");
    expect(stillRequestFor(bare({ engine: "raster" }), registry, null).engine).toBe("raster");
  });

  it("falls back to Good rather than to nothing for a preset it has not been taught", () => {
    // A node type can gain a variant without the frontend changing; degrading
    // to a working render beats rendering zero samples.
    const r = stillRequestFor(bare({ quality: "cinematic" }), registry, null);
    expect(r.samples).toBe(64);
  });

  it("carries the camera the node names", () => {
    expect(stillRequestFor(bare(), registry, 42).camera).toBe(42);
  });
});
