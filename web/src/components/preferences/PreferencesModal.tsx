// The preferences modal, on the Minimystix pattern: a draft
// copy edited locally, dirty star in the title, footer with Reset to
// Defaults (nested confirm) on the left and Cancel / Apply / Save on the
// right. Four tabs per the ratified scope: Appearance, Review, Autosave,
// Screenshot, Viewport. Esc and backdrop-click dismiss.

import { useState } from "react";
import {
  DEFAULT_PREFS,
  usePrefs,
  type BackgroundChoice,
  type MotionChoice,
  type Prefs,
  type GizmoOrientation,
  type SelectionHighlightStyle,
  type ScreenshotResolution,
  type ThemeChoice,
  type WireframeWeight,
} from "../../store/prefs";
import { ConfirmDialog } from "../ConfirmDialog";
import { Modal } from "../Modal";
import { Select } from "../Select";

const TABS = ["Appearance", "Display", "Review", "Autosave", "Screenshot", "Viewport"] as const;
type Tab = (typeof TABS)[number];

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="prefs-row">
      <span className="prefs-label">{label}</span>
      {children}
    </div>
  );
}

function AppearanceTab({ draft, patch }: TabProps) {
  return (
    <>
      <p className="prefs-desc">Theme and motion. System choices follow the OS.</p>
      <Row label="Theme">
        <Select
          ariaLabel="Theme"
          value={draft.appearance.theme}
          options={[
            { value: "dark", label: "Dark" },
            { value: "light", label: "Light" },
            { value: "system", label: "System" },
          ]}
          onChange={(v) => patch({ appearance: { ...draft.appearance, theme: v as ThemeChoice } })}
        />
      </Row>
      <Row label="Reduced motion">
        <Select
          ariaLabel="Reduced motion"
          value={draft.appearance.reducedMotion}
          options={[
            { value: "system", label: "Follow system" },
            { value: "reduce", label: "Reduce" },
            { value: "none", label: "Full motion" },
          ]}
          onChange={(v) =>
            patch({ appearance: { ...draft.appearance, reducedMotion: v as MotionChoice } })
          }
        />
      </Row>
      <div className="prefs-info">
        Reduced motion disables the connection-rejection shake and spinner animations in favor of
        static states.
      </div>
    </>
  );
}

/** Viewport display defaults. Wireframe and background seed every pane;
 * the pane's Display menu stays the live per-pane override and a scene
 * file keeps the per-pane settings it was saved with. */
function DisplayTab({ draft, patch }: TabProps) {
  const d = draft.display;
  const setD = (p: Partial<typeof d>) => patch({ display: { ...d, ...p } });
  return (
    <>
      <p className="prefs-desc">Viewport display defaults.</p>
      <Row label="Wireframe weight">
        <Select
          ariaLabel="Wireframe weight"
          value={d.wireframeWeight}
          options={[
            { value: "Light", label: "Light (1 px)" },
            { value: "Medium", label: "Medium (2 px)" },
            { value: "Bold", label: "Bold (3 px)" },
          ]}
          onChange={(v) => setD({ wireframeWeight: v as WireframeWeight })}
        />
      </Row>
      <Row label="Background">
        <Select
          ariaLabel="Background"
          value={d.background}
          options={[
            { value: "Gradient", label: "Gradient" },
            { value: "White", label: "White" },
            { value: "DarkGray", label: "Dark" },
            { value: "AyuMirage", label: "Ayu" },
            { value: "Black", label: "Black" },
            { value: "HdriSky", label: "HDRI Sky" },
          ]}
          onChange={(v) => setD({ background: v as BackgroundChoice })}
        />
      </Row>
      <Row label="Turntable speed">
        <input
          className="input-field"
          type="number"
          min={1}
          max={60}
          step={1}
          value={d.turntableRpm}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v)) setD({ turntableRpm: Math.min(60, Math.max(1, v)) });
          }}
        />
        <span className="prefs-unit">rpm</span>
      </Row>
      <div className="prefs-info">
        Wireframe and background are the defaults for new scenes and panes; a scene file keeps the
        per-pane settings it was saved with, and the pane&apos;s Display menu overrides the current
        view. The turntable speed applies immediately.
      </div>
    </>
  );
}

function ReviewTab({ draft, patch }: TabProps) {
  return (
    <>
      <p className="prefs-desc">Review annotations.</p>
      <Row label="Author">
        <input
          className="input-field"
          type="text"
          placeholder="Anonymous"
          value={draft.review.author}
          onChange={(e) => patch({ review: { author: e.target.value } })}
        />
      </Row>
      <div className="prefs-info">
        Attribution is opt-in: the name is written to annotations you create and stored in the
        scene file. Leave empty to stay anonymous. Solarxy never derives it from your system.
      </div>
    </>
  );
}

function AutosaveTab({ draft, patch }: TabProps) {
  return (
    <>
      <p className="prefs-desc">Background autosave to browser storage.</p>
      <Row label="Enabled">
        <input
          type="checkbox"
          checked={draft.autosave.enabled}
          onChange={(e) => patch({ autosave: { ...draft.autosave, enabled: e.target.checked } })}
        />
      </Row>
      <Row label="Delay">
        <input
          className="input-field"
          type="number"
          min={1}
          max={60}
          disabled={!draft.autosave.enabled}
          value={draft.autosave.debounceSec}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v)) {
              patch({ autosave: { ...draft.autosave, debounceSec: Math.min(60, Math.max(1, v)) } });
            }
          }}
        />
        <span className="prefs-unit">seconds after the last edit</span>
      </Row>
      <div className="prefs-info">
        While edits keep arriving a save is forced at least every 15 seconds. Recovery is offered
        on the next launch after an unclean exit.
      </div>
    </>
  );
}

function ScreenshotTab({ draft, patch }: TabProps) {
  const sc = draft.screenshot;
  const overlays = sc.overlays;
  const patchSc = (p: Partial<typeof sc>) => patch({ screenshot: { ...sc, ...p } });
  return (
    <>
      <p className="prefs-desc">Defaults for the screenshot dialog.</p>
      <Row label="Resolution">
        <Select
          ariaLabel="Resolution"
          value={sc.resolution}
          options={[
            { value: "viewport", label: "Viewport" },
            { value: "1.5x", label: "1.5x" },
            { value: "2x", label: "2x" },
            { value: "4x", label: "4x" },
            { value: "custom", label: "Custom" },
          ]}
          onChange={(v) => patchSc({ resolution: v as ScreenshotResolution })}
        />
      </Row>
      {sc.resolution === "custom" && (
        <Row label="Size">
          <input
            className="input-field prefs-dim"
            type="number"
            min={16}
            value={sc.customWidth}
            onChange={(e) => patchSc({ customWidth: Number(e.target.value) || sc.customWidth })}
          />
          <span className="prefs-unit">x</span>
          <input
            className="input-field prefs-dim"
            type="number"
            min={16}
            value={sc.customHeight}
            onChange={(e) => patchSc({ customHeight: Number(e.target.value) || sc.customHeight })}
          />
        </Row>
      )}
      {(
        [
          ["grid", "Grid"],
          ["axes", "Axes"],
          ["validation", "Validation overlay"],
        ] as const
      ).map(([key, label]) => (
        <Row key={key} label={label}>
          <input
            type="checkbox"
            checked={overlays[key]}
            onChange={(e) => patchSc({ overlays: { ...overlays, [key]: e.target.checked } })}
          />
        </Row>
      ))}
    </>
  );
}

/** The gizmo's drag ergonomics. These are pushed into the Rust host (which owns
 * the drag loop and never crosses back into JS to ask), so a change here takes
 * effect on the very next drag. */
function ViewportTab({ draft, patch }: TabProps) {
  const v = draft.viewport;
  const setV = (p: Partial<typeof v>) => patch({ viewport: { ...v, ...p } });
  const sel = draft.selection;
  const setSel = (p: Partial<typeof sel>) => patch({ selection: { ...sel, ...p } });

  return (
    <>
      <p className="prefs-desc">
        How selected objects highlight in the 3D view, the transform gizmo orientation, and the
        increments Ctrl-drag snaps to.
      </p>
      <Row label="Selection highlight">
        <Select
          ariaLabel="Selection highlight"
          value={sel.style}
          options={[
            { value: "outline", label: "Outline" },
            { value: "tint", label: "Tint (legacy)" },
            { value: "none", label: "None" },
          ]}
          onChange={(v) => setSel({ style: v as SelectionHighlightStyle })}
        />
      </Row>
      {sel.style !== "none" && (
        <Row label="Highlight color">
          <input
            type="color"
            className="input-field prefs-color"
            value={sel.color}
            onChange={(e) => setSel({ color: e.target.value })}
          />
        </Row>
      )}
      {sel.style === "outline" && (
        <Row label="Outline width (px)">
          <input
            className="input-field"
            type="number"
            min={1}
            max={16}
            step={1}
            value={sel.width}
            onChange={(e) =>
              setSel({ width: Math.min(16, Math.max(1, Number(e.target.value) || 3)) })
            }
          />
        </Row>
      )}
      <Row label="Handle orientation">
        <Select
          ariaLabel="Handle orientation"
          value={v.orientation}
          options={[
            { value: "world", label: "World axes" },
            { value: "local", label: "Object axes" },
          ]}
          onChange={(o) => setV({ orientation: o as GizmoOrientation })}
        />
      </Row>
      <Row label="Snap: move (m)">
        <input
          className="input-field"
          type="number"
          min={0}
          step={0.1}
          value={v.snapTranslate}
          onChange={(e) => setV({ snapTranslate: Math.max(0, Number(e.target.value) || 0) })}
        />
      </Row>
      <Row label="Snap: rotate (deg)">
        <input
          className="input-field"
          type="number"
          min={0}
          step={1}
          value={v.snapRotate}
          onChange={(e) => setV({ snapRotate: Math.max(0, Number(e.target.value) || 0) })}
        />
      </Row>
      <Row label="Snap: scale">
        <input
          className="input-field"
          type="number"
          min={0}
          step={0.05}
          value={v.snapScale}
          onChange={(e) => setV({ snapScale: Math.max(0, Number(e.target.value) || 0) })}
        />
      </Row>
      <div className="prefs-info">
        Hold Ctrl (or Cmd) while dragging a handle to snap. A step of 0 disables snapping for that
        tool. The Scale tool always uses the object's own axes: a world-axis scale on a rotated
        object would shear it, and there is no shear parameter.
      </div>
    </>
  );
}

interface TabProps {
  draft: Prefs;
  patch: (p: Partial<Prefs>) => void;
}

export function PreferencesModal({ onClose }: { onClose: () => void }) {
  const saved = usePrefs((s) => s.prefs);
  const [draft, setDraft] = useState<Prefs>(() => JSON.parse(JSON.stringify(saved)) as Prefs);
  const [tab, setTab] = useState<Tab>("Appearance");
  const [confirmReset, setConfirmReset] = useState(false);
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);

  const patch = (p: Partial<Prefs>) => setDraft((d) => ({ ...d, ...p }));
  const apply = () => usePrefs.getState().setPrefs(draft);

  const body =
    tab === "Appearance" ? (
      <AppearanceTab draft={draft} patch={patch} />
    ) : tab === "Display" ? (
      <DisplayTab draft={draft} patch={patch} />
    ) : tab === "Review" ? (
      <ReviewTab draft={draft} patch={patch} />
    ) : tab === "Autosave" ? (
      <AutosaveTab draft={draft} patch={patch} />
    ) : tab === "Screenshot" ? (
      <ScreenshotTab draft={draft} patch={patch} />
    ) : (
      <ViewportTab draft={draft} patch={patch} />
    );

  return (
    <Modal
      id="preferences"
      title={`Preferences${dirty ? " *" : ""}`}
      onClose={onClose}
      className="modal-prefs"
    >
        <div className="prefs-tabs">
          {TABS.map((t) => (
            <button
              key={t}
              className={`prefs-tab${tab === t ? " active" : ""}`}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ))}
        </div>
        <div className="prefs-body">{body}</div>
        <div className="prefs-footer">
          <button className="btn" onClick={() => setConfirmReset(true)}>
            Reset to Defaults
          </button>
          <div className="prefs-footer-right">
            <button className="btn" onClick={onClose}>
              Cancel
            </button>
            <button className="btn" disabled={!dirty} onClick={apply}>
              Apply
            </button>
            <button
              className="btn primary"
              disabled={!dirty}
              onClick={() => {
                apply();
                onClose();
              }}
            >
              Save
            </button>
          </div>
        </div>
        {confirmReset && (
          <ConfirmDialog
            title="Reset preferences"
            message="Reset all preferences to their defaults? This cannot be undone."
            confirmLabel="Reset"
            onConfirm={() => {
              setConfirmReset(false);
              setDraft(JSON.parse(JSON.stringify(DEFAULT_PREFS)) as Prefs);
            }}
            onCancel={() => setConfirmReset(false)}
          />
        )}
    </Modal>
  );
}
