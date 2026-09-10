// What is left of the node info card's helpers after the shared rules
// moved into `solarxy-studio`.
//
// One function, and the reason it stayed is the reason it is worth
// naming. Rendering an ABSOLUTE date needs a locale and a timezone, which
// are facts about the person reading rather than about the document, and
// taking an internationalization stack into the engine to print one line
// would be a large bill for a small answer. The relative phrase beside it
// IS a rule and comes across with the rest of the report, on
// `TimestampText.relative`.
//
// Everything else is gone rather than moved-and-forwarded: the duration
// scale, the bounds line, the relative phrase and the connection summary
// arrive already read as text on `nodeReportText`, in one crossing for
// the whole card. The connection summary is the one worth a note, because
// it was not a formatting rule at all: the browser walked its own
// mirrored graph to answer who is wired to a node, which is an engine
// question the desktop had no answer to.

/** An absolute local date-time, with the engine's relative phrase in
 * parentheses when it still says something.
 *
 * `ms` null means unknown and renders as such, never as an epoch date. */
export function formatTimestamp(ms: number | null, relative: string): string {
  if (ms === null || !Number.isFinite(ms)) return "unknown";
  const absolute = new Date(ms).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  return relative ? `${absolute} (${relative})` : absolute;
}
