// The keyboard shortcuts modal, generated ENTIRELY from the typed keymap
// table (section 16: the table feeds both the dispatcher and this
// modal, preventing the README-vs-code drift Minimystix accumulated).
// No shortcut strings are hardcoded here or anywhere outside keymap.ts.

import { formatKeys, KEY_GROUPS, KEYMAP, type KeyBinding } from "../input/keymap";
import { Modal } from "./Modal";

function Chip({ label }: { label: string }) {
  return <kbd className="key-chip">{label}</kbd>;
}

function Row({ binding }: { binding: KeyBinding }) {
  return (
    <div className="shortcut-row">
      <span className="shortcut-desc">
        {binding.description}
        {binding.context !== "global" && (
          <span className="shortcut-context">{binding.context}</span>
        )}
      </span>
      <span className="shortcut-keys">
        {formatKeys(binding.keys).map((k, i) => (
          <Chip key={i} label={k} />
        ))}
      </span>
    </div>
  );
}

export function ShortcutsModal({ onClose }: { onClose: () => void }) {
  // Dedupe alternate bindings of the same action (undo/redo-alt style):
  // one row per description within a group, extra key sets joined.
  const groups = KEY_GROUPS.map((group) => ({
    group,
    bindings: KEYMAP.filter((b) => b.group === group),
  })).filter((g) => g.bindings.length > 0);

  const notes = KEYMAP.filter((b) => b.note);

  return (
    <Modal id="shortcuts" title="Keyboard Shortcuts" onClose={onClose} className="modal-wide">
        <div className="shortcuts-grid">
          {groups.map(({ group, bindings }) => (
            <div key={group} className="shortcut-group">
              <div className="shortcut-group-title">{group}</div>
              {bindings.map((b) => (
                <Row key={b.id} binding={b} />
              ))}
            </div>
          ))}
        </div>
        {notes.length > 0 && (
          <div className="shortcut-notes">
            {notes.map((b) => (
              <p key={b.id}>
                {formatKeys(b.keys).join("")}: {b.note}
              </p>
            ))}
          </div>
        )}
        <div className="modal-actions">
          <button className="btn primary" onClick={onClose}>
            Done
          </button>
        </div>
    </Modal>
  );
}
