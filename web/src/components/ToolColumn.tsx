// The viewport tool column (Phase 11): a Blender-style strip of square buttons
// down the left edge of the 3D region.
//
// An OVERLAY inside the canvas host, not a DOM column beside it, so it never
// shrinks the canvas and therefore never perturbs the Rust pane rects (which the
// ghost toolbars and picking are positioned from).
//
// All four tools are live as of Phase 12: Select (Q), Move (W), Rotate (E),
// Scale (R). Rotate and Scale shipped disabled in Phase 11 so the column's final
// shape was honest from day one; this is that promise being kept.

import { setTool } from "../engine/session";
import type { ToolMode } from "../engine/types";
import { useViewState } from "../store/viewState";

interface Tool {
  mode: ToolMode;
  label: string;
  hotkey: string;
  /** The glyph. Deliberately simple shapes: at 28px an icon font would be mush. */
  icon: React.ReactNode;
  enabled: boolean;
}

const CURSOR = (
  <svg width="19" height="19" viewBox="0 0 14 14" aria-hidden>
    <path d="M2 1 L2 11 L4.6 8.6 L6.4 12.6 L8.2 11.8 L6.4 7.9 L10 7.6 Z" fill="currentColor" />
  </svg>
);

const MOVE = (
  <svg width="19" height="19" viewBox="0 0 14 14" aria-hidden>
    <path
      d="M7 1 V13 M1 7 H13 M7 1 L5.2 3 M7 1 L8.8 3 M7 13 L5.2 11 M7 13 L8.8 11 M1 7 L3 5.2 M1 7 L3 8.8 M13 7 L11 5.2 M13 7 L11 8.8"
      stroke="currentColor"
      strokeWidth="1.2"
      fill="none"
      strokeLinecap="round"
    />
  </svg>
);

const ROTATE = (
  <svg width="19" height="19" viewBox="0 0 14 14" aria-hidden>
    <path
      d="M11.5 5.5 A5 5 0 1 1 9 2.2"
      stroke="currentColor"
      strokeWidth="1.2"
      fill="none"
      strokeLinecap="round"
    />
    <path d="M9.2 1 L9.6 4 L6.7 3.2 Z" fill="currentColor" />
  </svg>
);

const SCALE = (
  <svg width="19" height="19" viewBox="0 0 14 14" aria-hidden>
    <path d="M2 12 L12 2" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
    <rect x="1" y="9" width="4" height="4" fill="currentColor" />
    <rect x="9.5" y="1.5" width="3" height="3" fill="none" stroke="currentColor" strokeWidth="1.2" />
  </svg>
);

const TOOLS: Tool[] = [
  { mode: "select", label: "Select", hotkey: "Q", icon: CURSOR, enabled: true },
  { mode: "move", label: "Move", hotkey: "W", icon: MOVE, enabled: true },
  { mode: "rotate", label: "Rotate", hotkey: "E", icon: ROTATE, enabled: true },
  { mode: "scale", label: "Scale", hotkey: "R", icon: SCALE, enabled: true },
];

function ToolButton({ t, active }: { t: Tool; active: boolean }) {
  return (
    <button
      type="button"
      className={`tool-btn${active ? " active" : ""}`}
      disabled={!t.enabled}
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
