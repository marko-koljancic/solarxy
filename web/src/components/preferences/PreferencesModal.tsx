// The preferences modal, on the Minimystix pattern: a draft
// copy edited locally, dirty star in the title, footer with Reset to
// Defaults (nested confirm) on the left and Cancel / Apply / Save on the
// right. Six tabs: Appearance, Display, Review, Autosave, Screenshot,
// Viewport (`TABS` below is the list; keep this sentence in step with it).
// Esc and backdrop-click dismiss.
//
// The dialog is a fixed-height column: `bodyLayout="column"` on the shell is
// what lets `.prefs-body` grow and keeps `.prefs-footer` on the bottom edge
// whatever the current tab's height is.

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
import type {
  LabelBackgroundChoice,
  LabelSizeChoice,
} from "../../store/displayDefaults";
import { Row, Section } from "../DialogRow";
import { ConfirmDialog } from "../ConfirmDialog";
import { Modal } from "../Modal";
import { Select } from "../Select";

const TABS = ["Appearance", "Display", "Review", "Autosave", "Screenshot", "Viewport"] as const;
type Tab = (typeof TABS)[number];

function AppearanceTab({ draft, patch }: TabProps) {
  return (
    <>
      <p className="prefs-desc">Theme and motion. Device choices follow the operating system.</p>
      <Section title="Theme">
      <Row
        label="Theme"
        doc="Which palette the whole interface uses. **Device** follows your operating system and switches with it, including on a schedule if your OS has one."
      >
        <Select
          ariaLabel="Theme"
          value={draft.appearance.theme}
          options={[
            { value: "dark", label: "Dark" },
            { value: "light", label: "Light" },
            { value: "system", label: "Device" },
          ]}
          onChange={(v) => patch({ appearance: { ...draft.appearance, theme: v as ThemeChoice } })}
        />
      </Row>
      </Section>
      <Section title="Motion">
      <Row
        label="Reduced motion"
        doc="Whether the interface animates. **Follow device** honours the system accessibility setting; **Reduce** forces static states regardless of it."
      >
        <Select
          ariaLabel="Reduced motion"
          value={draft.appearance.reducedMotion}
          options={[
            { value: "system", label: "Follow device" },
            { value: "reduce", label: "Reduce" },
            { value: "none", label: "Full motion" },
          ]}
          onChange={(v) =>
            patch({ appearance: { ...draft.appearance, reducedMotion: v as MotionChoice } })
          }
        />
      </Row>
      </Section>
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
      <p className="prefs-desc">
        Defaults for new scenes and panes. A saved scene keeps the per-pane settings it was saved
        with, and a pane&apos;s Display menu overrides the current view.
      </p>
      <Section title="Viewport">
      <Row
        label="Wireframe weight"
        doc="Line thickness for wireframe and shaded-wireframe views. Seeds new panes and new scenes; a saved scene keeps the weight it was saved with, and a pane's Display menu overrides the current view."
      >
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
      <Row
        label="Background"
        doc="The default backdrop for new panes. **HDRI Sky** shows the loaded environment image itself rather than a flat colour, so it only differs from Gradient once you have loaded one."
      >
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
      <Row
        label="Turntable speed"
        doc="How fast a pane spins when its turntable is running, in revolutions per minute. Applies immediately to a spin already in progress."
      >
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
      </Section>
      <Section title="Points and labels">
      <Row
        label="Point size"
        doc="On-screen size of a rendered point, in pixels. Point clouds and the `scatter` node both draw with it. Global rather than per pane: there is no comparison worth two point sizes side by side."
      >
        <input
          className="input-field"
          type="number"
          min={1}
          max={32}
          step={1}
          value={d.pointSize}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v)) setD({ pointSize: Math.min(32, Math.max(1, v)) });
          }}
        />
        <span className="prefs-unit">px</span>
      </Row>
      <Row
        label="Label size"
        doc="Text size for attribute labels in the viewport. Three presets rather than a free number, because the renderer scales one baked glyph atlas."
      >
        <Select
          ariaLabel="Attribute label size"
          value={d.labelSize}
          options={[
            { value: "small", label: "Small" },
            { value: "medium", label: "Medium" },
            { value: "large", label: "Large" },
          ]}
          onChange={(v) => setD({ labelSize: v as LabelSizeChoice })}
        />
      </Row>
      <Row
        label="Label background"
        doc="What sits behind a label's text. **Chip** guarantees contrast over any scene; **None** is quieter but leaves the text to fend for itself against whatever is behind it."
      >
        <Select
          ariaLabel="Attribute label background"
          value={d.labelBackground}
          options={[
            { value: "chip", label: "Chip" },
            { value: "none", label: "None (text only)" },
          ]}
          onChange={(v) => setD({ labelBackground: v as LabelBackgroundChoice })}
        />
      </Row>
      <Row
        label="Label opacity"
        doc="How solid attribute labels are, from 0.1 to 1. The chip keeps its own 82% underneath, so it always stays the more transparent of the two."
      >
        <input
          className="input-field prefs-dim"
          type="number"
          min={0.1}
          max={1}
          step={0.05}
          value={d.labelOpacity}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v)) setD({ labelOpacity: Math.min(1, Math.max(0.1, v)) });
          }}
        />
      </Row>
      <Row
        label="Label decimals"
        doc="Decimal places in a label's value, 0 to 4. Fewer places fit more labels on screen before they start overlapping."
      >
        <input
          className="input-field prefs-dim"
          type="number"
          min={0}
          max={4}
          step={1}
          value={d.labelDecimals}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v)) setD({ labelDecimals: Math.min(4, Math.max(0, Math.round(v))) });
          }}
        />
      </Row>
      </Section>
      <Section title="Code editor">
        <Row
          label="Word wrap"
          doc="Whether long lines wrap rather than scrolling sideways. A wrangle is usually short enough that wrapping costs nothing and horizontal scrolling costs a lot."
        >
          <input
            type="checkbox"
            checked={draft.editor.wordWrap}
            onChange={(e) => patch({ editor: { ...draft.editor, wordWrap: e.target.checked } })}
          />
        </Row>
        <Row
          label="Line numbers"
          doc="Whether the editor shows a line-number gutter. Cook errors name a line, so the gutter is how you find the one they mean."
        >
          <input
            type="checkbox"
            checked={draft.editor.lineNumbers}
            onChange={(e) => patch({ editor: { ...draft.editor, lineNumbers: e.target.checked } })}
          />
        </Row>
        <Row
          label="Editor font size"
          doc="Text size inside code editors, in pixels. Independent of the rest of the interface, because code and prose are comfortable at different sizes."
        >
          <input
            className="input-field prefs-dim"
            type="number"
            min={9}
            max={24}
            step={1}
            value={draft.editor.fontSize}
            onChange={(e) => {
              const v = Number(e.target.value);
              if (Number.isFinite(v)) {
                patch({ editor: { ...draft.editor, fontSize: Math.min(24, Math.max(9, v)) } });
              }
            }}
          />
          <span className="prefs-unit">px</span>
        </Row>
      </Section>
      <div className="prefs-info">
        Wireframe and background are the defaults for new scenes and panes; a scene file keeps the
        per-pane settings it was saved with, and the pane&apos;s Display menu overrides the current
        view. The turntable speed and point size apply immediately. Point size is the on-screen
        size of a rendered point, which a point cloud and the scatter node both draw with. The four
        label settings are what a session starts from; the gear in the attribute strip is the live
        override, and in a shaded view labels on the far side of an object are hidden by the near
        side (a wireframe view shows all of them).
      </div>
    </>
  );
}

function ReviewTab({ draft, patch }: TabProps) {
  return (
    <>
      <p className="prefs-desc">Review annotations.</p>
      <Row
        label="Author"
        doc="The name written onto review annotations you create. Left empty, annotations are anonymous: attribution is opt-in and never taken from your operating system account."
      >
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
      <Section title="Background saving">
      <Row
        label="Enabled"
        doc="Whether the scene is saved to browser storage in the background. Autosave is what the recovery prompt restores from after a crash or a closed tab; it never writes to a file you chose."
      >
        <input
          type="checkbox"
          checked={draft.autosave.enabled}
          onChange={(e) => patch({ autosave: { ...draft.autosave, enabled: e.target.checked } })}
        />
      </Row>
      <Row
        label="Delay"
        doc="How long after your last edit an autosave runs. A hard 15-second cap applies regardless, so a continuous drag still gets saved."
      >
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
      </Section>
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
      <Row
        label="Resolution"
        doc="Capture size for screenshots. The multipliers are relative to the pane's current on-screen size; **Custom** takes exact pixels below."
      >
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
        <Row
        label="Size"
        doc="Exact capture size in pixels. Captures are budgeted at about 4 megapixels: past that the browser can lose the graphics device."
      >
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
      <Section title="Include in the capture">
      {(
        [
          [
            "grid",
            "Grid",
            "Whether the ground grid appears in the saved image. Off is usual for a presentation frame, on for a measurement one.",
          ],
          [
            "axes",
            "Axes",
            "Whether the corner axis gizmo appears in the saved image.",
          ],
          [
            "validation",
            "Validation overlay",
            "Whether validation highlights are baked into the image. On, a screenshot doubles as a bug report showing exactly which faces are flagged.",
          ],
        ] as const
      ).map(([key, label, doc]) => (
        <Row key={key} label={label} doc={doc}>
          <input
            type="checkbox"
            checked={overlays[key]}
            onChange={(e) => patchSc({ overlays: { ...overlays, [key]: e.target.checked } })}
          />
        </Row>
      ))}
      </Section>
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
  const chrome = draft.chrome;

  return (
    <>
      <p className="prefs-desc">
        Viewport chrome, how selected objects highlight in the 3D view, the transform gizmo
        orientation, and the increments Ctrl-drag snaps to.
      </p>
      <Section title="Chrome">
      <Row
        label="Playbar"
        doc="The scene-clock strip under the viewport, with the frame scrubber and the range and rate fields. Hidden, the viewport reclaims its height and the Space, comma and period keys still work, so you give up the readout rather than the clock."
      >
        <input
          type="checkbox"
          checked={chrome.transportBar}
          onChange={(e) => patch({ chrome: { ...chrome, transportBar: e.target.checked } })}
        />
      </Row>
      </Section>
      <Section title="Selection">
      <Row
        label="Selection highlight"
        doc="How a selected object is marked in the 3D view. **Outline** draws a rim around its silhouette; **Tint** washes the surface, which is cheaper but hides the material underneath."
      >
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
        <Row
        label="Highlight color"
        doc="The colour of the selection rim or tint. Picked in sRGB and converted for the renderer."
      >
          <input
            type="color"
            className="input-field prefs-color"
            value={sel.color}
            onChange={(e) => setSel({ color: e.target.value })}
          />
        </Row>
      )}
      {sel.style === "outline" && (
        <Row
        label="Outline width (px)"
        doc="Thickness of the selection rim, 1 to 16 pixels. Measured on screen, so it stays the same regardless of how far you zoom."
      >
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
      </Section>
      <Section title="Transform handles">
      <Row
        label="Handle orientation"
        doc="Which axes the Move and Rotate handles align to. Scale is always object axes: a world-axis scale on a rotated object would shear it, and there is no shear parameter."
      >
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
      </Section>
      <Section title="Snapping">
      <Row
        label="Snap: move (m)"
        doc="World units a translate drag snaps to while Ctrl or Cmd is held. 0 disables snapping for the Move tool."
      >
        <input
          className="input-field"
          type="number"
          min={0}
          step={0.1}
          value={v.snapTranslate}
          onChange={(e) => setV({ snapTranslate: Math.max(0, Number(e.target.value) || 0) })}
        />
      </Row>
      <Row
        label="Snap: rotate (deg)"
        doc="Degrees a rotate drag snaps to while Ctrl or Cmd is held. 0 disables snapping for the Rotate tool."
      >
        <input
          className="input-field"
          type="number"
          min={0}
          step={1}
          value={v.snapRotate}
          onChange={(e) => setV({ snapRotate: Math.max(0, Number(e.target.value) || 0) })}
        />
      </Row>
      <Row
        label="Snap: scale"
        doc="The increment a scale drag snaps to while Ctrl or Cmd is held. 0 disables snapping for the Scale tool."
      >
        <input
          className="input-field"
          type="number"
          min={0}
          step={0.05}
          value={v.snapScale}
          onChange={(e) => setV({ snapScale: Math.max(0, Number(e.target.value) || 0) })}
        />
      </Row>
      </Section>
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
      bodyLayout="column"
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
