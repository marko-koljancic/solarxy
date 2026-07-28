// Escape belongs to the innermost thing on screen.
//
// This is a regression test for a bug that shipped twice in the same
// dialog. A help popover, and separately an open `Select` list, each took
// the whole Preferences window down when dismissed -- discarding unsaved
// edits to answer "what does this row do?" or to close a dropdown you
// opened by mistake.
//
// Neither was fixable by stopping propagation. `Modal` listens on `window`
// in the CAPTURE phase, so it runs before a sibling window listener
// registered later (the popover) and long before a React `onKeyDown` on the
// element (the select). By the time either could stop anything, the dialog
// had already closed. The dialog has to ask instead, which is what this
// pins.

import { beforeEach, describe, expect, it } from "vitest";
import { claimEscape, isEscapeClaimed, releaseEscape } from "./escapeClaim";

/** Drains any leak from a previous test so each starts from zero. */
function reset(): void {
  while (isEscapeClaimed()) releaseEscape();
}

describe("the Escape claim", () => {
  beforeEach(reset);

  it("is unclaimed until an overlay opens", () => {
    expect(isEscapeClaimed()).toBe(false);
    claimEscape();
    expect(isEscapeClaimed()).toBe(true);
    releaseEscape();
    expect(isEscapeClaimed()).toBe(false);
  });

  it("survives two overlays at once", () => {
    // A portaled Select claims twice: once for the open list, once for the
    // DropdownPortal that renders it. Releasing one must not free Escape.
    claimEscape();
    claimEscape();
    releaseEscape();
    expect(isEscapeClaimed()).toBe(true);
    releaseEscape();
    expect(isEscapeClaimed()).toBe(false);
  });

  it("cannot go negative if a release is delivered twice", () => {
    releaseEscape();
    releaseEscape();
    expect(isEscapeClaimed()).toBe(false);
    claimEscape();
    expect(isEscapeClaimed()).toBe(true);
  });
});

describe("Escape with an overlay open inside a dialog", () => {
  beforeEach(reset);

  /** Replays the real topology: the dialog's handler runs FIRST, whatever
   * the overlay does, because capture on `window` beats both a later
   * window listener and any handler on the element itself. */
  function pressEscape(): { dialogClosed: boolean; overlayClosed: boolean } {
    const out = { dialogClosed: false, overlayClosed: false };
    // Modal.tsx: window keydown, capture, registered at mount.
    if (!isEscapeClaimed()) out.dialogClosed = true;
    // Popover.tsx (window capture, registered on open) or Select.tsx
    // (onKeyDown on the trigger). Either way it runs after the dialog's.
    if (isEscapeClaimed()) {
      releaseEscape();
      out.overlayClosed = true;
    }
    return out;
  }

  it("closes the overlay and leaves the dialog up", () => {
    claimEscape();
    expect(pressEscape()).toEqual({ dialogClosed: false, overlayClosed: true });
  });

  it("still closes the dialog when nothing is above it", () => {
    expect(pressEscape()).toEqual({ dialogClosed: true, overlayClosed: false });
  });

  it("takes two presses to get from an overlay to a closed dialog", () => {
    claimEscape();
    const first = pressEscape();
    const second = pressEscape();
    expect(first).toEqual({ dialogClosed: false, overlayClosed: true });
    expect(second.dialogClosed).toBe(true);
  });
});
