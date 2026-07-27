// A plain Text param, committed as one edit rather than one per keystroke.
//
// Why this is not just `onChange={commit}`. A rename is an ordinary
// `SetParam` on `name`, and the engine rewrites every expression that
// referenced the old name inside that command's own undo step (decision
// M-7). Committing per keystroke therefore turns typing "control" into
// seven commands, seven document-wide expression rewrites, and seven undo
// steps to get back. M-7's promise is only true if the rename is one
// command, which means the widget has to hold a draft.
//
// Same contract as ExpressionField and NoteNode, which already work this
// way: Enter and blur commit, Escape abandons, and the stored value is
// authoritative so an undo or a rewrite from elsewhere pulls the field
// back into line.

import { useEffect, useRef, useState } from "react";
import { shouldCommit } from "./draftCommit";

interface Props {
  value: string;
  ariaLabel: string;
  onCommit: (v: string) => void;
}

export function TextField({ value, ariaLabel, onCommit }: Props) {
  const [draft, setDraft] = useState(value);
  const inputRef = useRef<HTMLInputElement>(null);
  // What we last sent, which is NOT the same as `value`. Enter commits and
  // then blurs, and the blur handler runs before the dispatched value has
  // travelled back through the mirror, so comparing against `value` alone
  // sends the same edit twice and costs the user a second undo step to get
  // past a no-op.
  const sentRef = useRef(value);

  // The stored value wins: an undo, a redo, or a name the engine
  // uniquified out from under us all change it, and the field must follow.
  useEffect(() => {
    setDraft(value);
    sentRef.current = value;
  }, [value]);

  const commit = () => {
    if (!shouldCommit(draft, sentRef.current)) return;
    sentRef.current = draft;
    onCommit(draft);
  };

  return (
    <input
      ref={inputRef}
      type="text"
      className="input-field text-input"
      aria-label={ariaLabel}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") {
          commit();
          inputRef.current?.blur();
        }
        if (e.key === "Escape") {
          setDraft(value);
          inputRef.current?.blur();
        }
      }}
    />
  );
}
