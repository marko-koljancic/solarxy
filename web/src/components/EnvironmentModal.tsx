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

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Environment</h3>
        <div className="env-row">
          <span className="env-label">HDRI</span>
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
        </div>
        <div className="env-row">
          <span className="env-label">IBL mode</span>
          <select
            className="input-field"
            value={env?.iblMode ?? "full"}
            onChange={(e) => setIblMode(e.target.value)}
          >
            {IBL_MODES.map(([v, label]) => (
              <option key={v} value={v}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <div className="env-row">
          <span className="env-label">Rotation</span>
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
          <span className="env-hint">degrees; rotates the visible sky and IBL</span>
        </div>
        <p className="env-hint">
          Show the HDRI as a pane background via the pane toolbar's background select (Sky).
        </p>
        <div className="modal-actions">
          <button className="btn primary" onClick={onClose}>
            Done
          </button>
        </div>
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
      </div>
    </div>
  );
}
