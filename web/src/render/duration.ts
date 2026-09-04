// How a span of time is written, on this side of the boundary.
//
// A mirror of `format_duration_ms` in `solarxy-host`, not a second opinion.
// TypeScript cannot call the Rust one, and sending a formatted string across on
// every progress event would put presentation in the engine and cost a string
// allocation per frame to say something the page can work out. So the rule is
// copied and the copy is pinned: `duration.test.ts` asserts the same cases the
// Rust test asserts, and the two lists are meant to be read side by side.
//
// The rule: seconds with a tenth below a minute, because that is the range
// where a tenth means something; whole seconds above it, because past a minute
// nobody reads the fraction.

/** A span in milliseconds, as a person reads one. */
export function formatDurationMs(ms: number): string {
  const clamped = Number.isFinite(ms) && ms > 0 ? ms : 0;
  const secs = Math.floor(clamped / 1000);
  if (secs < 60) return `${(clamped / 1000).toFixed(1)}s`;
  if (secs < 3600) {
    return `${Math.floor(secs / 60)}m ${String(secs % 60).padStart(2, "0")}s`;
  }
  return `${Math.floor(secs / 3600)}h ${String(Math.floor((secs % 3600) / 60)).padStart(2, "0")}m`;
}
