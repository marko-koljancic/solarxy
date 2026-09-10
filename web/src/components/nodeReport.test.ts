// The one half of the info card's reading that stays with the shell.
//
// The duration scale, the bounds line, the relative phrase and the
// connection summary moved into `solarxy-studio` and are asserted there,
// against the same cases these used to carry. What is left is the
// absolute date, which needs a locale and a timezone, and the rule for
// composing it with the phrase the engine sends.

import { describe, expect, it } from "vitest";
import { formatTimestamp } from "./nodeReport";

describe("formatTimestamp", () => {
  const now = Date.UTC(2026, 6, 28, 12, 0, 0);

  it("says unknown for a scene that predates timestamps", () => {
    // NOT "1 Jan 1970". A null stamp is a real answer and must read as one.
    expect(formatTimestamp(null, "")).toBe("unknown");
  });

  it("says unknown rather than rendering a non-finite stamp", () => {
    expect(formatTimestamp(Number.NaN, "5 minutes ago")).toBe("unknown");
  });

  it("renders a real stamp with the engine's relative hint beside it", () => {
    const out = formatTimestamp(now, "5 minutes ago");
    expect(out).toContain("5 minutes ago");
    expect(out).toMatch(/^\S/);
    expect(out).not.toBe("unknown");
  });

  it("drops the parentheses once the engine sends no hint", () => {
    // The engine returns an empty phrase past thirty days, on the grounds
    // that the absolute date carries it alone by then.
    const out = formatTimestamp(now, "");
    expect(out).not.toContain("(");
    expect(out).not.toContain("ago");
  });
});
