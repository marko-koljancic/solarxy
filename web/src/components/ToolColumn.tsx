// The viewport tool column: a Blender-style strip of square buttons down the
// left edge of the 3D region.
//
// An OVERLAY inside the canvas host, not a DOM column beside it, so it never
// shrinks the canvas and therefore never perturbs the Rust pane rects (which
// the ghost toolbars and picking are positioned from).
//
// Which tools are usable is the SELECTION's answer, not this component's: the
// host reports the set that applies as it changes, and a tool outside it
// renders disabled rather than dead. A point light has no rotation and no
// size, so arming Rotate on one would draw handles that write nowhere.

import {
  IconToolAim,
  IconToolMove,
  IconToolRotate,
  IconToolScale,
  IconToolSelect,
} from "../icons";
import { setTool } from "../engine/session";
import type { ToolMode } from "../engine/types";
import { toolApplies, useViewState } from "../store/viewState";

interface Tool {
  mode: ToolMode;
  label: string;
  /** The key that arms it, or null for a tool the keymap does not bind. */
  hotkey: string | null;
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
  // Last, so Q/W/E/R keep the positions people reach for by muscle memory.
  // No hotkey: every free letter over the viewport is a poor mnemonic, and a
  // fifth tool is not worth spending one on.
  { mode: "aim", label: "Aim", hotkey: null, icon: <IconToolAim size={GLYPH} /> },
];

function ToolButton({
  t,
  active,
  enabled,
}: {
  t: Tool;
  active: boolean;
  enabled: boolean;
}) {
  const title = t.hotkey ? `${t.label} (${t.hotkey})` : t.label;
  return (
    <button
      type="button"
      className={`tool-btn${active ? " active" : ""}`}
      title={enabled ? title : `${title} — not available for this selection`}
      aria-label={t.label}
      aria-pressed={active}
      disabled={!enabled}
      onClick={() => setTool(t.mode)}
    >
      {t.icon}
    </button>
  );
}

export function ToolColumn() {
  const tool = useViewState((s) => s.toolMode);
  const available = useViewState((s) => s.selectionTools);
  // Blender groups the selection tool apart from the transform trio; the first
  // entry is Select, the rest are the transform tools.
  const [select, ...transforms] = TOOLS;

  return (
    <div className="tool-column" role="toolbar" aria-label="Transform tools">
      <div className="tool-group">
        <ToolButton t={select} active={tool === select.mode} enabled />
      </div>
      <div className="tool-group">
        {transforms.map((t) => {
          const enabled = toolApplies(t.mode, available);
          return (
            <ToolButton
              key={t.mode}
              t={t}
              // A tool that cannot act must not look armed, even while it
              // stays armed underneath: selecting a mesh again finds Scale
              // exactly as you left it.
              active={tool === t.mode && enabled}
              enabled={enabled}
            />
          );
        })}
      </div>
    </div>
  );
}
