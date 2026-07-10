// Review store logic: section partitioning, filters, reply lookup, and the
// selection-pruning on annotation refresh.

import { describe, expect, it } from "vitest";
import type { Annotation } from "../engine/types";
import { repliesOf, sectionAnnotations, useReview, type ReviewFilters } from "./review";

function note(
  id: number,
  over: Partial<Annotation> = {},
): Annotation {
  return {
    id,
    anchor: { ctx: "root", node: 1 },
    text: `note ${id}`,
    category: "question",
    resolved: false,
    createdAt: "2026-07-10T09:00:00Z",
    updatedAt: "2026-07-10T09:00:00Z",
    needsReanchor: false,
    ...over,
  };
}

function allFilters(): ReviewFilters {
  return {
    categories: { info: true, warning: true, question: true, change: true },
    showResolved: true,
    text: "",
  };
}

describe("sectionAnnotations", () => {
  it("partitions into open / needs-reanchor / complete, replies excluded", () => {
    const annotations = [
      note(1),
      note(2, { needsReanchor: true }),
      note(3, { resolved: true }),
      note(4, { replyTo: 1 }),
    ];
    const s = sectionAnnotations(annotations, allFilters());
    expect(s.open.map((a) => a.id)).toEqual([1]);
    expect(s.needsReanchor.map((a) => a.id)).toEqual([2]);
    expect(s.complete.map((a) => a.id)).toEqual([3]);
  });

  it("staleness trumps the open section; resolved trumps staleness", () => {
    const s = sectionAnnotations(
      [note(1, { needsReanchor: true, resolved: true })],
      allFilters(),
    );
    expect(s.needsReanchor).toHaveLength(0);
    expect(s.complete.map((a) => a.id)).toEqual([1]);
  });

  it("category and resolved filters hide rows", () => {
    const filters = allFilters();
    filters.categories.question = false;
    filters.showResolved = false;
    const s = sectionAnnotations(
      [note(1), note(2, { category: "change" }), note(3, { category: "change", resolved: true })],
      filters,
    );
    expect(s.open.map((a) => a.id)).toEqual([2]);
    expect(s.complete).toHaveLength(0);
  });

  it("text filter matches the thread (parent, author, replies)", () => {
    const annotations = [
      note(1, { text: "seam issue" }),
      note(2, { text: "plain" }),
      note(3, { text: "fixed in v2", replyTo: 2 }),
      note(4, { text: "other", author: "Mara" }),
    ];
    const byText = sectionAnnotations(annotations, { ...allFilters(), text: "seam" });
    expect(byText.open.map((a) => a.id)).toEqual([1]);
    const byReply = sectionAnnotations(annotations, { ...allFilters(), text: "v2" });
    expect(byReply.open.map((a) => a.id)).toEqual([2]);
    const byAuthor = sectionAnnotations(annotations, { ...allFilters(), text: "mara" });
    expect(byAuthor.open.map((a) => a.id)).toEqual([4]);
  });
});

describe("repliesOf", () => {
  it("lists direct replies only", () => {
    const annotations = [note(1), note(2, { replyTo: 1 }), note(3, { replyTo: 1 }), note(4)];
    expect(repliesOf(annotations, 1).map((a) => a.id)).toEqual([2, 3]);
    expect(repliesOf(annotations, 4)).toHaveLength(0);
  });
});

describe("useReview.setAnnotations", () => {
  it("prunes a selection and reanchor target that no longer exist", () => {
    const s = useReview.getState();
    s.setAnnotations([note(1), note(2)]);
    s.setSelected(1);
    s.setReanchorTarget(2);
    useReview.getState().setAnnotations([note(2)]);
    expect(useReview.getState().selected).toBeNull();
    expect(useReview.getState().reanchorTarget).toBe(2);
    useReview.getState().setAnnotations([]);
    expect(useReview.getState().reanchorTarget).toBeNull();
  });
});
