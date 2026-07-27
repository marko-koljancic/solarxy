import { describe, expect, it } from "vitest";
import { shouldCommit } from "./draftCommit";

/** Replays the Enter path: commit, then the blur that Enter itself causes. */
function enterThenBlur(stored: string, typed: string): string[] {
  const dispatched: string[] = [];
  let lastSent = stored;
  const commit = () => {
    if (!shouldCommit(typed, lastSent)) return;
    lastSent = typed;
    dispatched.push(typed);
  };
  commit(); // Enter
  commit(); // the blur Enter triggers, before the mirror has caught up
  return dispatched;
}

describe("draft commit", () => {
  it("dispatches a rename once, not once per handler", () => {
    // Two dispatches here means two undo steps, the second a no-op, so
    // the user's first undo appears to do nothing.
    expect(enterThenBlur("box1", "control")).toEqual(["control"]);
  });

  it("dispatches nothing when the field was not edited", () => {
    expect(enterThenBlur("box1", "box1")).toEqual([]);
  });

  it("still commits an edit made after a previous one", () => {
    let lastSent = "box1";
    expect(shouldCommit("control", lastSent)).toBe(true);
    lastSent = "control";
    expect(shouldCommit("control", lastSent)).toBe(false);
    expect(shouldCommit("driver", lastSent)).toBe(true);
  });
});
