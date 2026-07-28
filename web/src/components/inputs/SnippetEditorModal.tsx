// The wrangle program in a window: the Houdini "Edit: /path/param" affordance.
//
// The inline field in the parameter panel is fine for a two-line program and
// cramped for anything real. This is the same editor at a size you can work
// in, on the shared `Modal` shell so it drags, resizes and remembers its
// bounds for the session.
//
// Three actions, which is the shape this dialog has everywhere it exists:
// Apply commits and stays, Accept commits and closes, Close abandons. A
// dirty Close asks first -- the whole reason to open a window is that the
// program in it took a while to write.

import { lazy, Suspense, useState } from "react";
import { ConfirmDialog } from "../ConfirmDialog";
import { Modal } from "../Modal";

const CodeEditor = lazy(() => import("./CodeEditor"));

export function SnippetEditorModal({
  value,
  ariaLabel,
  error,
  errorLine,
  path,
  onCommit,
  onClose,
}: {
  value: string;
  ariaLabel: string;
  error?: string;
  errorLine?: number;
  path?: string;
  onCommit: (v: string) => void;
  onClose: () => void;
}) {
  // The window holds its own draft so Apply is a deliberate act. The inline
  // field commits on blur; here, clicking into the scene to look at the
  // result should NOT run a half-written program.
  const [draft, setDraft] = useState(value);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const dirty = draft !== value;

  const apply = () => {
    if (draft !== value) onCommit(draft);
  };

  const close = () => {
    if (dirty) {
      setConfirmDiscard(true);
      return;
    }
    onClose();
  };

  return (
    <Modal
      id="snippet-editor"
      title={path ? `Edit: ${path}` : "Edit program"}
      onClose={close}
      className="modal-wide snippet-editor-modal"
      bodyLayout="column"
      // Esc belongs to the editor inside (revert the line you are on), not
      // to the window: losing a program to a stray Escape is exactly the
      // failure this dialog exists to avoid.
      closeOnEsc={false}
      closeOnBackdrop={false}
      minWidth={520}
      minHeight={360}
    >
      <div className="snippet-editor-body">
        <Suspense fallback={<div className="snippet-loading" aria-busy="true" />}>
          <CodeEditor
            value={draft}
            ariaLabel={ariaLabel}
            errorLine={dirty ? undefined : errorLine}
            minLines={16}
            // The window's editor writes to the local draft; only the
            // buttons below turn that into a command.
            onCommit={setDraft}
          />
        </Suspense>
      </div>
      {error && !dirty && (
        <p className="snippet-error" role="status">
          {error}
        </p>
      )}
      <div className="modal-actions">
        <button className="btn" onClick={close}>
          Close
        </button>
        <button className="btn" disabled={!dirty} onClick={apply}>
          Apply
        </button>
        <button
          className="btn primary"
          onClick={() => {
            apply();
            onClose();
          }}
        >
          Accept
        </button>
      </div>
      {confirmDiscard && (
        <ConfirmDialog
          title="Discard changes"
          message="This program has unapplied changes. Close anyway?"
          confirmLabel="Discard"
          onConfirm={() => {
            setConfirmDiscard(false);
            onClose();
          }}
          onCancel={() => setConfirmDiscard(false)}
        />
      )}
    </Modal>
  );
}
