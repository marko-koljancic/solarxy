// The review mirror + interaction state. The engine owns the annotations
// (this store re-reads them from `review_annotations()` on every
// `reviewChanged`); everything else here is host-side interaction state:
// review mode, the create/edit draft, the pending re-anchor target, panel
// filters, and the selected thread.

import { create } from "zustand";
import type { Annotation, PickDetail, ReviewCategory } from "../engine/types";

/** The floating editor's contents (create, edit, or reply). */
export interface ReviewDraft {
  /** The pick the draft anchors to (creates only; edits keep the anchor). */
  pick: PickDetail | null;
  /** Popup position, canvas CSS px. */
  screen: { x: number; y: number };
  text: string;
  category: ReviewCategory;
  /** Set when editing an existing annotation. */
  editing?: number;
  /** Set when replying to a top-level annotation. */
  replyTo?: number;
}

export interface ReviewFilters {
  categories: Record<ReviewCategory, boolean>;
  showResolved: boolean;
  text: string;
}

interface ReviewStore {
  /** The engine's annotation set (mirror; refreshed on reviewChanged). */
  annotations: Annotation[];
  /** Review mode: viewport clicks place annotations instead of picking. */
  reviewMode: boolean;
  /** Hide all marker pins (panel stays usable). */
  markersHidden: boolean;
 /** Whether the Review dock panel is present. made this a read-only
   * MIRROR of the dock: the panel's presence is the truth, and the Dock writes
   * this on add/remove. To open or close it, call `setReviewPanelOpen` in
   * `dock/api`, never this. */
  panelOpen: boolean;
  /** The selected annotation (top-level id; drives panel + overlay). */
  selected: number | null;
  draft: ReviewDraft | null;
  /** An annotation waiting for a re-anchor click in the viewport. */
  reanchorTarget: number | null;
  filters: ReviewFilters;
  setAnnotations: (annotations: Annotation[]) => void;
  setReviewMode: (on: boolean) => void;
  setMarkersHidden: (hidden: boolean) => void;
  setPanelOpen: (open: boolean) => void;
  setSelected: (id: number | null) => void;
  setDraft: (draft: ReviewDraft | null) => void;
  setReanchorTarget: (id: number | null) => void;
  setFilters: (filters: Partial<ReviewFilters>) => void;
  toggleCategory: (category: ReviewCategory) => void;
}

const ALL_CATEGORIES: Record<ReviewCategory, boolean> = {
  info: true,
  warning: true,
  question: true,
  change: true,
};

export const useReview = create<ReviewStore>((set) => ({
  annotations: [],
  reviewMode: false,
  markersHidden: false,
  panelOpen: false,
  selected: null,
  draft: null,
  reanchorTarget: null,
  filters: { categories: { ...ALL_CATEGORIES }, showResolved: true, text: "" },
  setAnnotations: (annotations) =>
    set((s) => ({
      annotations,
      // A deleted selection or reanchor target must not linger.
      selected:
        s.selected !== null && annotations.some((a) => a.id === s.selected)
          ? s.selected
          : null,
      reanchorTarget:
        s.reanchorTarget !== null && annotations.some((a) => a.id === s.reanchorTarget)
          ? s.reanchorTarget
          : null,
    })),
  setReviewMode: (on) => set({ reviewMode: on }),
  setMarkersHidden: (hidden) => set({ markersHidden: hidden }),
  setPanelOpen: (open) => set({ panelOpen: open }),
  setSelected: (id) => set({ selected: id }),
  setDraft: (draft) => set({ draft }),
  setReanchorTarget: (id) => set({ reanchorTarget: id }),
  setFilters: (filters) => set((s) => ({ filters: { ...s.filters, ...filters } })),
  toggleCategory: (category) =>
    set((s) => ({
      filters: {
        ...s.filters,
        categories: {
          ...s.filters.categories,
          [category]: !s.filters.categories[category],
        },
      },
    })),
}));

/** The panel's three sections over the filtered top-level annotations. */
export interface ReviewSections {
  open: Annotation[];
  needsReanchor: Annotation[];
  complete: Annotation[];
}

/** Applies the filters and partitions top-level annotations into the
 * panel's Open / Needs re-anchor / Complete sections (desktop parity:
 * staleness trumps the open/complete split; resolved hides behind the
 * toggle but stays hit-testable in the overlay). */
export function sectionAnnotations(
  annotations: Annotation[],
  filters: ReviewFilters,
): ReviewSections {
  const query = filters.text.trim().toLowerCase();
  const visible = annotations.filter((a) => {
    if (a.replyTo !== null && a.replyTo !== undefined) return false;
    if (!filters.categories[a.category]) return false;
    if (a.resolved && !filters.showResolved) return false;
    if (query.length > 0) {
      const replies = annotations.filter((r) => r.replyTo === a.id);
      const haystack = [a.text, a.author ?? "", ...replies.map((r) => r.text)]
        .join("\n")
        .toLowerCase();
      if (!haystack.includes(query)) return false;
    }
    return true;
  });
  return {
    open: visible.filter((a) => !a.resolved && !a.needsReanchor),
    needsReanchor: visible.filter((a) => a.needsReanchor && !a.resolved),
    complete: visible.filter((a) => a.resolved),
  };
}

/** Direct replies of a top-level annotation, in id order. */
export function repliesOf(annotations: Annotation[], id: number): Annotation[] {
  return annotations.filter((a) => a.replyTo === id);
}
