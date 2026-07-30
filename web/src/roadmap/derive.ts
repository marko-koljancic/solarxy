/* Release and disposition are derived from PROGRAM and BACKLOG_WAVES rather
 * than stored on each card, so the two can never disagree. A card may appear
 * in more than one release: the desktop wiring ships as its engine half in
 * 0.8.2 and its canvas half in 0.9.5. */

import { BACKLOG_WAVES, DEFERRED_IDS, PROGRAM, WONT_IDS } from "./data";

export type DispositionKey = "shipped" | "scheduled" | "backlog" | "deferred" | "wont";

export interface Placement {
  /** Card id to the releases it appears in, in program order. */
  relOf: Record<string, string[]>;
  /** Card id to its single disposition. */
  dispOf: Record<string, DispositionKey>;
  /** Card id to its backlog trigger, backlog cards only. */
  trigOf: Record<string, string>;
}

export function derivePlacement(): Placement {
  const relOf: Record<string, string[]> = {};
  const dispOf: Record<string, DispositionKey> = {};
  const trigOf: Record<string, string> = {};
  for (const r of PROGRAM) {
    for (const id of r.cards) {
      (relOf[id] = relOf[id] ?? []).push(r.v);
      dispOf[id] = r.kind === "shipped" ? "shipped" : "scheduled";
    }
  }
  for (const w of BACKLOG_WAVES) {
    for (const id of w.cards) {
      dispOf[id] = "backlog";
      trigOf[id] = w.trig;
    }
  }
  for (const id of DEFERRED_IDS) dispOf[id] = "deferred";
  for (const id of WONT_IDS) dispOf[id] = "wont";
  return { relOf, dispOf, trigOf };
}
