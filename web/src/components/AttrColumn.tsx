// The viewport attribute-visualization strip: a ToolColumn twin down the
// RIGHT edge of the 3D region. Three toggles (value labels, Vec3 arrows,
// point markers) over a lane picker fed by the displayed geometries'
// attribute summaries. Like the tool column it is an overlay inside the
// canvas host, never a DOM column beside it, so the canvas rect (and the
// Rust pane rects derived from it) never move.
//
// Also hosts the pooled pin elements the rAF loop patches imperatively
// (engine/attrPins.ts): one clip box per 3D pane, `cap` slots each.

import { useMemo, useRef, useState } from "react";
import { getClient, setAttrViz } from "../engine/session";
import { useAttrPinStats } from "../engine/attrPins";
import type { AttrLane, AttrVizState, RampPreset } from "../engine/types";
import {
  IconAttrLabels,
  IconAttrPoints,
  IconAttrSettings,
  IconAttrVectors,
  IconChevronDown,
} from "../icons";
import { useMirror } from "../store/mirror";
import { usePrefs } from "../store/prefs";
import { useViewState } from "../store/viewState";
import { DropdownPortal } from "./DropdownPortal";
import { Select, type SelectOption } from "./Select";

/** The curated ramp styles. Labels and gradient chips only; the actual
 * stop colors live in `solarxy_web::attr_viz::RampPreset` (the chips are
 * a display-only mirror of those stops). */
const RAMP_PRESETS: readonly SelectOption<RampPreset>[] = [
  { value: "coldWarm", label: "Cold to Warm", swatch: "linear-gradient(90deg, #4073e6, #ff9e26, #e6261a)" },
  { value: "ember", label: "Ember", swatch: "linear-gradient(90deg, #0d081f, #731a59, #e6731f, #faeba6)" },
  { value: "ocean", label: "Ocean", swatch: "linear-gradient(90deg, #081a40, #1a73b3, #d9f7ff)" },
  { value: "grayscale", label: "Grayscale", swatch: "linear-gradient(90deg, #141414, #f2f2f2)" },
  { value: "signal", label: "Signal", swatch: "linear-gradient(90deg, #26a64d, #f2d933, #d9261a)" },
];

/** The host color is raw RGB 0..1 fed straight to the HDR line pipeline
 * (matching the historical amber constant), so the picker's hex maps
 * component-for-component with no gamma conversion. Exported for tests. */
export function rgbToHex([r, g, b]: [number, number, number]): string {
  const c = (v: number) =>
    Math.round(Math.max(0, Math.min(1, v)) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`;
}

export function hexToRgb(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.slice(1), 16);
  if (Number.isNaN(n)) return [1, 1, 1];
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

const GLYPH = 19;

/** Mirrors `AttrVizState::MAX_CAP` (the GPU label channel ceiling). */
const PIN_CAP_MAX = 16384;

/** The union of point-domain lanes across every displayed geometry (each
 * geo subflow's display-flag node), first-seen type per name. Fetched on
 * demand, matching the summary query's pull design. */
function sceneLanes(): AttrLane[] {
  const s = useMirror.getState();
  const merged = new Map<string, AttrLane>();
  for (const [key, graph] of Object.entries(s.contexts)) {
    if (key === "root" || graph.activeOutput === null) continue;
    const summary = getClient().attributeSummary(graph.activeOutput);
    for (const lane of summary?.point ?? []) {
      if (!merged.has(lane.name)) merged.set(lane.name, lane);
    }
  }
  return [...merged.values()].sort((a, b) => a.name.localeCompare(b.name));
}

function VizButton({
  icon,
  label,
  active,
  disabled,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`tool-btn${active ? " active" : ""}`}
      title={label}
      aria-label={label}
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
    >
      {icon}
    </button>
  );
}

function LanePicker({ viz }: { viz: AttrVizState }) {
  const [open, setOpen] = useState(false);
  const [lanes, setLanes] = useState<AttrLane[]>([]);
  const triggerRef = useRef<HTMLButtonElement>(null);

  const stale = viz.name !== null && !lanes.some((l) => l.name === viz.name);

  return (
    <div className="attr-lane-picker">
      <button
        ref={triggerRef}
        type="button"
        className={`attr-lane-trigger${open ? " open" : ""}${stale && open ? " stale" : ""}`}
        title={
          viz.name
            ? `Visualized attribute: ${viz.name}`
            : "Pick the attribute to visualize"
        }
        onClick={() => {
          if (open) {
            setOpen(false);
          } else {
            setLanes(sceneLanes());
            setOpen(true);
          }
        }}
      >
        <span className="attr-lane-name">{viz.name ?? "attr"}</span>
        <IconChevronDown size={10} />
      </button>
      {open && (
        <DropdownPortal anchorRef={triggerRef} align="right" onClose={() => setOpen(false)}>
          <div className="select-list attr-lane-list" role="listbox">
            {lanes.length === 0 && <div className="attr-name-empty">No point attributes.</div>}
            {lanes.map((lane) => (
              <button
                key={lane.name}
                type="button"
                role="option"
                aria-selected={lane.name === viz.name}
                className={`select-option${lane.name === viz.name ? " active" : ""}`}
                onClick={() => {
                  setAttrViz({ ...viz, name: lane.name });
                  setOpen(false);
                }}
              >
                <span className="select-option-label">{lane.name}</span>
                <span className="select-option-hint">{lane.ty}</span>
              </button>
            ))}
          </div>
        </DropdownPortal>
      )}
    </div>
  );
}

/** The gear under the lane pill: a compact popover with the vector
 * scale, normalize, color mode, uniform color, and pin cap. Every change
 * flows through the same host mutator as the strip toggles. */
function VizSettings({ viz }: { viz: AttrVizState }) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const patch = (p: Partial<AttrVizState>) => setAttrViz({ ...viz, ...p });
  const capacity = useAttrPinStats((s) => s.capacity);
  // The 0 sentinel means all points (up to the host ceiling); show what
  // that resolves to right now so the field is never a lie.
  const shownCap = viz.cap === 0 ? capacity || PIN_CAP_MAX : viz.cap;

  return (
    <div className="attr-viz-settings">
      <button
        ref={triggerRef}
        type="button"
        className={`tool-btn${open ? " active" : ""}`}
        title="Visualization settings"
        aria-label="Visualization settings"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <IconAttrSettings size={GLYPH} />
      </button>
      {open && (
        <DropdownPortal anchorRef={triggerRef} align="right" onClose={() => setOpen(false)}>
          <div className="attr-viz-panel">
            <div className="attr-viz-row">
              <label htmlFor="attr-viz-scale">Vector scale</label>
              <input
                id="attr-viz-scale"
                type="range"
                min={0.05}
                max={10}
                step={0.05}
                value={viz.vectorScale}
                onChange={(e) => patch({ vectorScale: Number(e.target.value) })}
              />
              <span className="attr-viz-value">{viz.vectorScale.toFixed(2)}x</span>
            </div>
            <div className="attr-viz-row">
              <label htmlFor="attr-viz-normalize">Normalize</label>
              <input
                id="attr-viz-normalize"
                type="checkbox"
                checked={viz.normalize}
                onChange={(e) => patch({ normalize: e.target.checked })}
              />
            </div>
            <div className="attr-viz-row">
              <span className="attr-viz-label">Color</span>
              <div className="attr-viz-segment" role="radiogroup" aria-label="Arrow color mode">
                <button
                  type="button"
                  className={viz.colorMode === "uniform" ? "active" : ""}
                  onClick={() => patch({ colorMode: "uniform" })}
                >
                  Uniform
                </button>
                <button
                  type="button"
                  className={viz.colorMode === "ramp" ? "active" : ""}
                  title="Cold to warm over the lane's magnitude range"
                  onClick={() => patch({ colorMode: "ramp" })}
                >
                  Ramp
                </button>
              </div>
              {viz.colorMode === "uniform" && (
                <input
                  type="color"
                  aria-label="Arrow color"
                  value={rgbToHex(viz.color)}
                  onChange={(e) => patch({ color: hexToRgb(e.target.value) })}
                />
              )}
            </div>
            {viz.colorMode === "ramp" && (
              <div className="attr-viz-row">
                <span className="attr-viz-label">Ramp</span>
                <Select
                  ariaLabel="Ramp preset"
                  value={viz.rampPreset}
                  options={RAMP_PRESETS}
                  onChange={(rampPreset) => patch({ rampPreset })}
                  width="100%"
                />
              </div>
            )}
            {/* Label appearance. Grouped after the vector controls and
                before the shared pin cap, because these four only affect
                the label channel. The values ride the same session state,
                and Preferences > Display holds the defaults they start
                from. */}
            <div className="attr-viz-row">
              <span className="attr-viz-label">Label size</span>
              <div className="attr-viz-segment" role="radiogroup" aria-label="Label size">
                {(["small", "medium", "large"] as const).map((s) => (
                  <button
                    key={s}
                    type="button"
                    className={viz.labelSize === s ? "active" : ""}
                    onClick={() => patch({ labelSize: s })}
                  >
                    {s === "small" ? "S" : s === "medium" ? "M" : "L"}
                  </button>
                ))}
              </div>
            </div>
            <div className="attr-viz-row">
              <span className="attr-viz-label">Background</span>
              <div className="attr-viz-segment" role="radiogroup" aria-label="Label background">
                <button
                  type="button"
                  className={viz.labelBackground === "chip" ? "active" : ""}
                  title="A rounded chip behind the text: legible over any scene"
                  onClick={() => patch({ labelBackground: "chip" })}
                >
                  Chip
                </button>
                <button
                  type="button"
                  className={viz.labelBackground === "none" ? "active" : ""}
                  title="Text and anchor dot only. Quieter, but contrast is no longer guaranteed"
                  onClick={() => patch({ labelBackground: "none" })}
                >
                  None
                </button>
              </div>
            </div>
            <div className="attr-viz-row">
              <label htmlFor="attr-viz-opacity">Label opacity</label>
              <input
                id="attr-viz-opacity"
                type="range"
                min={0.1}
                max={1}
                step={0.05}
                value={viz.labelOpacity}
                onChange={(e) => patch({ labelOpacity: Number(e.target.value) })}
              />
              <span className="attr-viz-value">{Math.round(viz.labelOpacity * 100)}%</span>
            </div>
            <div className="attr-viz-row">
              <label htmlFor="attr-viz-decimals">Decimals</label>
              <input
                id="attr-viz-decimals"
                type="number"
                min={0}
                max={4}
                step={1}
                value={viz.labelDecimals}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  if (Number.isFinite(n)) {
                    patch({ labelDecimals: Math.max(0, Math.min(4, Math.round(n))) });
                  }
                }}
              />
            </div>
            <div className="attr-viz-row">
              <label htmlFor="attr-viz-cap" title="Default: every point, up to 16384 per scene">
                Pin cap
              </label>
              <input
                id="attr-viz-cap"
                type="number"
                min={8}
                max={PIN_CAP_MAX}
                step={8}
                value={shownCap}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  if (Number.isFinite(n)) patch({ cap: Math.max(8, Math.min(PIN_CAP_MAX, n)) });
                }}
              />
            </div>
            <div className="attr-viz-row">
              <button
                type="button"
                className="attr-viz-reset"
                onClick={() => {
                  // Labels reset to the SAVED defaults, not to the shipped
                  // ones: someone who set large text in Preferences means
                  // it, and having Reset overrule their own default would
                  // make the preference feel broken.
                  const d = usePrefs.getState().prefs.display;
                  patch({
                    vectorScale: 1,
                    normalize: false,
                    colorMode: "uniform",
                    color: [1, 0.62, 0.15],
                    rampPreset: "coldWarm",
                    cap: 0,
                    labelSize: d.labelSize,
                    labelBackground: d.labelBackground,
                    labelOpacity: d.labelOpacity,
                    labelDecimals: d.labelDecimals,
                  });
                }}
              >
                Reset to defaults
              </button>
            </div>
          </div>
        </DropdownPortal>
      )}
    </div>
  );
}

/** The discreet sampling notice: shown only while pins are on and the
 * host had to stride-sample (more displayed points than the budget), so
 * a sparse-looking read is never mistaken for complete data. */
function SamplingNotice({ viz }: { viz: AttrVizState }) {
  const { capacity, total } = useAttrPinStats();
  if (!(viz.labels || viz.points) || capacity === 0 || total <= capacity) return null;
  return (
    <div
      className="attr-pin-notice"
      title={`More points than the pin budget: showing every ${Math.ceil(total / capacity)}th point. Raise the cap in the visualization settings.`}
    >
      {capacity.toLocaleString()} of {total.toLocaleString()} pts
    </div>
  );
}

export function AttrColumn() {
  const viz = useViewState((s) => s.view?.attrViz);
  // Re-derive the scene's lane inventory when the document changes (the
  // revision advances on command batches and cook events alike), so the
  // arrows toggle gates on the picked lane's REAL type.
  const revision = useMirror((s) => s.revision);
  const lanes = useMemo(
    () => (viz ? sceneLanes() : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [revision, viz?.name],
  );
  if (!viz) return null;

  // Arrows draw vec3 and vec4 (xyz) lanes; the toggle stays visible but
  // disabled for float/vec2 so the strip's shape is stable.
  const picked = lanes.find((l) => l.name === viz.name);
  const canArrow = picked?.ty === "vec3" || picked?.ty === "vec4";
  return (
    <div className="attr-column" role="toolbar" aria-label="Attribute visualization">
      <div className="tool-group">
        <VizButton
          icon={<IconAttrLabels size={GLYPH} />}
          label="Value labels"
          active={viz.labels}
          onClick={() => setAttrViz({ ...viz, labels: !viz.labels })}
        />
        <VizButton
          icon={<IconAttrVectors size={GLYPH} />}
          label="Vector arrows (vec3/vec4 lanes)"
          active={viz.vectors}
          disabled={!canArrow}
          onClick={() => setAttrViz({ ...viz, vectors: !viz.vectors })}
        />
        <VizButton
          icon={<IconAttrPoints size={GLYPH} />}
          label="Point numbers"
          active={viz.points}
          onClick={() => setAttrViz({ ...viz, points: !viz.points })}
        />
        {/* The gear closes the square stack (feedback: one grouped strip,
            all square buttons); only the text-carrying lane pill sits
            outside it. */}
        <VizSettings viz={viz} />
      </div>
      <LanePicker viz={viz} />
      <SamplingNotice viz={viz} />
    </div>
  );
}

