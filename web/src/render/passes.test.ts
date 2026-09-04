import { describe, expect, it } from "vitest";
import { passAvailable, passOptions } from "./passes";
import type { StillPasses } from "../engine/types";

const traced = (over: Partial<StillPasses> = {}): StillPasses => ({
  albedo: false,
  normal: false,
  depth: false,
  engineWritesAovs: true,
  ...over,
});

describe("passOptions", () => {
  it("shows the beauty alone before a render has said anything", () => {
    expect(passOptions(undefined).map((o) => o.value)).toEqual(["beauty"]);
  });

  // The rule the epic ratified: a pass unavailable because the engine writes
  // none is unavailable for that reason, and the frontend does not decide it.
  it("shows the beauty alone when the engine writes no passes", () => {
    const raster = { ...traced(), engineWritesAovs: false };
    expect(passOptions(raster).map((o) => o.value)).toEqual(["beauty"]);
    expect(passAvailable("albedo", raster)).toBe(false);
  });

  it("offers a pass nobody asked for as disabled, saying what would produce it", () => {
    const rows = passOptions(traced({ normal: true }));
    expect(rows.map((o) => o.value)).toEqual(["beauty", "albedo", "normal", "depth"]);
    expect(rows.find((o) => o.value === "normal")?.unavailable).toBeUndefined();

    const albedo = rows.find((o) => o.value === "albedo");
    expect(albedo?.unavailable).toContain("Albedo pass");
    expect(albedo?.unavailable).toContain("render again");
  });

  it("lets a requested pass be chosen and refuses the rest", () => {
    const p = traced({ albedo: true, depth: true });
    expect(passAvailable("beauty", p)).toBe(true);
    expect(passAvailable("albedo", p)).toBe(true);
    expect(passAvailable("normal", p)).toBe(false);
    expect(passAvailable("depth", p)).toBe(true);
  });

  it("always allows the beauty, even with nothing known about the render", () => {
    expect(passAvailable("beauty", undefined)).toBe(true);
  });
});
