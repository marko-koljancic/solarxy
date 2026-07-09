// Per-pane DOM toolbars floated over the WebGPU canvas (UX spec section
// 11): one strip per pane carrying the pane's view mode, inspection mode,
// projection, view presets, and grid toggle. Pure interpreters of the
// view-state mirror; every change goes through the session's view actions
// (Rust owns the truth). Positioned from the host-computed pane rects.

import { cameraCommand, setActivePane, setPaneSettings, setSplitRatio } from "../engine/session";
import type { PaneDisplaySettings, ViewAxis, ViewStateDto } from "../engine/types";
import { useViewState } from "../store/viewState";

const VIEW_MODES = [
  ["Shaded", "Shaded"],
  ["ShadedWireframe", "Shaded + Wire"],
  ["WireframeOnly", "Wireframe"],
  ["Ghosted", "Ghosted"],
] as const;

const INSPECTION_MODES = [
  ["Shaded", "Shaded"],
  ["MaterialId", "Material ID"],
  ["TexelDensity", "Texel Density"],
  ["Depth", "Depth"],
  ["Overdraw", "Overdraw"],
  ["AoPreview", "AO Preview"],
] as const;

const VIEW_AXES: [ViewAxis, string][] = [
  ["top", "Top"],
  ["bottom", "Bottom"],
  ["front", "Front"],
  ["back", "Back"],
  ["left", "Left"],
  ["right", "Right"],
];

function PaneToolbar({ pane, settings, projection, active }: {
  pane: number;
  settings: PaneDisplaySettings;
  projection: string;
  active: boolean;
}) {
  const overlapPct = useViewState((s) => s.uvOverlapPct);
  const overlapPending = useViewState((s) => s.uvOverlapPending);
  const patch = (p: Partial<PaneDisplaySettings>) => {
    setActivePane(pane);
    setPaneSettings(pane, { ...settings, ...p });
  };

  // A UV pane swaps the 3D controls for the UV set: background, the
  // overlap toggle with its live percentage (or pending indicator), and
  // the way back to the 3D view (key 3 toggles too).
  if (settings.paneMode === "UvMap") {
    return (
      <div
        className={`pane-toolbar${active ? " active" : ""}`}
        onPointerDown={(e) => e.stopPropagation()}
      >
        <span className="pane-label">UV Map</span>
        <select
          className="pane-select"
          title="UV background"
          value={settings.uvBg}
          onChange={(e) => patch({ uvBg: e.target.value as PaneDisplaySettings["uvBg"] })}
        >
          <option value="Checker">Checker</option>
          <option value="Dark">Dark</option>
          <option value="Charcoal">Charcoal</option>
        </select>
        <button
          type="button"
          className={`pane-btn${settings.showUvOverlap ? " on" : ""}`}
          title="Toggle the UV overlap display (key O)"
          onClick={() => patch({ showUvOverlap: !settings.showUvOverlap })}
        >
          Ovl
        </button>
        {settings.showUvOverlap && (
          <span className="pane-overlap" title="UV overlap percentage">
            {overlapPending || overlapPct === null
              ? "computing..."
              : `${overlapPct.toFixed(1)}% overlap`}
          </span>
        )}
        <button
          type="button"
          className="pane-btn"
          title="Back to the 3D view (key 3)"
          onClick={() => patch({ paneMode: "Scene3D" })}
        >
          3D
        </button>
      </div>
    );
  }

  return (
    <div className={`pane-toolbar${active ? " active" : ""}`} onPointerDown={(e) => e.stopPropagation()}>
      <select
        className="pane-select"
        title="View mode"
        value={settings.viewMode}
        onChange={(e) => patch({ viewMode: e.target.value as PaneDisplaySettings["viewMode"] })}
      >
        {VIEW_MODES.map(([v, label]) => (
          <option key={v} value={v}>
            {label}
          </option>
        ))}
      </select>
      <select
        className="pane-select"
        title="Inspection mode (keys 1-7)"
        value={settings.inspectionMode}
        onChange={(e) =>
          patch({ inspectionMode: e.target.value as PaneDisplaySettings["inspectionMode"] })
        }
      >
        {INSPECTION_MODES.map(([v, label]) => (
          <option key={v} value={v}>
            {label}
          </option>
        ))}
      </select>
      <select
        className="pane-select"
        title="Projection"
        value={projection}
        onChange={(e) => {
          setActivePane(pane);
          cameraCommand(pane, {
            kind: "projection",
            mode: e.target.value as "perspective" | "orthographic",
          });
        }}
      >
        <option value="perspective">Persp</option>
        <option value="orthographic">Ortho</option>
      </select>
      <select
        className="pane-select"
        title="View presets"
        value=""
        onChange={(e) => {
          const v = e.target.value;
          setActivePane(pane);
          if (v === "fit") cameraCommand(pane, { kind: "fit" });
          else if (v) cameraCommand(pane, { kind: "view", axis: v as ViewAxis });
          e.target.value = "";
        }}
      >
        <option value="" disabled>
          Views
        </option>
        <option value="fit">Fit (F)</option>
        {VIEW_AXES.map(([axis, label]) => (
          <option key={axis} value={axis}>
            {label}
          </option>
        ))}
      </select>
      <select
        className="pane-select"
        title="Pane background"
        value={typeof settings.backgroundMode === "string" ? settings.backgroundMode : "Custom"}
        onChange={(e) => patch({ backgroundMode: e.target.value })}
      >
        <option value="Gradient">Gradient</option>
        <option value="White">White</option>
        <option value="DarkGray">Dark</option>
        <option value="AyuMirage">Ayu</option>
        <option value="Black">Black</option>
        <option value="HdriSky">Sky</option>
      </select>
      <button
        type="button"
        className={`pane-btn${settings.showGrid ? " on" : ""}`}
        title="Toggle grid"
        onClick={() => patch({ showGrid: !settings.showGrid })}
      >
        Grid
      </button>
      <button
        className={`pane-btn${settings.showValidation ? " on" : ""}`}
        title="Toggle the validation overlay (issue tints + edge highlights)"
        onClick={() => patch({ showValidation: !settings.showValidation })}
      >
        Val
      </button>
    </div>
  );
}

/** The draggable split divider for the two-pane layouts. Dragging drives
 * the host ratio (clamped 0.05-0.95 Rust-side); the pane rects follow via
 * the paneRects host event. */
function PaneDivider({ view }: { view: ViewStateDto }) {
  const vertical = view.layout === "splitVertical";
  if (!vertical && view.layout !== "splitHorizontal") return null;
  const [a, b] = view.paneRects;
  if (!a || !b) return null;

  const style: React.CSSProperties = vertical
    ? {
        left: (a.x + a.width + b.x) / 2 - 3,
        top: a.y,
        width: 6,
        height: a.height,
        cursor: "col-resize",
      }
    : {
        left: a.x,
        top: (a.y + a.height + b.y) / 2 - 3,
        width: a.width,
        height: 6,
        cursor: "row-resize",
      };

  const onPointerDown = (e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    // The divider's parent is the viewport region container.
    const region = (e.currentTarget as HTMLElement).parentElement?.getBoundingClientRect();
    if (!region) return;
    const onMove = (ev: PointerEvent) => {
      const ratio = vertical
        ? (ev.clientX - region.left) / region.width
        : (ev.clientY - region.top) / region.height;
      setSplitRatio(ratio);
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.classList.remove(vertical ? "col-resizing" : "row-resizing");
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    document.body.classList.add(vertical ? "col-resizing" : "row-resizing");
  };

  return <div className="viewport-divider" style={style} onPointerDown={onPointerDown} />;
}

/** All pane toolbars, absolutely positioned from the host pane rects. */
export function PaneToolbars() {
  const view = useViewState((s) => s.view);
  if (!view) return null;
  return (
    <>
      {view.paneRects.map((rect, i) => (
        <div
          key={i}
          className="pane-toolbar-anchor"
          style={{ left: rect.x, top: rect.y, width: rect.width }}
        >
          <PaneToolbar
            pane={i}
            settings={view.paneSettings[i]}
            projection={view.paneProjections[i]}
            active={view.activePane === i && view.paneRects.length > 1}
          />
        </div>
      ))}
      <PaneDivider view={view} />
    </>
  );
}
