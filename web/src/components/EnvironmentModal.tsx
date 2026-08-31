// The environment panel (View menu): HDRI load/clear, the IBL contribution
// mode, and the scene-global HDRI rotation. The host owns the truth; this
// modal mirrors `EnvironmentState` from the view-state store and mutates
// through the session actions. The per-pane HDRI-sky background lives on
// each pane's toolbar background select.

import { useRef, useState } from "react";
import {
  clearEnvironment,
  loadHdri,
  setDisplaySettings,
  setIblMode,
} from "../engine/session";
import { pushToast } from "../store/toasts";
import { useViewState } from "../store/viewState";
import { Modal } from "./Modal";
import { Row } from "./DialogRow";
import { Select } from "./Select";

const IBL_MODES: [string, string][] = [
  ["off", "Off"],
  ["diffuse", "Diffuse only"],
  ["full", "Full"],
];

export function EnvironmentModal({ onClose }: { onClose: () => void }) {
  const env = useViewState((s) => s.environment);
  const view = useViewState((s) => s.view);
  const fileRef = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);

  const rotationDeg = view ? (view.display.hdriRotation * 180) / Math.PI : 0;
  const intensity = view ? view.display.hdriIntensity : 1;

  const pick = async (file: File) => {
    setBusy(true);
    try {
      await loadHdri(file);
    } catch (err) {
      pushToast(`HDRI load failed: ${err instanceof Error ? err.message : err}`, "error");
    } finally {
      setBusy(false);
    }
  };

  // No Esc close historically (the modal predates the shared shell's Esc
  // handling); backdrop and Done still dismiss.
  return (
    <Modal
      id="environment"
      title="Environment"
      onClose={onClose}
      closeOnEsc={false}
      footer={
        <div className="modal-actions">
          <button className="btn primary" onClick={onClose}>
            Done
          </button>
        </div>
      }
    >
        <Row
          label="HDRI"
          doc="The high-dynamic-range image that lights the scene. It supplies both the ambient light and the reflections, so loading one changes the look of every material at once. Embedded in the scene file, so it travels with it."
        >
          <span className="env-value">
            {busy ? "preparing..." : (env?.hdriName ?? (env?.hdriHash ? "embedded" : "None"))}
          </span>
          <button className="btn" disabled={busy} onClick={() => fileRef.current?.click()}>
            Load...
          </button>
          <button
            className="btn"
            disabled={busy || !env?.hdriHash}
            onClick={() => clearEnvironment()}
          >
            Clear
          </button>
        </Row>
        <Row
          label="IBL mode"
          doc="How much of the HDRI reaches the shading. **Full** uses it for both ambient light and reflections; **Diffuse only** drops the reflections, which is cheaper and calmer on rough surfaces; **Off** ignores the image entirely and lights from the scene lights alone."
        >
          <Select
            ariaLabel="IBL mode"
            value={env?.iblMode ?? "full"}
            options={IBL_MODES.map(([v, label]) => ({ value: v, label }))}
            onChange={(v) => setIblMode(v)}
          />
        </Row>
        <Row
          label="Rotation"
          doc="Spins the environment around the vertical axis, in degrees. Moves the visible sky and its lighting together, so it is how you place a highlight without moving a light."
        >
          <input
            className="input-field"
            type="number"
            step={5}
            value={Math.round(rotationDeg)}
            onChange={(e) => {
              if (!view) return;
              const deg = Number(e.target.value);
              if (!Number.isFinite(deg)) return;
              setDisplaySettings({ ...view.display, hdriRotation: (deg * Math.PI) / 180 });
            }}
          />
          <span className="prefs-unit">degrees</span>
        </Row>
        <Row
          label="Intensity"
          doc="Scales how much light the environment casts, leaving the visible sky alone. Use it to keep a backdrop readable while dialling the key it throws up or down; 1 is the image as it was authored."
        >
          <input
            className="input-field"
            type="number"
            step={0.1}
            min={0}
            max={8}
            value={intensity}
            onChange={(e) => {
              if (!view) return;
              const next = Number(e.target.value);
              if (!Number.isFinite(next)) return;
              setDisplaySettings({ ...view.display, hdriIntensity: next });
            }}
          />
        </Row>
        <p className="env-hint">
          Show the HDRI as a pane background via the pane toolbar's background select (Sky).
        </p>
        <input
          ref={fileRef}
          type="file"
          accept=".hdr,.exr"
          style={{ display: "none" }}
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) void pick(file);
            e.target.value = "";
          }}
        />
    </Modal>
  );
}
