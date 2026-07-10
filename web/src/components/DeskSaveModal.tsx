// Naming dialog for "Save Current Desk As..." (Phase 7b D3). A native
// prompt() would block the event loop; this is the styled equivalent.

import { useState } from "react";
import { useDesks } from "../store/desks";

export function DeskSaveModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const taken = useDesks((s) => s.desks.some((d) => d.name === name.trim()));
  const valid = name.trim().length > 0;

  const save = () => {
    if (!valid) return;
    useDesks.getState().saveCurrent(name);
    onClose();
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Save desk</h3>
        <p className="modal-note">
          Saves the current arrangement (panel docking, sizes, canvas chrome, viewport layout) as
          a named desk.
        </p>
        <input
          className="input-field"
          autoFocus
          placeholder="Desk name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") onClose();
          }}
        />
        {taken && <p className="modal-note">A desk with this name exists; saving replaces it.</p>}
        <div className="modal-actions">
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" disabled={!valid} onClick={save}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
