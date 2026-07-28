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

  // The other half of the contract `useDraftCommit` implements: the stored
  // value is authoritative, so an undo or a rename the engine uniquified
  // resets both the draft and what counts as already-sent. Without the
  // second reset, re-typing the value the field had before the undo would
  // look like a repeat and dispatch nothing, silently losing the edit.
  it("follows the stored value when it changes underneath", () => {
    const dispatched: string[] = [];
    let stored = "box1";
    let draft = stored;
    let lastSent = stored;
    const commit = () => {
      if (!shouldCommit(draft, lastSent)) return;
      lastSent = draft;
      dispatched.push(draft);
    };
    /** What the hook's effect does when `value` changes. */
    const storedChanged = (next: string) => {
      stored = next;
      draft = next;
      lastSent = next;
    };

    draft = "control";
    commit();
    expect(dispatched).toEqual(["control"]);

    // Undo: the engine puts the old name back.
    storedChanged("box1");
    expect(draft).toBe("box1");

    // Re-typing "control" must dispatch again, not be swallowed.
    draft = "control";
    commit();
    expect(dispatched).toEqual(["control", "control"]);
    expect(stored).toBe("box1");
  });
});
