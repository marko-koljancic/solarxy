// Per-pane ghost-text viewport controls floated over the WebGPU canvas
// (D2, Minimystix ViewportControls / desktop label-menu pattern):
// frameless bracketed labels that open small local dropdowns, replacing
// the filled toolbar strip. Pure interpreters of the view-state mirror;
// every change goes through the session's view actions (Rust owns the
// truth). Positioned from the host-computed pane rects.

import {
  createContext,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  cameraCommand,
  createCameraFromView,
  jumpToCamera,
  setActivePane,
  setDisplaySettings,
  setPaneCamera,
  setPaneCameraLock,
  setPaneSettings,
  setSplitRatio,
} from "../engine/session";
import { PaneLookModal } from "./PaneLookModal";
import { TURNTABLE_SPEEDS, turntableSpeedLabel } from "./turntableSpeeds";
import type {
  NodeMirror,
  PaneDisplaySettings,
  PaneRectDto,
  ViewAxis,
  ViewStateDto,
} from "../engine/types";
import { IconCheck, IconChevronRight } from "../icons";
import { selectGraph, useMirror } from "../store/mirror";
import { useViewState } from "../store/viewState";
import { DropdownPortal } from "./DropdownPortal";

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

// Temporary per-pane shading overrides: the desktop set, wired to
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
  ["top", "Top (T)"],
  ["bottom", "Bottom (B)"],
  ["front", "Front (F)"],
  ["back", "Back"],
  ["left", "Left (L)"],
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

/** Wireframe stroke weight. Per pane, like every other entry in this menu, so
 * a split view can compare weights. The px figures are
 * `solarxy_core::preferences::LineWeight::width_px`; they are in the labels
 * because "Light" alone does not tell you what you are choosing between. */
const LINE_WEIGHTS = [
  ["Light", "Light (1 px)"],
  ["Medium", "Medium (2 px)"],
  ["Bold", "Bold (3 px)"],
] as const;

/** Submenu flyouts and their parent panel are one menu tree: the portal's
 * outside-close treats every panel with the tree's id as inside, and a
 * non-sticky pick anywhere closes the whole tree. */
const GhostMenuCtx = createContext<{ treeId: string; close: () => void } | null>(null);

/** One frameless bracketed label opening a local dropdown; closes on
 * outside pointerdown, Esc, or picking a non-sticky item. The panel is
 * portaled (DropdownPortal) so it paints above the sibling overlay
 * layers the toolbar anchor's stacking context sits below. */
function GhostMenu({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const treeId = useId();
  const anchorRef = useRef<HTMLButtonElement>(null);
  const close = () => setOpen(false);

  return (
    <div className="ghost-menu">
      <button
        ref={anchorRef}
        type="button"
        className={`ghost-label${open ? " open" : ""}`}
        onClick={() => setOpen((o) => !o)}
      >
        [ {label} ]
      </button>
      {open && (
        <GhostMenuCtx.Provider value={{ treeId, close }}>
          <DropdownPortal anchorRef={anchorRef} treeId={treeId} onClose={close}>
            <div className="ghost-dropdown" onClick={close}>
              {children}
            </div>
          </DropdownPortal>
        </GhostMenuCtx.Provider>
      )}
    </div>
  );
}

const SUBMENU_CLOSE_MS = 150;

/** A submenu row inside a GhostMenu: hover or click opens a side flyout
 * (portaled; the panel's own overflow scroll would clip an inline one).
 * The row shows the current value as a dim hint so the collapsed menu
 * still answers "what is this set to" at a glance. */
function GhostSubmenu({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  const ctx = useContext(GhostMenuCtx);
  const [open, setOpen] = useState(false);
  const rowRef = useRef<HTMLButtonElement>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelClose = () => {
    if (closeTimer.current) {
      clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  };
  const scheduleClose = () => {
    cancelClose();
    closeTimer.current = setTimeout(() => setOpen(false), SUBMENU_CLOSE_MS);
  };
  useEffect(() => cancelClose, []);

  return (
    <>
      <button
        ref={rowRef}
        type="button"
        className={`ghost-item ghost-submenu-row${open ? " open" : ""}`}
        onClick={(e) => {
          e.stopPropagation();
          cancelClose();
          setOpen((o) => !o);
        }}
        onPointerEnter={() => {
          cancelClose();
          setOpen(true);
        }}
        onPointerLeave={scheduleClose}
      >
        <span className="ghost-check" />
        {label}
        {hint && <span className="ghost-submenu-hint">{hint}</span>}
        <span className="ghost-submenu-caret">
          <IconChevronRight size={11} />
        </span>
      </button>
      {open && (
        <DropdownPortal
          anchorRef={rowRef}
          placement="side"
          treeId={ctx?.treeId}
          onClose={() => setOpen(false)}
          onPointerEnter={cancelClose}
          onPointerLeave={scheduleClose}
        >
          <div className="ghost-dropdown ghost-flyout" onClick={() => ctx?.close()}>
            {children}
          </div>
        </DropdownPortal>
      )}
    </>
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
  // The global display settings: the turntable-speed submenu writes the
  // scene-wide rpm (per-pane is only the on/off toggle).
  const display = useViewState((s) => s.view?.display);
  const [lookOpen, setLookOpen] = useState(false);
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
          label="Perspective (P)"
          checked={projection === "perspective"}
          onPick={() => {
            setActivePane(pane);
            cameraCommand(pane, { kind: "projection", mode: "perspective" });
          }}
        />
        <GhostItem
          label="Orthographic (O)"
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
          label="Fit view (Z)"
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
          label="Look..."
          onPick={() => {
            setActivePane(pane);
            setLookOpen(true);
          }}
        />
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
        {display && (
          <GhostSubmenu label="Turntable speed" hint={turntableSpeedLabel(display.turntableRpm)}>
            {TURNTABLE_SPEEDS.map(([label, rpm]) => (
              <GhostItem
                key={label}
                label={label}
                checked={Math.abs(display.turntableRpm - rpm) < 0.01}
                sticky
                onPick={() => setDisplaySettings({ ...display, turntableRpm: rpm })}
              />
            ))}
          </GhostSubmenu>
        )}
        <GhostSubmenu label="Normals" hint={labelOf(NORMALS, settings.normalsMode)}>
          {NORMALS.map(([v, label]) => (
            <GhostItem
              key={v}
              label={label}
              checked={settings.normalsMode === v}
              sticky
              onPick={() => patch({ normalsMode: v })}
            />
          ))}
        </GhostSubmenu>
        <GhostSubmenu label="Bounds" hint={labelOf(BOUNDS, settings.boundsMode)}>
          {BOUNDS.map(([v, label]) => (
            <GhostItem
              key={v}
              label={label}
              checked={settings.boundsMode === v}
              sticky
              onPick={() => patch({ boundsMode: v })}
            />
          ))}
        </GhostSubmenu>
        <GhostSubmenu label="Wireframe" hint={labelOf(LINE_WEIGHTS, settings.lineWeight)}>
          {LINE_WEIGHTS.map(([v, label]) => (
            <GhostItem
              key={v}
              label={label}
              checked={settings.lineWeight === v}
              sticky
              onPick={() => patch({ lineWeight: v })}
            />
          ))}
        </GhostSubmenu>
        <GhostSubmenu
          label="Background"
          hint={
            typeof settings.backgroundMode === "string"
              ? labelOf(BACKGROUNDS, settings.backgroundMode)
              : undefined
          }
        >
          {BACKGROUNDS.map(([v, label]) => (
            <GhostItem
              key={v}
              label={label}
              checked={settings.backgroundMode === v}
              sticky
              onPick={() => patch({ backgroundMode: v })}
            />
          ))}
        </GhostSubmenu>
        <GhostItem label="UV Layout (3)" onPick={() => patch({ paneMode: "UvMap" })} />
      </GhostMenu>
      {lookOpen && <PaneLookModal pane={pane} onClose={() => setLookOpen(false)} />}
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
