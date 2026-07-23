// On load, if a prior-session autosave exists, offer to restore it (never
// silently). Discard clears the ring. Shown once per boot.

import { useEffect, useState } from "react";
import { clearAutosaves } from "../persistence/opfs";
import { isBooted, restoreDocument, takeRecovery } from "../engine/session";
import { Modal } from "./Modal";

export function RecoveryPrompt() {
  const [rec, setRec] = useState<{ bytes: Uint8Array; when: number } | null>(null);

  useEffect(() => {
    let cancelled = false;
    const check = async () => {
      // Wait for the engine to boot (recovery is captured during boot).
      while (!isBooted() && !cancelled) {
        await new Promise((r) => setTimeout(r, 80));
      }
      if (cancelled) return;
      const r = takeRecovery();
      if (r) setRec(r);
    };
    void check();
    return () => {
      cancelled = true;
    };
  }, []);

  if (!rec) return null;
  const when = new Date(rec.when).toLocaleTimeString();

  // Deliberately no onClose: the choice is explicit (no Esc, no
  // backdrop dismiss, no X); losing work by a stray click is the one
  // thing this prompt exists to prevent.
  return (
    <Modal title="Recover unsaved work?">
        <p>An autosave from {when} was found. Restore it, or discard and continue.</p>
        <div className="modal-actions">
          <button
            className="btn"
            onClick={async () => {
              await clearAutosaves();
              setRec(null);
            }}
          >
            Discard
          </button>
          <button
            className="btn primary"
            onClick={() => {
              restoreDocument(rec.bytes);
              setRec(null);
            }}
          >
            Restore
          </button>
        </div>
    </Modal>
  );
}
