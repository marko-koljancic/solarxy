// A plain Text param, committed as one edit rather than one per keystroke.
//
// Why this is not just `onChange={commit}`. A rename is an ordinary
// `SetParam` on `name`, and the engine rewrites every expression that
// referenced the old name inside that command's own undo step.
// Committing per keystroke therefore turns typing "control" into seven
// commands, seven document-wide expression rewrites, and seven undo steps
// to get back. That promise is only true if the rename is one command,
// which means the widget has to hold a draft.
//
// The draft contract itself is `useDraftCommit`, shared with every other
// text-like field: Enter and blur commit, Escape abandons, and the stored
// value is authoritative so an undo or a rewrite from elsewhere pulls the
// field back into line.

import { useRef } from "react";
import { useDraftCommit } from "./draftCommit";

interface Props {
  value: string;
  ariaLabel: string;
  onCommit: (v: string) => void;
}

export function TextField({ value, ariaLabel, onCommit }: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const { draft, setDraft, commit, revert } = useDraftCommit(value, onCommit);

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
          revert();
          inputRef.current?.blur();
        }
      }}
    />
  );
}
