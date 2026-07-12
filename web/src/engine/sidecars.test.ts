import { describe, expect, it } from "vitest";

import { hasMissing, missingSidecars, referencedSidecars } from "./sidecars";

const enc = new TextEncoder();

function gltf(json: object): Uint8Array {
  return enc.encode(JSON.stringify(json));
}

describe("referencedSidecars", () => {
  it("collects external gltf buffers as required and images as optional", () => {
    const refs = referencedSidecars(
      "FlightHelmet.gltf",
      gltf({
        buffers: [{ uri: "FlightHelmet.bin", byteLength: 3 }],
        images: [{ uri: "textures/albedo.png" }, { uri: "normal.png" }],
      }),
    );
    expect(refs.required).toEqual(["FlightHelmet.bin"]);
    expect(refs.optional).toEqual(["albedo.png", "normal.png"]);
  });

  it("skips data URIs and dedupes", () => {
    const refs = referencedSidecars(
      "m.gltf",
      gltf({
        buffers: [{ uri: "data:application/octet-stream;base64,AAA=" }, { uri: "geo.bin" }, { uri: "geo.bin" }],
        images: [{ uri: "data:image/png;base64,AAA=" }],
      }),
    );
    expect(refs.required).toEqual(["geo.bin"]);
    expect(refs.optional).toEqual([]);
  });

  it("percent-decodes URIs and basenames subpaths", () => {
    const refs = referencedSidecars(
      "m.gltf",
      gltf({ buffers: [{ uri: "sub%20dir/my%20buffer.bin" }] }),
    );
    expect(refs.required).toEqual(["my buffer.bin"]);
  });

  it("handles buffer-view-only gltf (no uri fields) and embedded images", () => {
    const refs = referencedSidecars(
      "m.gltf",
      gltf({ buffers: [{ byteLength: 8 }], images: [{ bufferView: 0 }] }),
    );
    expect(hasMissing(refs)).toBe(false);
  });

  it("collects obj mtllib lines as optional, spaces included", () => {
    const obj = enc.encode("# comment\nmtllib helmet materials.mtl\nv 0 0 0\nmtllib extra.mtl\n");
    const refs = referencedSidecars("model.obj", obj);
    expect(refs.required).toEqual([]);
    expect(refs.optional).toEqual(["helmet materials.mtl", "extra.mtl"]);
  });

  it("returns nothing for self-contained formats and unparseable primaries", () => {
    expect(hasMissing(referencedSidecars("m.glb", enc.encode("glTF binary")))).toBe(false);
    expect(hasMissing(referencedSidecars("m.stl", enc.encode("solid t")))).toBe(false);
    expect(hasMissing(referencedSidecars("broken.gltf", enc.encode("{not json")))).toBe(false);
  });
});

describe("missingSidecars", () => {
  const refs = {
    required: ["FlightHelmet.bin"],
    optional: ["albedo.png", "normal.png"],
  };

  it("diffs against staged basenames", () => {
    const missing = missingSidecars(refs, ["FlightHelmet.gltf", "albedo.png"]);
    expect(missing.required).toEqual(["FlightHelmet.bin"]);
    expect(missing.optional).toEqual(["normal.png"]);
  });

  it("is satisfied when everything is staged, matching by basename", () => {
    const missing = missingSidecars(refs, [
      "FlightHelmet.bin",
      "albedo.png",
      "normal.png",
    ]);
    expect(hasMissing(missing)).toBe(false);
  });

  it("matches case-sensitively, exactly as the resolver does", () => {
    const missing = missingSidecars(refs, ["flighthelmet.bin"]);
    expect(missing.required).toEqual(["FlightHelmet.bin"]);
  });
});
