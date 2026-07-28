// The draft-commit contract shared by every text-like param field.
//
// **The contract.** One user edit is ONE `SetParam`, and therefore one undo
// step. Committing per keystroke is not merely wasteful: it recooks the
// graph on every character, and for a `name` param it also rewrites every
// expression in the document that referenced the old name, so typing a
// seven-character rename would cost seven commands and seven undo steps to
// get back.
//
// **Why a `sent` ref and not a comparison against `value`.** Enter commits
// and then calls `blur()`, which fires the blur handler. The dispatched
// value has not travelled back through the mirror by then, so a field
// comparing its draft against the STORED prop still sees a difference and
// dispatches the same edit a second time. The cost is not a wasted round
// trip, it is a wrong undo stack: the user presses undo once, pops the
// duplicate, and the document does not move. Comparing against what was
// last SENT makes the second call a no-op.
//
// **Why the stored value wins.** An undo, a redo, a rename the engine
// uniquified, or an edit from another surface (the docked panel while the
// floating one is open) all change the stored value underneath the field,
// and the field must follow rather than hold a stale draft.

import { useCallback, useEffect, useRef, useState } from "react";

/** Whether a draft is a real edit rather than a repeat of the last one.
 *
 * Kept as a separate pure function, not folded into the hook, because it is
 * the only part of this contract that can be tested without a DOM: the web
 * project has no React-rendering test tooling by design. */
export function shouldCommit(draft: string, lastSent: string): boolean {
  return draft !== lastSent;
}

export interface DraftCommit {
  /** The in-flight text. Bind straight to the input's `value`. */
  draft: string;
  /** Bind to `onChange`. Never dispatches. */
  setDraft: (next: string) => void;
  /** Bind to `onBlur` and to whatever key means "done". Dispatches at most
   * once per real edit. */
  commit: () => void;
  /** Bind to Escape. Abandons the draft; dispatches nothing. */
  revert: () => void;
}

/** Holds a draft over a stored string value and commits it exactly once.
 *
 * Callers own their own key handling, because the right key differs per
 * field and that difference is deliberate: Enter commits a single-line
 * field, Enter inserts a newline in a multi-line one and Cmd/Ctrl+Enter
 * commits it instead.
 *
 * Two fields deliberately do NOT use this hook, and neither is an
 * oversight:
 *
 * - `CodeEditor` keeps its draft inside CodeMirror's own document rather
 *   than React state, so there is no `draft` here to own. It implements the
 *   same `sent`-ref contract against `EditorView`.
 * - `flow/NoteNode` opens a modal edit session on double-click and closes
 *   it on commit, so its draft has no persistent stored value to mirror and
 *   needs no `sent` ref.
 */
export function useDraftCommit(
  value: string,
  onCommit: (next: string) => void,
): DraftCommit {
  const [draft, setDraft] = useState(value);
  const sent = useRef(value);

  useEffect(() => {
    setDraft(value);
    sent.current = value;
  }, [value]);

  const commit = useCallback(() => {
    if (!shouldCommit(draft, sent.current)) return;
    sent.current = draft;
    onCommit(draft);
  }, [draft, onCommit]);

  const revert = useCallback(() => setDraft(value), [value]);

  return { draft, setDraft, commit, revert };
}
