import { describe, expect, it } from "vitest";
import { errorLine } from "./SnippetField";

// The wrangle's parse errors arrive as ordinary cook errors, formatted by
// the engine as "line N, column M: ...". The gutter highlight is driven by
// parsing that back, so this is the seam where the two sides agree.

describe("errorLine", () => {
  it("reads the line out of an engine cook error", () => {
    expect(errorLine("line 2, column 6: the assignment has no value")).toBe(2);
  });

  it("reads a multi-digit line", () => {
    expect(errorLine("line 137, column 1: something")).toBe(137);
  });

  it("is null when there is no error at all", () => {
    expect(errorLine(undefined)).toBeNull();
  });

  it("is null for a cook error that names no line", () => {
    // Not every cook failure comes from the parser: a kernel-side element
    // failure names a mesh and an element instead, and the gutter must not
    // guess a line from it.
    expect(errorLine("`@Cd` is assigned a 1-component value at element 3")).toBeNull();
  });

  it("does not mistake the word line inside a message for a location", () => {
    expect(errorLine("the polyline input is empty")).toBeNull();
  });

  it("ignores a zero line, which is never valid in 1-based coordinates", () => {
    expect(errorLine("line 0, column 1: impossible")).toBeNull();
  });
});
