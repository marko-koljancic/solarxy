// The review side panel (desktop review_panel parity, Minimystix skin):
// category filter chips, text search, show-resolved toggle, then the
// Open / Needs re-anchor / Complete sections. The selected annotation
// expands an inline editor with Reply / Re-place / Delete and a Complete
// checkbox; replies list indented under their parent.

import { useState } from "react";
import { deleteAnnotation, resolveAnnotation } from "../../engine/session";
import type { Annotation, ReviewCategory } from "../../engine/types";
import { repliesOf, sectionAnnotations, useReview } from "../../store/review";
import { pushToast } from "../../store/toasts";
import { ConfirmDialog } from "../ConfirmDialog";
import { CATEGORY_GLYPHS, CATEGORY_LABELS, relativeTime, shortPreview } from "./visuals";

function Row({ a, replies }: { a: Annotation; replies: Annotation[] }) {
  const selected = useReview((s) => s.selected === a.id);
  const setSelected = useReview((s) => s.setSelected);
  const setDraft = useReview((s) => s.setDraft);
  const setReanchorTarget = useReview((s) => s.setReanchorTarget);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const centerScreen = () => ({ x: window.innerWidth * 0.35, y: window.innerHeight * 0.3 });

  return (
    <div className={`review-row cat-${a.category}${selected ? " selected" : ""}`}>
      <button
        className="review-row-head"
        onClick={() => setSelected(selected ? null : a.id)}
      >
        <span className={`review-chip cat-${a.category}`}>{CATEGORY_GLYPHS[a.category]}</span>
        <span className="review-row-text">{shortPreview(a.text)}</span>
        <span className="review-row-meta">
          {replies.length > 0 ? `${replies.length} · ` : ""}
          {relativeTime(a.updatedAt || a.createdAt)}
        </span>
      </button>
      {selected && (
        <div className="review-row-body">
          <div className="review-row-full">{a.text}</div>
          <div className="review-row-byline">
            {a.author ?? "Anonymous"} · created {relativeTime(a.createdAt)}
          </div>
          {replies.map((r) => (
            <div key={r.id} className="review-reply">
              <div className="review-row-byline">
                {r.author ?? "Anonymous"} · {relativeTime(r.createdAt)}
              </div>
              <div>{r.text}</div>
            </div>
          ))}
          <div className="review-row-actions">
            <label className="review-complete">
              <input
                type="checkbox"
                checked={a.resolved}
                onChange={(e) => resolveAnnotation(a.id, e.target.checked)}
              />
              Complete
            </label>
            <button
              className="btn"
              onClick={() =>
                setDraft({
                  pick: null,
                  screen: centerScreen(),
                  text: "",
                  category: a.category,
                  replyTo: a.id,
                })
              }
            >
              Reply
            </button>
            <button
              className="btn"
              onClick={() =>
                setDraft({
                  pick: null,
                  screen: centerScreen(),
                  text: a.text,
                  category: a.category,
                  editing: a.id,
                })
              }
            >
              Edit
            </button>
            <button
              className="btn"
              onClick={() => {
                setReanchorTarget(a.id);
                pushToast("Click geometry to re-place the marker (Esc cancels)", "info");
              }}
            >
              Re-place
            </button>
            <button className="btn danger" onClick={() => setConfirmDelete(true)}>
              Delete
            </button>
          </div>
        </div>
      )}
      {confirmDelete && (
        <ConfirmDialog
          title="Delete note"
          message={
            replies.length > 0
              ? `Delete this note and its ${replies.length} repl${replies.length === 1 ? "y" : "ies"}?`
              : "Delete this note?"
          }
          confirmLabel="Delete"
          onConfirm={() => {
            setConfirmDelete(false);
            deleteAnnotation(a.id);
          }}
          onCancel={() => setConfirmDelete(false)}
        />
      )}
    </div>
  );
}

function Section({ title, items, all }: { title: string; items: Annotation[]; all: Annotation[] }) {
  if (items.length === 0) return null;
  return (
    <div className="review-section">
      <div className="review-section-title">
        {title} <span className="review-section-count">{items.length}</span>
      </div>
      {items.map((a) => (
        <Row key={a.id} a={a} replies={repliesOf(all, a.id)} />
      ))}
    </div>
  );
}

/** The Review panel. promoted it from a canvas overlay drawer to a real
 * dock panel, so its presence in the dock IS its open state: dockview's tab owns
 * the title and the close button, and N adds or removes the panel. */
export function ReviewPanel() {
  const annotations = useReview((s) => s.annotations);
  const filters = useReview((s) => s.filters);
  const setFilters = useReview((s) => s.setFilters);
  const toggleCategory = useReview((s) => s.toggleCategory);
  const reviewMode = useReview((s) => s.reviewMode);

  const sections = sectionAnnotations(annotations, filters);
  const empty = annotations.length === 0;

  return (
    <div className="review-panel">
      <div className="review-panel-filters">
        {(Object.keys(CATEGORY_LABELS) as ReviewCategory[]).map((c) => (
          <button
            key={c}
            className={`review-chip cat-${c}${filters.categories[c] ? " on" : ""}`}
            title={CATEGORY_LABELS[c]}
            onClick={() => toggleCategory(c)}
          >
            {CATEGORY_GLYPHS[c]}
          </button>
        ))}
        <input
          className="input-field review-search"
          placeholder="Filter..."
          value={filters.text}
          onChange={(e) => setFilters({ text: e.target.value })}
        />
        <label className="review-complete" title="Show resolved">
          <input
            type="checkbox"
            checked={filters.showResolved}
            onChange={(e) => setFilters({ showResolved: e.target.checked })}
          />
          Done
        </label>
      </div>
      <div className="review-panel-body">
        {empty ? (
          <div className="review-empty">
            No notes yet. {reviewMode ? "Click geometry to pin one." : "Shift+R, then click geometry."}
          </div>
        ) : (
          <>
            <Section title="Needs re-anchor" items={sections.needsReanchor} all={annotations} />
            <Section title="Open" items={sections.open} all={annotations} />
            <Section title="Complete" items={sections.complete} all={annotations} />
          </>
        )}
      </div>
    </div>
  );
}
