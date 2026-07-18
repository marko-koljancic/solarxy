// A single-line inline text editor (rename), extracted from the
// NoteNode editing pattern: draft state, focus-and-select on open, commit
// on blur or Enter, revert on Esc, keydown propagation stopped so the
// canvas and global keymaps never see the typing. The `nodrag` class keeps
// React Flow from dragging the node under the caret.

import { useEffect, useRef, useState } from "react";

export function InlineEdit({
  value,
  placeholder,
  className,
  onCommit,
  onClose,
}: {
  /** The current committed value the editor opens with (and reverts to). */
  value: string;
  placeholder?: string;
  className?: string;
  /** Called with the trimmed draft when it differs from `value`. */
  onCommit: (next: string) => void;
  /** Called after every close, committed or reverted. */
  onClose: () => void;
}) {
  const [draft, setDraft] = useState(value);
  const inputRef = useRef<HTMLInputElement>(null);
  // Guards the unmount blur: closing the editor (Esc or Enter) removes the
  // focused input, which fires a native blur that would otherwise commit a
  // reverted draft or commit twice.
  const closed = useRef(false);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const close = () => {
    if (closed.current) return;
    closed.current = true;
    onClose();
  };

  const commit = () => {
    if (closed.current) return;
    const next = draft.trim();
    if (next !== value) onCommit(next);
    close();
  };

  return (
    <input
      ref={inputRef}
      type="text"
      className={`inline-edit nodrag${className ? ` ${className}` : ""}`}
      placeholder={placeholder}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onPointerDown={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") close();
        if (e.key === "Enter") commit();
      }}
    />
  );
}
