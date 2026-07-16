// The preferences modal (Phase 7 W4), on the Minimystix pattern: a draft
// copy edited locally, dirty star in the title, footer with Reset to
// Defaults (nested confirm) on the left and Cancel / Apply / Save on the
// right. Four tabs per the ratified scope: Appearance, Review, Autosave,
// Screenshot, Viewport. Esc and backdrop-click dismiss.

import { useEffect, useState } from "react";
import {
  DEFAULT_PREFS,
  usePrefs,
  type MotionChoice,
  type Prefs,
  type GizmoOrientation,
  type ScreenshotResolution,
  type ThemeChoice,
} from "../../store/prefs";
import { ConfirmDialog } from "../ConfirmDialog";

const TABS = ["Appearance", "Review", "Autosave", "Screenshot", "Viewport"] as const;
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
        <select
          className="input-field"
          value={draft.appearance.theme}
          onChange={(e) =>
            patch({ appearance: { ...draft.appearance, theme: e.target.value as ThemeChoice } })
          }
        >
          <option value="dark">Dark</option>
          <option value="light">Light</option>
          <option value="mpw">MPW Light</option>
          <option value="system">System</option>
        </select>
      </Row>
      <Row label="Reduced motion">
        <select
          className="input-field"
          value={draft.appearance.reducedMotion}
          onChange={(e) =>
            patch({
              appearance: {
                ...draft.appearance,
                reducedMotion: e.target.value as MotionChoice,
              },
            })
          }
        >
          <option value="system">Follow system</option>
          <option value="reduce">Reduce</option>
          <option value="none">Full motion</option>
        </select>
      </Row>
      <div className="prefs-info">
        Reduced motion disables the connection-rejection shake and spinner animations in favor of
        static states.
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
        <select
          className="input-field"
          value={sc.resolution}
          onChange={(e) => patchSc({ resolution: e.target.value as ScreenshotResolution })}
        >
          <option value="viewport">Viewport</option>
          <option value="1.5x">1.5x</option>
          <option value="2x">2x</option>
          <option value="4x">4x</option>
          <option value="custom">Custom</option>
        </select>
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

  return (
    <>
      <p className="prefs-desc">
        Transform gizmo orientation and the increments Ctrl-drag snaps to.
      </p>
      <Row label="Handle orientation">
        <select
          className="input-field"
          value={v.orientation}
          onChange={(e) => setV({ orientation: e.target.value as GizmoOrientation })}
        >
          <option value="world">World axes</option>
          <option value="local">Object axes</option>
        </select>
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

  const patch = (p: Partial<Prefs>) => setDraft((d) => ({ ...d, ...p }));
  const apply = () => usePrefs.getState().setPrefs(draft);

  const body =
    tab === "Appearance" ? (
      <AppearanceTab draft={draft} patch={patch} />
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
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal modal-prefs" onClick={(e) => e.stopPropagation()}>
        <h3>Preferences{dirty ? " *" : ""}</h3>
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
      </div>
    </div>
  );
}
