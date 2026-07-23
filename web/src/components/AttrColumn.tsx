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
import { attrPinKey, registerAttrPin } from "../engine/attrPins";
import type { AttrLane, AttrVizState } from "../engine/types";
import {
  IconAttrLabels,
  IconAttrPoints,
  IconAttrSettings,
  IconAttrVectors,
  IconChevronDown,
} from "../icons";
import { useMirror } from "../store/mirror";
import { useViewState } from "../store/viewState";
import { DropdownPortal } from "./DropdownPortal";

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
  const shownCap = viz.cap === 0 ? 64 : viz.cap;

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
            <div className="attr-viz-row">
              <label htmlFor="attr-viz-cap">Pin cap</label>
              <input
                id="attr-viz-cap"
                type="number"
                min={8}
                max={256}
                step={8}
                value={shownCap}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  if (Number.isFinite(n)) patch({ cap: Math.max(8, Math.min(256, n)) });
                }}
              />
            </div>
            <div className="attr-viz-row">
              <button
                type="button"
                className="attr-viz-reset"
                onClick={() =>
                  patch({
                    vectorScale: 1,
                    normalize: false,
                    colorMode: "uniform",
                    color: [1, 0.62, 0.15],
                    cap: 0,
                  })
                }
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
      </div>
      <LanePicker viz={viz} />
      <VizSettings viz={viz} />
    </div>
  );
}

/** The pooled pin elements (position/text patched imperatively per frame
 * by engine/attrPins.ts). One clip box per 3D pane; pool size follows the
 * host's cap. */
export function AttrPinsOverlay() {
  const view = useViewState((s) => s.view);
  if (!view?.attrViz || !(view.attrViz.labels || view.attrViz.points)) return null;
  const cap = view.attrViz.cap === 0 ? 64 : Math.min(view.attrViz.cap, 256);

  return (
    <>
      {view.paneRects.map((rect, pane) => {
        if (view.paneSettings[pane]?.paneMode === "UvMap") return null;
        return (
          <div
            key={pane}
            className="attr-pin-clip"
            style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
          >
            {Array.from({ length: cap }, (_, slot) => (
              <div
                key={slot}
                className="attr-pin"
                ref={(el) => registerAttrPin(attrPinKey(pane, slot), el)}
              >
                <span className="attr-pin-dot" aria-hidden />
                <span className="attr-pin-text" />
              </div>
            ))}
          </div>
        );
      })}
    </>
  );
}
