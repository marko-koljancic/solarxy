// The tour catalog's self-consistency. The companion check that every
// `target` selector matches a real class in web/src lives in Rust
// (`solarxy-core/tests/tokens_drift.rs`, `tour_steps_point_at_real_classes`)
// because this test graph has no node:fs types; vitest holds the pure half.

import { describe, expect, it } from "vitest";
import { completionWritesOnboarding } from "./Tour";
import { OVERVIEW_TOUR, TOURS, tourById } from "./steps";

describe("tour catalog", () => {
  it("has unique tour ids, titles, and versions", () => {
    const ids = TOURS.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const tour of TOURS) {
      expect(tour.title.length).toBeGreaterThan(0);
      expect(tour.version).toBeGreaterThanOrEqual(1);
    }
  });

  it("gives every tour unique step ids and real copy, not placeholders", () => {
    for (const tour of TOURS) {
      const stepIds = tour.steps.map((s) => s.id);
      expect(new Set(stepIds).size, tour.id).toBe(stepIds.length);
      for (const s of tour.steps) {
        expect(s.title.length, `${tour.id}/${s.id}`).toBeGreaterThan(3);
        expect(s.body.length, `${tour.id}/${s.id}`).toBeGreaterThan(40);
        expect(s.target.trim().length, `${tour.id}/${s.id}`).toBeGreaterThan(1);
      }
    }
  });

  it("keeps every script walkable in one sitting", () => {
    for (const tour of TOURS) {
      expect(tour.steps.length, tour.id).toBeLessThanOrEqual(8);
      expect(tour.steps.length, tour.id).toBeGreaterThanOrEqual(4);
    }
  });

  it("resolves replay ids and falls back to the overview", () => {
    expect(tourById("modeling").id).toBe("modeling");
    expect(tourById("review").id).toBe("review");
    expect(tourById(undefined)).toBe(OVERVIEW_TOUR);
    expect(tourById("nope")).toBe(OVERVIEW_TOUR);
  });

  it("only completing the overview writes onboarding", () => {
    // A topic replay from Help must never eat a new user's first-run.
    expect(completionWritesOnboarding("overview")).toBe(true);
    expect(completionWritesOnboarding("modeling")).toBe(false);
    expect(completionWritesOnboarding("review")).toBe(false);
  });
});
