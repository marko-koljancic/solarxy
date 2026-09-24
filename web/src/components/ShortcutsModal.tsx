// The keyboard shortcuts modal, generated ENTIRELY from the typed keymap
// table (section 16: the table feeds both the dispatcher and this
// modal, preventing the README-vs-code drift Minimystix accumulated).
// No shortcut strings are hardcoded here or anywhere outside keymap.ts.

import { formatKeys, KEY_GROUPS, KEYMAP, type KeyBinding, type KeyGroup } from "../input/keymap";
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

/** The sections this reference lists, in KEY_GROUPS order, each holding the
 * bindings that declare it, with empty groups dropped.
 *
 * Pure and exported so the suite can hold it to the table without a DOM:
 * the modal renders exactly this and nothing else, which is the claim worth
 * testing. It was inline here, so nothing said a binding could not quietly
 * stop being listed. */
export function shortcutGroups(): { group: KeyGroup; bindings: KeyBinding[] }[] {
  return KEY_GROUPS.map((group) => ({
    group,
    bindings: KEYMAP.filter((b) => b.group === group),
  })).filter((g) => g.bindings.length > 0);
}

/** The footnotes under the sections: every binding carrying a note. */
export function shortcutNotes(): KeyBinding[] {
  return KEYMAP.filter((b) => b.note);
}

export function ShortcutsModal({ onClose }: { onClose: () => void }) {
  const groups = shortcutGroups();
  const notes = shortcutNotes();

  return (
    <Modal
      id="shortcuts"
      title="Keyboard Shortcuts"
      onClose={onClose}
      className="modal-wide"
      footer={
        <div className="modal-actions">
          <button className="btn primary" onClick={onClose}>
            Done
          </button>
        </div>
      }
    >
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
    </Modal>
  );
}
