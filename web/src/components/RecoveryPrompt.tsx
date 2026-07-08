// On load, if a prior-session autosave exists, offer to restore it (never
// silently). Discard clears the ring. Shown once per boot.

import { useEffect, useState } from "react";
import { clearAutosaves } from "../persistence/opfs";
import { isBooted, restoreDocument, takeRecovery } from "../engine/session";

export function RecoveryPrompt() {
  const [rec, setRec] = useState<{ json: string; when: number } | null>(null);

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

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <h3>Recover unsaved work?</h3>
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
              restoreDocument(rec.json);
              setRec(null);
            }}
          >
            Restore
          </button>
        </div>
      </div>
    </div>
  );
}
