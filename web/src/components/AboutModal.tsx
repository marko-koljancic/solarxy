// The About dialog: name, version, a short positioning line, the project
// links (repository + wiki), and a discrete copyright. Esc / backdrop /
// Done dismiss. The version is the build-time package version
// (__APP_VERSION__, single-sourced from package.json by vite.config.ts),
// so a release bump reaches this dialog with no edit here.

import { Modal } from "./Modal";

const REPO_URL = "https://github.com/marko-koljancic/solarxy";
const WIKI_URL = "https://github.com/marko-koljancic/solarxy/wiki";

/** The dialog's version line, exported for tests. */
export function aboutVersionLine(): string {
  return `Version ${__APP_VERSION__}`;
}

/** The dialog's copyright line, exported for tests. */
export function aboutCopyrightLine(now = new Date()): string {
  return `© ${now.getFullYear()} Marko Koljancic`;
}

export function AboutModal({ onClose }: { onClose: () => void }) {
  return (
    <Modal id="about" title="Solarxy Web" onClose={onClose}>
      <p>
        A WebGPU node-based parametric modeler with production-grade model
        inspection, validation, and review, on the shared Solarxy core.
      </p>
      <p className="about-version">{aboutVersionLine()}</p>
      <p>
        <a href={REPO_URL} target="_blank" rel="noreferrer">
          GitHub repository
        </a>
        {" · "}
        <a href={WIKI_URL} target="_blank" rel="noreferrer">
          Documentation wiki
        </a>
      </p>
      <p className="about-copyright">{aboutCopyrightLine()}</p>
      <div className="modal-actions">
        <button className="btn primary" onClick={onClose}>
          Done
        </button>
      </div>
    </Modal>
  );
}
