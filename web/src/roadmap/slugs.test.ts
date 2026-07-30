// Referential integrity for the roadmap page's hand-authored data: every id
// that one array points at must exist in the array that owns it, and the
// derived dispositions must reconcile exactly with the published totals.
// Nothing validates this data against the internal docs, so these are the
// only checks between an authoring slip and a publicly wrong page.

import { describe, expect, it } from "vitest";
import {
  BACKLOG_WAVES,
  CARD_COUNT,
  CARDS,
  CHANGELOG,
  COVERAGE,
  COVERAGE_RELEASES,
  DEFERRED_IDS,
  DISPOSITIONS,
  JOURNEY_COUNT,
  JOURNEYS,
  PERSONAS,
  PROGRAM,
  PROGRAM_RELEASE_COUNT,
  SECTIONS,
  THEMES,
  WONT_IDS,
} from "./data";
import { derivePlacement } from "./derive";

function duplicates(ids: string[]): string[] {
  const seen = new Set<string>();
  return ids.filter((id) => (seen.has(id) ? true : (seen.add(id), false)));
}

const cardIds = new Set(CARDS.map((c) => c.id));
const journeyIds = new Set(JOURNEYS.map((j) => j.id));
const themeKeys = new Set(THEMES.map((t) => t.key));

describe("section slugs", () => {
  it("are unique and lowercase", () => {
    expect(duplicates(SECTIONS.map((s) => s.id))).toEqual([]);
    for (const s of SECTIONS) expect(s.id).toMatch(/^[a-z]+$/);
  });
});

describe("cards", () => {
  it("match the published count with unique ids", () => {
    expect(CARDS).toHaveLength(CARD_COUNT);
    expect(duplicates(CARDS.map((c) => c.id))).toEqual([]);
  });

  it("each belong to a defined theme", () => {
    for (const c of CARDS) {
      expect(themeKeys.has(c.t), `card ${c.id} names unknown theme ${c.t}`).toBe(true);
    }
  });
});

describe("the program", () => {
  it("matches the published release count", () => {
    expect(PROGRAM).toHaveLength(PROGRAM_RELEASE_COUNT);
  });

  it("references only real cards", () => {
    const referenced = [
      ...PROGRAM.flatMap((r) => r.cards),
      ...BACKLOG_WAVES.flatMap((w) => w.cards),
      ...DEFERRED_IDS,
      ...WONT_IDS,
    ];
    for (const id of referenced) {
      expect(cardIds.has(id), `unknown card id ${id}`).toBe(true);
    }
  });

  it("derives exactly one disposition per card, reconciling the totals", () => {
    const { dispOf } = derivePlacement();
    const totals: Record<string, number> = {};
    for (const c of CARDS) {
      const d = dispOf[c.id];
      expect(d, `card ${c.id} has no disposition`).toBeTruthy();
      totals[d] = (totals[d] ?? 0) + 1;
    }
    for (const d of DISPOSITIONS) {
      expect(totals[d.key], `disposition ${d.key}`).toBe(d.n);
    }
    expect(DISPOSITIONS.reduce((sum, d) => sum + d.n, 0)).toBe(CARD_COUNT);
  });

  it("covers journeys against the non-shipped releases in order", () => {
    const upcoming = PROGRAM.filter((r) => r.kind !== "shipped").map((r) => r.v);
    expect(COVERAGE_RELEASES).toEqual(upcoming);
    for (const [jid, row] of Object.entries(COVERAGE)) {
      expect(journeyIds.has(jid), `coverage row for unknown journey ${jid}`).toBe(true);
      expect(row, `coverage row ${jid}`).toHaveLength(COVERAGE_RELEASES.length);
    }
  });
});

describe("personas and journeys", () => {
  it("match the published journey count with unique ids", () => {
    expect(JOURNEYS).toHaveLength(JOURNEY_COUNT);
    expect(duplicates(JOURNEYS.map((j) => j.id))).toEqual([]);
  });

  it("reference only real journeys", () => {
    for (const p of PERSONAS) {
      for (const jid of p.jr) {
        expect(journeyIds.has(jid), `persona ${p.idx} names unknown journey ${jid}`).toBe(true);
      }
    }
  });
});

describe("the changelog", () => {
  it("has unique version entries", () => {
    expect(duplicates(CHANGELOG.map((c) => c.v))).toEqual([]);
  });
});
