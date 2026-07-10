// The About dialog: name, version, a short positioning line, and the
// project links (repository + wiki). Esc / backdrop / Done dismiss.

import { useEffect } from "react";

const REPO_URL = "https://github.com/marko-koljancic/solarxy";
const WIKI_URL = "https://github.com/marko-koljancic/solarxy/wiki";

export function AboutModal({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Solarxy Web</h3>
        <p>
          A WebGPU node-based parametric modeler with production-grade model
          inspection, validation, and review, on the shared Solarxy core.
        </p>
        <p className="about-version">Version 0.7.0 (pre-beta)</p>
        <p>
          <a href={REPO_URL} target="_blank" rel="noreferrer">
            GitHub repository
          </a>
          {" · "}
          <a href={WIKI_URL} target="_blank" rel="noreferrer">
            Documentation wiki
          </a>
        </p>
        <div className="modal-actions">
          <button className="btn primary" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
