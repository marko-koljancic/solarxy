// A MultilineText param: prose over several lines.
//
// TextField's draft-commit contract, verbatim, with one deliberate
// difference: Enter inserts a newline instead of committing, because a
// multi-line field where Enter ends the edit is a single-line field wearing
// a costume. Blur commits, Escape abandons, and Cmd/Ctrl+Enter commits
// without leaving the field -- the same explicit-commit chord SnippetField
// uses, so the two multi-line editors agree.
//
// The draft matters for the same reason it does in TextField: one edit is
// one `SetParam` and one undo step, not one per keystroke.

import { useEffect, useRef, useState } from "react";
import { shouldCommit } from "./draftCommit";

/** Rows the textarea grows through before it starts scrolling. Three is
 * enough that the field reads as prose at rest; twelve keeps a long note
 * from pushing the rest of the panel off-screen. */
const MIN_ROWS = 3;
const MAX_ROWS = 12;

interface Props {
  value: string;
  ariaLabel: string;
  onCommit: (v: string) => void;
}

export function MultilineField({ value, ariaLabel, onCommit }: Props) {
  const [draft, setDraft] = useState(value);
  const ref = useRef<HTMLTextAreaElement>(null);
  // What we last sent, which is not the same as `value`: a commit followed
  // by a blur runs before the dispatched value has travelled back through
  // the mirror, and comparing against `value` alone would send it twice.
  const sentRef = useRef(value);

  // The stored value wins: undo, redo, or an edit from another surface (the
  // docked panel while the floating one is open) all change it underneath.
  useEffect(() => {
    setDraft(value);
    sentRef.current = value;
  }, [value]);

  const commit = () => {
    if (!shouldCommit(draft, sentRef.current)) return;
    sentRef.current = draft;
    onCommit(draft);
  };

  const rows = Math.min(Math.max(draft.split("\n").length, MIN_ROWS), MAX_ROWS);

  return (
    <textarea
      ref={ref}
      className="input-field multiline-input"
      aria-label={ariaLabel}
      rows={rows}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        // The canvas keymap must never see typing: a description containing
        // the letter "b" would otherwise bypass the node.
        e.stopPropagation();
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
          e.preventDefault();
          commit();
          return;
        }
        if (e.key === "Escape") {
          setDraft(value);
          ref.current?.blur();
        }
      }}
    />
  );
}
