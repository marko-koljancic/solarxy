// "Export web bundle...": publishes the scene as a folder that runs on any
// static host.
//
// The options here are what the PAGE does with the scene, not what the scene
// means. fps, the frame range and loop mode live in the `.slxy` and are
// edited on the transport bar; autoplay is a document setting this dialog can
// only ever turn off. Keeping that line clear is what stops "the exported
// copy behaves differently" from becoming a class of bug.

import { useState } from "react";
import { buildWebBundle } from "../export/webBundle";
import { DEFAULT_PLAYER_CONFIG } from "../export/playerConfig";
import { buildSaveExtra, getClient } from "../engine/session";
import { saveExportToFile } from "../persistence/opfs";
import { pushToast } from "../store/toasts";
import { useMirror } from "../store/mirror";
import { Modal } from "./Modal";
import { Row, Section } from "./DialogRow";

function formatSize(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function WebBundleModal({ onClose }: { onClose: () => void }) {
  const runtime = useMirror((s) => s.runtime);
  const [transport, setTransport] = useState(DEFAULT_PLAYER_CONFIG.transport);
  const [autoplay, setAutoplay] = useState(DEFAULT_PLAYER_CONFIG.autoplay);
  const [turntable, setTurntable] = useState(DEFAULT_PLAYER_CONFIG.turntable);
  const [background, setBackground] = useState(DEFAULT_PLAYER_CONFIG.background);
  const [busy, setBusy] = useState(false);

  const exportBundle = async () => {
    setBusy(true);
    try {
      const scene = getClient().saveSlxy(buildSaveExtra());
      const { zip, files } = await buildWebBundle({
        scene,
        config: { transport, autoplay, turntable, background },
      });
      await saveExportToFile(zip, "solarxy-scene-bundle.zip", "application/zip");
      pushToast(`Exported ${files.length} files, ${formatSize(zip.length)} zipped`);
      onClose();
    } catch (e) {
      // The dev-build case has its own sentence in the thrown message, which
      // is more useful than a generic failure.
      pushToast(e instanceof Error ? e.message : "The bundle could not be built", "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal id="web-bundle" title="Export web bundle" onClose={onClose} minWidth={420}>
      <p className="modal-note">
        Writes a zip containing your scene and the Solarxy engine. Unzip it onto any static
        host and the scene runs there, with no install and no account. It carries the engine
        rather than a recording, so anything driven by an expression or a wrangle keeps
        computing.
      </p>

      <Section title="Playback">
        <Row
          label="Allow autoplay"
          doc={
            runtime.autoplay
              ? "Whether the published page starts its clock on load. This scene is set to autoplay, so unchecking stops the published page doing so."
              : "Whether the published page starts its clock on load. This scene does not autoplay, so this changes nothing until you turn autoplay on for the scene itself."
          }
        >
          <input
            type="checkbox"
            checked={autoplay}
            onChange={(e) => setAutoplay(e.target.checked)}
          />
        </Row>
        <Row
          label="Show playback controls"
          doc="Whether a visitor gets play, pause and a frame scrubber. Off by default: most published scenes are meant to be watched rather than driven."
        >
          <input
            type="checkbox"
            checked={transport}
            onChange={(e) => setTransport(e.target.checked)}
          />
        </Row>
        <Row
          label="Turntable"
          doc="Spins the camera around the scene on the published page. A camera effect rather than scene time, so it works whether or not the scene animates."
        >
          <input
            type="checkbox"
            checked={turntable}
            onChange={(e) => setTurntable(e.target.checked)}
          />
        </Row>
      </Section>

      <Section title="Appearance">
        <Row
          label="Page background"
          doc="The colour behind the canvas on the published page. Matches the bundle to the site you are embedding it in; it does not change the scene's own viewport background."
        >
          <input
            type="color"
            value={background}
            onChange={(e) => setBackground(e.target.value)}
            aria-label="Page background"
          />
        </Row>
      </Section>

      <p className="modal-note">
        The bundle must be served over HTTP. Opening its index.html straight from disk will
        not work, because browsers refuse to load an ES module or fetch wasm from a file
        address. The quickest local check, from inside the unzipped folder:{" "}
        <code>python3 -m http.server 8000</code>, then open localhost:8000. The included
        README says so too.
      </p>

      <div className="modal-actions">
        <button className="btn" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button className="btn btn-primary" onClick={() => void exportBundle()} disabled={busy}>
          {busy ? "Building..." : "Export"}
        </button>
      </div>
    </Modal>
  );
}
