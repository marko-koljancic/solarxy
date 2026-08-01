// One pane's look: exposure, tone mapper, and the lift/gamma/gain grade.
//
// A dialog rather than entries in the pane's Display dropdown, because that
// dropdown is a list of checkmarks and submenus and these are continuous
// values. Follows the Environment modal: the host owns the truth, this
// mirrors `paneLooks` from the view-state store and mutates through a
// session action.
//
// Deliberately smaller than the camera node's look. A pane reaches the
// scalars only; the two lookup-table slots live on the camera, because a
// table is a staged document asset and a pane is a viewport rather than a
// document object. When the pane is looking through a camera this dialog
// says so and sends you to the node, instead of editing a value the pane
// is not compositing with.

import { setPaneLook } from "../engine/session";
import type { PaneLook } from "../engine/types";
import { useViewState } from "../store/viewState";
import { Modal } from "./Modal";
import { Row } from "./DialogRow";
import { Select } from "./Select";

export const TONE_MODES: [PaneLook["toneMode"], string][] = [
  ["None", "None (clip)"],
  ["Linear", "Linear"],
  ["Reinhard", "Reinhard"],
  ["AcesFilmic", "ACES Filmic"],
];

/** The look that changes nothing, mirroring `PaneLook::default()` in
 * `solarxy_core::view_config`. Exported so the Reset button and the test
 * that pins it to the Rust defaults share one definition. */
export const NEUTRAL: PaneLook = {
  exposure: 1,
  toneMode: "AcesFilmic",
  lift: [0, 0, 0],
  gamma: [1, 1, 1],
  gain: [1, 1, 1],
};

const CHANNELS: [number, string][] = [
  [0, "R"],
  [1, "G"],
  [2, "B"],
];

/** Three numeric fields for one grade vector, editable per channel. */
function Triple({
  value,
  step,
  min,
  onChange,
}: {
  value: [number, number, number];
  step: number;
  min?: number;
  onChange: (next: [number, number, number]) => void;
}) {
  return (
    <>
      {CHANNELS.map(([i, label]) => (
        <span key={label} className="look-channel">
          <span className="look-channel-label">{label}</span>
          <input
            className="input-field"
            type="number"
            aria-label={label}
            step={step}
            min={min}
            value={value[i]}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (!Number.isFinite(n)) return;
              const next: [number, number, number] = [...value];
              next[i] = n;
              onChange(next);
            }}
          />
        </span>
      ))}
    </>
  );
}

export function PaneLookModal({ pane, onClose }: { pane: number; onClose: () => void }) {
  const view = useViewState((s) => s.view);
  const look = view?.paneLooks?.[pane] ?? NEUTRAL;
  const throughCamera = view?.paneLookThrough?.[pane] != null;

  const patch = (p: Partial<PaneLook>) => setPaneLook(pane, { ...look, ...p });

  return (
    <Modal id="pane-look" title={`Look: pane ${pane + 1}`} onClose={onClose}>
      {throughCamera && (
        <p className="env-hint">
          This pane is looking through a camera, so it composites with that
          camera&apos;s look. Edit it on the camera node, where it also saves with
          the scene and carries the two LUT slots. The values below apply when the
          pane goes back to a free view.
        </p>
      )}
      <Row
        label="Exposure"
        doc="Linear multiplier on the whole image before tone mapping, so 2 is one stop brighter and 0.5 one stop darker. Reach for this before changing light intensities: it moves the exposure of the shot rather than the lighting of the scene."
      >
        <input
          className="input-field"
          type="number"
          step={0.1}
          min={0.01}
          max={64}
          value={look.exposure}
          onChange={(e) => {
            const n = Number(e.target.value);
            if (Number.isFinite(n)) patch({ exposure: n });
          }}
        />
      </Row>
      <Row
        label="Tone map"
        doc="How high dynamic range is brought down to what a screen can show. **ACES Filmic** is the filmic default; **Reinhard** is gentler and flatter; **Linear** and **None** both clip, and are for judging raw values rather than for looking at."
      >
        <Select
          ariaLabel="Tone map"
          value={look.toneMode}
          options={TONE_MODES.map(([v, label]) => ({ value: v, label }))}
          onChange={(v) => patch({ toneMode: v as PaneLook["toneMode"] })}
        />
      </Row>
      <Row
        label="Lift"
        doc="Raises or lowers the darkest part of the image, per channel, after tone mapping. Positive lifts the blacks towards grey for a faded base; negative crushes them. It is an addition, so it moves shadows far more than highlights."
      >
        <Triple value={look.lift} step={0.01} onChange={(lift) => patch({ lift })} />
      </Row>
      <Row
        label="Gamma"
        doc="Bends the midtones per channel without moving black or white: above 1 brightens, below 1 darkens. The control for an image whose ends are right and whose middle is not. 1 is neutral."
      >
        <Triple value={look.gamma} step={0.05} min={0.01} onChange={(gamma) => patch({ gamma })} />
      </Row>
      <Row
        label="Gain"
        doc="Multiplies each channel, which moves the highlights most and leaves black at black. Use it to set the white point, or to warm and cool an image by pushing red and blue apart. 1 is neutral."
      >
        <Triple value={look.gain} step={0.05} min={0} onChange={(gain) => patch({ gain })} />
      </Row>
      <p className="env-hint">
        LUT slots live on the camera node: a table is part of the document, so it
        travels with the scene rather than with the viewport.
      </p>
      <div className="modal-actions">
        <button className="btn" onClick={() => setPaneLook(pane, NEUTRAL)}>
          Reset
        </button>
        <button className="btn primary" onClick={onClose}>
          Done
        </button>
      </div>
    </Modal>
  );
}
