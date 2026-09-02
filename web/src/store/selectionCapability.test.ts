// Which viewport tools are offered for what is selected. The rule is the
// host's, not the frontend's: this only checks that the mirror carries the
// answer and that the two components reading it agree about what an empty
// answer means.

import { describe, expect, it } from "vitest";

import type { ToolMode } from "../engine/types";
import { toolApplies, useViewState } from "./viewState";

describe("toolApplies", () => {
  it("narrows nothing when nothing manipulable is selected", () => {
    // An empty set is "no target", not "no tool works". Arming a tool with
    // nothing selected is harmless and goes live on the next selection, so an
    // empty scene's tool column looks the way it always has.
    for (const t of ["select", "move", "rotate", "scale", "aim"] as ToolMode[]) {
      expect(toolApplies(t, [])).toBe(true);
    }
  });

  it("offers a point light moving and nothing else", () => {
    const available: ToolMode[] = ["select", "move"];
    expect(toolApplies("move", available)).toBe(true);
    expect(toolApplies("select", available)).toBe(true);
    expect(toolApplies("rotate", available)).toBe(false);
    expect(toolApplies("scale", available)).toBe(false);
    expect(toolApplies("aim", available)).toBe(false);
  });

  it("offers an aiming light moving and aiming, but not rotating", () => {
    const available: ToolMode[] = ["select", "move", "aim"];
    expect(toolApplies("aim", available)).toBe(true);
    expect(toolApplies("rotate", available)).toBe(false);
  });

  it("leaves geometry with everything it had", () => {
    const available: ToolMode[] = ["select", "move", "rotate", "scale"];
    for (const t of ["select", "move", "rotate", "scale"] as ToolMode[]) {
      expect(toolApplies(t, available)).toBe(true);
    }
    // Geometry does not aim: it carries an orientation, not a second point.
    expect(toolApplies("aim", available)).toBe(false);
  });
});

describe("the selection capability mirror", () => {
  it("stores the tools and the transform params the host reported", () => {
    useViewState.setState({ selectionTools: [], selectionTransformParams: [] });
    useViewState
      .getState()
      .setSelectionCapability(["select", "move"], ["position"]);
    const s = useViewState.getState();
    expect(s.selectionTools).toEqual(["select", "move"]);
    // What "reset transform" resets, which for a point light is one param and
    // not the four geometry has.
    expect(s.selectionTransformParams).toEqual(["position"]);
  });

  it("clears back to empty when the selection stops being manipulable", () => {
    useViewState
      .getState()
      .setSelectionCapability(["select", "move"], ["position"]);
    useViewState.getState().setSelectionCapability([], []);
    const s = useViewState.getState();
    expect(s.selectionTools).toEqual([]);
    expect(s.selectionTransformParams).toEqual([]);
  });
});
