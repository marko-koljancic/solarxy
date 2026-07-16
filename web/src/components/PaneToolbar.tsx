// Per-pane ghost-text viewport controls floated over the WebGPU canvas
// (Phase 7b D2, Minimystix ViewportControls / desktop label-menu pattern):
// frameless bracketed labels that open small local dropdowns, replacing
// the filled toolbar strip. Pure interpreters of the view-state mirror;
// every change goes through the session's view actions (Rust owns the
// truth). Positioned from the host-computed pane rects.

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  cameraCommand,
  createCameraFromView,
  jumpToCamera,
  setActivePane,
  setPaneCamera,
  setPaneCameraLock,
  setPaneSettings,
  setSplitRatio,
} from "../engine/session";
import type {
  NodeMirror,
  PaneDisplaySettings,
  PaneRectDto,
  ViewAxis,
  ViewStateDto,
} from "../engine/types";
import { IconCheck } from "../icons";
import { selectGraph, useMirror } from "../store/mirror";
import { useViewState } from "../store/viewState";

/** A camera node's display name (its `name` param, else a fallback). */
function cameraName(n: NodeMirror): string {
  const p = n.params.name;
  if (p?.kind === "literal" && p.type === "text" && p.value.trim()) return p.value;
  return `Camera ${n.id}`;
}

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

// Temporary per-pane shading overrides (item 7): the desktop set, wired to
// the already-plumbed camera.material_override uniform. Session-only, so it
// is never persisted to the scene (reset to None on load, host-side).
const MATERIAL_OVERRIDES = [
  ["None", "Textured"],
  ["Clay", "Clay Light"],
  ["ClayDark", "Clay Dark"],
  ["Chrome", "Chrome"],
  ["Silhouette", "Silhouette"],
] as const;

const VIEW_AXES: [ViewAxis, string][] = [
  ["top", "Top"],
  ["bottom", "Bottom"],
  ["front", "Front"],
  ["back", "Back"],
  ["left", "Left"],
  ["right", "Right"],
];

const BACKGROUNDS = [
  ["Gradient", "Gradient"],
  ["White", "White"],
  ["DarkGray", "Dark"],
  ["AyuMirage", "Ayu"],
  ["Black", "Black"],
  ["HdriSky", "HDRI Sky"],
] as const;

const NORMALS = [
  ["Off", "Off"],
  ["Face", "Face"],
  ["Vertex", "Vertex"],
  ["FaceAndVertex", "Face + Vertex"],
] as const;

const BOUNDS = [
  ["off", "Off"],
  ["wholeModel", "Whole model"],
  ["perMesh", "Per mesh"],
] as const;

/** One frameless bracketed label opening a local dropdown; closes on
 * outside pointerdown, Esc, or picking a non-sticky item. */
function GhostMenu({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!(e.target instanceof Element) || !rootRef.current?.contains(e.target)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="ghost-menu">
      <button
        type="button"
        className={`ghost-label${open ? " open" : ""}`}
        onClick={() => setOpen((o) => !o)}
      >
        [ {label} ]
      </button>
      {open && (
        <div className="ghost-dropdown" onClick={() => setOpen(false)}>
          {children}
        </div>
      )}
    </div>
  );
}

function GhostItem({
  label,
  checked,
  onPick,
  sticky,
}: {
  label: string;
  checked?: boolean;
  onPick: () => void;
  /** Sticky items keep the dropdown open (checkable toggles). */
  sticky?: boolean;
}) {
  return (
    <button
      type="button"
      className="ghost-item"
      onClick={(e) => {
        if (sticky) e.stopPropagation();
        onPick();
      }}
    >
      <span className="ghost-check">{checked && <IconCheck size={11} />}</span>
      {label}
    </button>
  );
}

function GhostHeading({ label }: { label: string }) {
  return <div className="ghost-heading">{label}</div>;
}

function labelOf<T extends readonly (readonly [string, string])[]>(
  table: T,
  value: string,
): string {
  return table.find(([v]) => v === value)?.[1] ?? value;
}

function PaneControls({ pane, settings, projection, active }: {
  pane: number;
  settings: PaneDisplaySettings;
  projection: string;
  active: boolean;
}) {
  const overlapPct = useViewState((s) => s.uvOverlapPct);
  const overlapPending = useViewState((s) => s.uvOverlapPending);
  // Select the STABLE nodes array and derive with useMemo: a selector that
  // filters inline returns a fresh array every snapshot read, which
  // useSyncExternalStore sees as perpetually changed (React #185 loop).
  const rootNodes = useMirror((s) => selectGraph(s, "root").nodes);
  const cameras = useMemo(() => rootNodes.filter((n) => n.typeId === "camera"), [rootNodes]);
  const lookThrough = useViewState((s) => s.view?.paneLookThrough?.[pane] ?? null);
  const cameraLocked = useViewState((s) => s.view?.paneCameraLock?.[pane] ?? false);
  const patch = (p: Partial<PaneDisplaySettings>) => {
    setActivePane(pane);
    setPaneSettings(pane, { ...settings, ...p });
  };

  // A UV pane swaps the 3D controls for the UV set: the label, a Display
  // dropdown (background, overlap toggle, exit), and the live overlap
  // percentage when enabled.
  if (settings.paneMode === "UvMap") {
    return (
      <div
        className={`pane-controls${active ? " active" : ""}`}
        onPointerDown={(e) => e.stopPropagation()}
      >
        <span className="ghost-label static">[ UV Map ]</span>
        <GhostMenu label="Display">
          <GhostHeading label="Background" />
          {(["Checker", "Dark", "Charcoal"] as const).map((bg) => (
            <GhostItem
              key={bg}
              label={bg}
              checked={settings.uvBg === bg}
              sticky
              onPick={() => patch({ uvBg: bg })}
            />
          ))}
          <GhostHeading label="Overlays" />
          <GhostItem
            label="UV overlap"
            checked={settings.showUvOverlap}
            sticky
            onPick={() => patch({ showUvOverlap: !settings.showUvOverlap })}
          />
          <GhostItem label="Exit UV Layout" onPick={() => patch({ paneMode: "Scene3D" })} />
        </GhostMenu>
        {settings.showUvOverlap && (
          <span className="ghost-info" title="UV overlap percentage">
            {overlapPending || overlapPct === null
              ? "computing..."
              : `${overlapPct.toFixed(1)}% overlap`}
          </span>
        )}
      </div>
    );
  }

  return (
    <div
      className={`pane-controls${active ? " active" : ""}`}
      onPointerDown={(e) => e.stopPropagation()}
    >
      <GhostMenu label={labelOf(VIEW_MODES, settings.viewMode)}>
        {VIEW_MODES.map(([v, label]) => (
          <GhostItem
            key={v}
            label={label}
            checked={settings.viewMode === v}
            onPick={() => patch({ viewMode: v })}
          />
        ))}
      </GhostMenu>
      {settings.inspectionMode !== "Shaded" && (
        <GhostMenu label={labelOf(INSPECTION_MODES, settings.inspectionMode)}>
          {INSPECTION_MODES.map(([v, label]) => (
            <GhostItem
              key={v}
              label={label}
              checked={settings.inspectionMode === v}
              onPick={() => patch({ inspectionMode: v })}
            />
          ))}
        </GhostMenu>
      )}
      {settings.inspectionMode === "Shaded" && (
        <GhostMenu label="Inspect">
          {INSPECTION_MODES.map(([v, label]) => (
            <GhostItem
              key={v}
              label={label}
              checked={settings.inspectionMode === v}
              onPick={() => patch({ inspectionMode: v })}
            />
          ))}
        </GhostMenu>
      )}
      <GhostMenu
        label={
          settings.materialOverride === "None"
            ? "Override"
            : labelOf(MATERIAL_OVERRIDES, settings.materialOverride)
        }
      >
        {MATERIAL_OVERRIDES.map(([v, label]) => (
          <GhostItem
            key={v}
            label={label}
            checked={settings.materialOverride === v}
            onPick={() => patch({ materialOverride: v })}
          />
        ))}
      </GhostMenu>
      <GhostMenu label={projection === "orthographic" ? "Ortho" : "Persp"}>
        <GhostItem
          label="Perspective"
          checked={projection === "perspective"}
          onPick={() => {
            setActivePane(pane);
            cameraCommand(pane, { kind: "projection", mode: "perspective" });
          }}
        />
        <GhostItem
          label="Orthographic"
          checked={projection === "orthographic"}
          onPick={() => {
            setActivePane(pane);
            cameraCommand(pane, { kind: "projection", mode: "orthographic" });
          }}
        />
      </GhostMenu>
      <GhostMenu label={lookThrough !== null ? "Camera*" : "Camera"}>
        <GhostItem
          label="Free view"
          checked={lookThrough === null}
          onPick={() => {
            setActivePane(pane);
            setPaneCamera(pane, -1);
          }}
        />
        {cameras.length > 0 && <GhostHeading label="Look through" />}
        {cameras.map((c) => (
          <GhostItem
            key={c.id}
            label={cameraName(c)}
            checked={lookThrough === c.id}
            onPick={() => {
              setActivePane(pane);
              setPaneCamera(pane, c.id);
            }}
          />
        ))}
        {lookThrough !== null && (
          <GhostItem
            label="Lock camera to view"
            checked={cameraLocked}
            sticky
            onPick={() => setPaneCameraLock(pane, !cameraLocked)}
          />
        )}
        <GhostHeading label="Bookmarks" />
        {cameras.map((c) => (
          <GhostItem
            key={`jump-${c.id}`}
            label={`Jump to ${cameraName(c)}`}
            onPick={() => {
              setActivePane(pane);
              jumpToCamera(pane, c.id);
            }}
          />
        ))}
        <GhostItem
          label="Create camera from view"
          onPick={() => {
            setActivePane(pane);
            createCameraFromView(pane);
          }}
        />
      </GhostMenu>
      <GhostMenu label="Views">
        <GhostItem
          label="Fit view (F)"
          onPick={() => {
            setActivePane(pane);
            cameraCommand(pane, { kind: "fit" });
          }}
        />
        {VIEW_AXES.map(([axis, label]) => (
          <GhostItem
            key={axis}
            label={label}
            onPick={() => {
              setActivePane(pane);
              cameraCommand(pane, { kind: "view", axis });
            }}
          />
        ))}
      </GhostMenu>
      <GhostMenu label="Display">
        <GhostItem
          label="Grid"
          checked={settings.showGrid}
          sticky
          onPick={() => patch({ showGrid: !settings.showGrid })}
        />
        <GhostItem
          label="Axes"
          checked={settings.showAxisGizmo}
          sticky
          onPick={() => patch({ showAxisGizmo: !settings.showAxisGizmo })}
        />
        <GhostItem
          label="Validation overlay"
          checked={settings.showValidation}
          sticky
          onPick={() => patch({ showValidation: !settings.showValidation })}
        />
        <GhostItem
          label="Turntable"
          checked={settings.turntableActive}
          sticky
          onPick={() => patch({ turntableActive: !settings.turntableActive })}
        />
        <GhostHeading label="Normals" />
        {NORMALS.map(([v, label]) => (
          <GhostItem
            key={v}
            label={label}
            checked={settings.normalsMode === v}
            sticky
            onPick={() => patch({ normalsMode: v })}
          />
        ))}
        <GhostHeading label="Bounds" />
        {BOUNDS.map(([v, label]) => (
          <GhostItem
            key={v}
            label={label}
            checked={settings.boundsMode === v}
            sticky
            onPick={() => patch({ boundsMode: v })}
          />
        ))}
        <GhostHeading label="Background" />
        {BACKGROUNDS.map(([v, label]) => (
          <GhostItem
            key={v}
            label={label}
            checked={settings.backgroundMode === v}
            sticky
            onPick={() => patch({ backgroundMode: v })}
          />
        ))}
        <GhostHeading label="Pane" />
        <GhostItem label="UV Layout (3)" onPick={() => patch({ paneMode: "UvMap" })} />
      </GhostMenu>
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

/** The letterbox framing gate for a look-through pane: the camera's aspect
 * rectangle centered in the pane, everything outside it dimmed (clipped to the
 * pane via overflow). Pure overlay; never intercepts pointer events. */
function FramingGate({ rect, aspect }: { rect: PaneRectDto; aspect: number }) {
  const paneAspect = rect.width / Math.max(rect.height, 1);
  let gw = rect.width;
  let gh = rect.height;
  if (aspect > paneAspect) {
    gw = rect.width;
    gh = rect.width / aspect;
  } else {
    gh = rect.height;
    gw = rect.height * aspect;
  }
  return (
    <div
      className="framing-gate-clip"
      style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
    >
      <div
        className="framing-gate"
        style={{ left: (rect.width - gw) / 2, top: (rect.height - gh) / 2, width: gw, height: gh }}
      />
    </div>
  );
}

/** All pane controls, absolutely positioned from the host pane rects. */
export function PaneToolbars() {
  const view = useViewState((s) => s.view);
  if (!view) return null;
  return (
    <>
      {view.paneRects.map((rect, i) => {
        const aspect = view.paneGateAspect?.[i];
        return aspect ? <FramingGate key={`gate-${i}`} rect={rect} aspect={aspect} /> : null;
      })}
      {view.paneRects.map((rect, i) => (
        <div
          key={i}
          className="pane-toolbar-anchor"
          style={{ left: rect.x, top: rect.y, width: rect.width }}
        >
          <PaneControls
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
