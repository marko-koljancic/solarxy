import { describe, expect, it } from "vitest";
import { formatDurationMs } from "./duration";

describe("formatDurationMs", () => {
  // The same cases `a_span_reads_the_same_way_everywhere` asserts in
  // solarxy-host. They are meant to be read against each other: this is the
  // only thing keeping the browser and the terminal writing one render's
  // elapsed the same way, since the two implementations cannot share code.
  it("spells a span the way the shared formatter does", () => {
    expect(formatDurationMs(0)).toBe("0.0s");
    expect(formatDurationMs(1500)).toBe("1.5s");
    expect(formatDurationMs(59_900)).toBe("59.9s");
    expect(formatDurationMs(60_000)).toBe("1m 00s");
    expect(formatDurationMs(252_000)).toBe("4m 12s");
    expect(formatDurationMs(3_599_000)).toBe("59m 59s");
    expect(formatDurationMs(3_600_000)).toBe("1h 00m");
    expect(formatDurationMs(3_840_000)).toBe("1h 04m");
  });

  // The Rust side takes an unsigned integer and cannot be handed these; the
  // boundary carries a double, so this side can. A clock that went backwards
  // should read as nothing having happened rather than as "NaNs".
  it("treats a nonsensical span as no time at all", () => {
    expect(formatDurationMs(-1)).toBe("0.0s");
    expect(formatDurationMs(Number.NaN)).toBe("0.0s");
    expect(formatDurationMs(Number.POSITIVE_INFINITY)).toBe("0.0s");
  });
});
