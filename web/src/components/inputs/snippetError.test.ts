// Decoding the engine's cook error back into a position.
//
// The line half shipped in 0.8.1; the column half is new, and it is what
// lets the editor underline the token rather than washing the whole row.

import { describe, expect, it } from "vitest";
import { errorLine, errorPosition } from "./snippetError";

describe("errorPosition", () => {
  it("reads both halves of the engine's format", () => {
    // The exact shape `ExprError::line_col` produces.
    expect(errorPosition("line 3, column 12: unknown function `noize`")).toEqual({
      line: 3,
      column: 12,
      message: "line 3, column 12: unknown function `noize`",
    });
  });

  it("falls back to column 1 when only a line is named", () => {
    // Not every engine error goes through the expression formatter; those
    // should still tint their line rather than being dropped.
    expect(errorPosition("line 7: something went wrong")).toEqual({
      line: 7,
      column: 1,
      message: "line 7: something went wrong",
    });
  });

  it("returns null for a message with no position", () => {
    expect(errorPosition("this program assigns nothing")).toBeNull();
    expect(errorPosition(undefined)).toBeNull();
    expect(errorPosition("")).toBeNull();
  });

  it("rejects a zero or negative line rather than marking one", () => {
    // Lines are 1-based; a 0 would index before the document.
    expect(errorPosition("line 0, column 4: x")).toBeNull();
  });

  it("rejects a zero column rather than trusting it", () => {
    expect(errorPosition("line 2, column 0: x")?.column).toBe(1);
  });

  it("keeps the whole message, not just the tail", () => {
    // The message is what the squiggle's hover shows, so truncating it to
    // the part after the colon would lose the position the user can read.
    const m = "line 1, column 5: `@Cd` cannot be assigned a float";
    expect(errorPosition(m)?.message).toBe(m);
  });
});

describe("errorLine", () => {
  it("is the line half of errorPosition", () => {
    expect(errorLine("line 4, column 2: x")).toBe(4);
    expect(errorLine("no position here")).toBeNull();
  });
});
