// The action gates shared by the radial and the list view. The dispatch
// mappers themselves are compile-time-checked against the Command union;
// what can silently drift is the gating, so that is what is pinned.

import { describe, expect, it } from "vitest";
import type { NodeTypeSnapshot } from "../engine/types";
import { isBypassable, isContainerType } from "./nodeActions";

const desc = (over: Partial<NodeTypeSnapshot>): NodeTypeSnapshot =>
  ({
    typeId: "probe",
    category: "generators",
    glyph: "box",
    role: "standard",
    bypass: { mode: "mute" },
    ...over,
  }) as unknown as NodeTypeSnapshot;

describe("isContainerType", () => {
  it("follows the role, matching the canvas radial's gate", () => {
    expect(isContainerType(desc({ role: "container" }))).toBe(true);
    expect(isContainerType(desc({ role: "standard" }))).toBe(false);
  });

  it("falls back by category for an unknown role", () => {
    expect(isContainerType(desc({ role: "hologram" as never, category: "container" }))).toBe(true);
    expect(isContainerType(desc({ role: "hologram" as never }))).toBe(false);
  });

  it("treats a missing descriptor as not a container", () => {
    expect(isContainerType(undefined)).toBe(false);
  });
});

describe("isBypassable", () => {
  it("only notBypassable is excluded", () => {
    expect(isBypassable(desc({ bypass: { mode: "mute" } }))).toBe(true);
    expect(
      isBypassable(desc({ bypass: { mode: "passThrough", input: "geometry" } })),
    ).toBe(true);
    expect(isBypassable(desc({ bypass: { mode: "notBypassable" } }))).toBe(false);
  });

  it("a missing descriptor is conservatively bypassable, like the canvas", () => {
    expect(isBypassable(undefined)).toBe(true);
  });
});
