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
    opens: null,
    bypass: { mode: "mute" },
    ...over,
  }) as unknown as NodeTypeSnapshot;

describe("isContainerType", () => {
  // It followed the SILHOUETTE until 0.10.0, with a category fallback for
  // a role this build could not draw. Diving in needs a network to dive
  // into, which is what the type declares, so the gate asks that instead:
  // a presentation answer no longer stands in for an engine one, and a
  // node drawn as a container that opens nothing can no longer be entered.
  it("follows what the type opens", () => {
    expect(isContainerType(desc({ opens: "sop" }))).toBe(true);
    expect(isContainerType(desc({ opens: null }))).toBe(false);
  });

  it("ignores the silhouette, which says nothing about diving in", () => {
    expect(isContainerType(desc({ role: "container", opens: null }))).toBe(false);
    expect(isContainerType(desc({ role: "hologram" as never, opens: "mat" }))).toBe(true);
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
