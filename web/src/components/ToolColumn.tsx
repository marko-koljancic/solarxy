// The viewport tool column: a Blender-style strip of square buttons down the
// left edge of the 3D region.
//
// An OVERLAY inside the canvas host, not a DOM column beside it, so it never
// shrinks the canvas and therefore never perturbs the Rust pane rects (which
// the ghost toolbars and picking are positioned from).

import {
  IconToolMove,
  IconToolRotate,
  IconToolScale,
  IconToolSelect,
} from "../icons";
import { setTool } from "../engine/session";
import type { ToolMode } from "../engine/types";
import { useViewState } from "../store/viewState";

interface Tool {
  mode: ToolMode;
  label: string;
  hotkey: string;
  /** The glyph. Deliberately simple shapes: at 28px an icon font would be
   * mush. Drawn centred on their viewBox in `icons.tsx` — the button centres
   * the <svg> box, not the artwork inside it. */
  icon: React.ReactNode;
}

const GLYPH = 19;

const TOOLS: Tool[] = [
  { mode: "select", label: "Select", hotkey: "Q", icon: <IconToolSelect size={GLYPH} /> },
  { mode: "move", label: "Move", hotkey: "W", icon: <IconToolMove size={GLYPH} /> },
  { mode: "rotate", label: "Rotate", hotkey: "E", icon: <IconToolRotate size={GLYPH} /> },
  { mode: "scale", label: "Scale", hotkey: "R", icon: <IconToolScale size={GLYPH} /> },
];

function ToolButton({ t, active }: { t: Tool; active: boolean }) {
  return (
    <button
      type="button"
      className={`tool-btn${active ? " active" : ""}`}
      title={`${t.label} (${t.hotkey})`}
      aria-label={t.label}
      aria-pressed={active}
      onClick={() => setTool(t.mode)}
    >
      {t.icon}
    </button>
  );
}

export function ToolColumn() {
  const tool = useViewState((s) => s.toolMode);
  // Blender groups the selection tool apart from the transform trio; the first
  // entry is Select, the rest are Move/Rotate/Scale.
  const [select, ...transforms] = TOOLS;

  return (
    <div className="tool-column" role="toolbar" aria-label="Transform tools">
      <div className="tool-group">
        <ToolButton t={select} active={tool === select.mode} />
      </div>
      <div className="tool-group">
        {transforms.map((t) => (
          <ToolButton key={t.mode} t={t} active={tool === t.mode} />
        ))}
      </div>
    </div>
  );
}
