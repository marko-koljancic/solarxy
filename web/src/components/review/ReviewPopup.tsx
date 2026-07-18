// The floating annotation editor: create (anchored at the picked point),
// edit, or reply. Cmd/Ctrl+Enter saves, Esc cancels. Positioned at the
// draft's canvas point, clamped to the viewport region.

import { useEffect, useRef } from "react";
import {
  addAnnotation,
  editAnnotation,
  replyToAnnotation,
} from "../../engine/session";
import type { ReviewCategory } from "../../engine/types";
import { useReview } from "../../store/review";
import { pushToast } from "../../store/toasts";
import { CATEGORY_LABELS } from "./visuals";
import { Select } from "../Select";

const POPUP_W = 260;
const POPUP_H = 170;

export function ReviewPopup() {
  const draft = useReview((s) => s.draft);
  const setDraft = useReview((s) => s.setDraft);
  const textRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (draft) textRef.current?.focus();
  }, [draft !== null]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!draft) return null;

  const save = () => {
    const text = draft.text.trim();
    if (text.length === 0) return;
    if (draft.editing !== undefined) {
      editAnnotation(draft.editing, text, draft.category);
    } else if (draft.replyTo !== undefined) {
      replyToAnnotation(draft.replyTo, text, draft.category);
    } else if (draft.pick) {
      addAnnotation(draft.pick, text, draft.category);
      pushToast("Annotation added", "info");
    }
    setDraft(null);
  };

  const title =
    draft.editing !== undefined ? "Edit note" : draft.replyTo !== undefined ? "Reply" : "New note";

  // Clamp into the positioning context (the viewport pane).
  const parent = { w: window.innerWidth, h: window.innerHeight };
  const left = Math.max(8, Math.min(draft.screen.x + 12, parent.w - POPUP_W - 8));
  const top = Math.max(8, Math.min(draft.screen.y + 12, parent.h - POPUP_H - 8));

  return (
    <div
      className="review-popup"
      style={{ left, top }}
      onPointerDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") setDraft(null);
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) save();
      }}
    >
      <div className="review-popup-head">
        <span>{title}</span>
        <Select
          ariaLabel="Annotation category"
          value={draft.category}
          options={(Object.keys(CATEGORY_LABELS) as ReviewCategory[]).map((c) => ({
            value: c,
            label: CATEGORY_LABELS[c],
          }))}
          onChange={(c) => setDraft({ ...draft, category: c })}
        />
      </div>
      <textarea
        ref={textRef}
        className="review-popup-text"
        placeholder="Write a note..."
        value={draft.text}
        onChange={(e) => setDraft({ ...draft, text: e.target.value })}
      />
      <div className="review-popup-actions">
        <button className="btn" onClick={() => setDraft(null)}>
          Cancel
        </button>
        <button className="btn primary" disabled={draft.text.trim().length === 0} onClick={save}>
          Save
        </button>
      </div>
    </div>
  );
}
