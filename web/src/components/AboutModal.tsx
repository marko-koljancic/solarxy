// The About dialog: name, version, a short positioning line, the project
// links (repository + wiki), the license with a source offer, and a discrete
// copyright. Esc / backdrop / Done dismiss. The version is the build-time
// package version
// (__APP_VERSION__, single-sourced from package.json by vite.config.ts),
// so a release bump reaches this dialog with no edit here.
//
// The doc links are the four capability pages rather than a generic "read
// the manual": this dialog is where somebody lands when they want to know
// what the app is, and naming the capabilities answers that better than a
// paragraph can.

import { Modal } from "./Modal";

const REPO_URL = "https://github.com/marko-koljancic/solarxy";
const WIKI_URL = "https://github.com/marko-koljancic/solarxy/wiki";

/** Both are copied out of the repository root by the prebuild step, so they
 * are served by the app itself rather than linked off-site. That matters: a
 * visitor is handed compiled WebAssembly, and the license has to arrive with
 * it rather than live somewhere the visitor is trusted to go looking. */
const LICENSE_URL = "/LICENSE.txt";
const NOTICES_URL = "/THIRD-PARTY-NOTICES.md";

/** The capability pages, in the order a new user meets them. Exported for
 * tests, which check that every href resolves under the wiki. */
export const DOC_LINKS: ReadonlyArray<{ label: string; page: string }> = [
  { label: "Expressions", page: "Expressions" },
  { label: "Attribute wrangle", page: "Attribute-Wrangle" },
  { label: "Runtime and playback", page: "Runtime-And-Playback" },
  { label: "Publishing a scene", page: "Publishing-A-Scene" },
];

/** The dialog's version line, exported for tests. */
export function aboutVersionLine(): string {
  return `Version ${__APP_VERSION__}`;
}

/** The dialog's copyright line, exported for tests. */
export function aboutCopyrightLine(now = new Date()): string {
  return `© ${now.getFullYear()} Marko Koljancic`;
}

/** A wiki page's full URL. */
export function docUrl(page: string): string {
  return `${WIKI_URL}/${page}`;
}

/** Source for the build that is running, pinned to its release tag rather
 * than the default branch. The obligation is to offer the source that
 * corresponds to THIS binary, and `main` stops corresponding the moment it
 * moves. Exported for tests. */
export function aboutSourceUrl(version = __APP_VERSION__): string {
  return `${REPO_URL}/tree/v${version}`;
}

export function AboutModal({ onClose }: { onClose: () => void }) {
  return (
    <Modal id="about" title="Solarxy Web" onClose={onClose}>
      <p>
        A WebGPU node-based parametric modeler. Build geometry with a typed
        node graph, drive any number with an expression, run a short program
        on every point, play the scene on a clock, and publish it to a URL
        that carries the engine rather than a recording. The full Solarxy
        inspection, validation and review toolset comes with it, on the same
        Rust core as the desktop app.
      </p>
      <p>
        A public beta. Everything runs in your browser: nothing is uploaded,
        and no account is needed.
      </p>
      <p className="about-docs">
        {DOC_LINKS.map(({ label, page }, i) => (
          <span key={page}>
            {i > 0 && " · "}
            <a href={docUrl(page)} target="_blank" rel="noreferrer">
              {label}
            </a>
          </span>
        ))}
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
      <p className="about-license">
        Free software under the{" "}
        <a href={LICENSE_URL} target="_blank" rel="noreferrer">
          GNU GPL, version 3 or later
        </a>
        . Use it, study it, share it, change it.{" "}
        <a href={aboutSourceUrl()} target="_blank" rel="noreferrer">
          Source for this build
        </a>
        {" · "}
        <a href={NOTICES_URL} target="_blank" rel="noreferrer">
          Third-party notices
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
