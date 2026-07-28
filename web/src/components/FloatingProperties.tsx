// The floating parameter editor: P over the node canvas.
//
// Hosts the real `ParameterPanel`, the same component the dock mounts.
// That is the entire point: one editor, two hosts, so a widget added to the
// panel appears in both without anybody remembering to do it twice.
//
// Built on `useDragResize` rather than `Modal`, because it is modeless: no
// backdrop, no focus trap, and the canvas underneath stays fully usable
// while it is open (which is what makes it useful for comparing two nodes,
// one pinned here and one following the selection in the dock).
//
// Distinct from `NodeInfoModal` (the I key), which is read-only and tells
// you what a node IS. This one edits.

import { useEffect } from "react";
import { useDragResize } from "../hooks/useDragResize";
import { selectGraph, useMirror } from "../store/mirror";
import { useUi } from "../store/ui";
import { ParameterPanel } from "./ParameterPanel";

export function FloatingProperties() {
  const open = useUi((s) => s.floatingProps);
  const pinned = useUi((s) => s.propsPin.floating);
  const selection = useMirror((s) => selectGraph(s, s.current).selection);
  const { ref, style, headerProps, resizeProps } = useDragResize({
    id: "floating-props",
    minWidth: 300,
    minHeight: 220,
  });

  // Escape closes, but only when nothing more urgent wants it: the cancel
  // ladder (gizmo drag, review draft, maximized panel) is handled by the
  // global keymap, so this listens WITHOUT capture and lets those run
  // first.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") useUi.getState().setFloatingProps(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open) return null;

  const canPin = pinned !== null || selection.length > 0;
  const togglePin = () => {
    useUi.getState().setPropsPin("floating", pinned !== null ? null : (selection[0] ?? null));
  };

  return (
    <div ref={ref} className="floating-props" style={style} role="dialog" aria-label="Properties">
      <div className="floating-props-header" {...headerProps}>
        <span className="floating-props-title">Properties</span>
        <span className="spacer" />
        <button
          type="button"
          className={`floating-props-btn${pinned !== null ? " active" : ""}`}
          aria-pressed={pinned !== null}
          title={
            pinned !== null
              ? "Pinned: this panel stays on one node. Click to follow the selection."
              : "Pin to the selected node so it stops following the selection"
          }
          disabled={!canPin}
          onClick={togglePin}
        >
          <PinGlyph filled={pinned !== null} />
        </button>
        <button
          type="button"
          className="floating-props-btn"
          title="Close (P)"
          aria-label="Close"
          onClick={() => useUi.getState().setFloatingProps(false)}
        >
          <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden>
            <path
              d="M4 4l8 8M12 4l-8 8"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              fill="none"
            />
          </svg>
        </button>
      </div>
      <div className="floating-props-body">
        <ParameterPanel surface="floating" />
      </div>
      <div className="modal-resize" {...resizeProps} aria-hidden />
    </div>
  );
}

/** The pin, filled when engaged. Inline SVG like every other glyph here, so
 * it inherits `currentColor` and needs no icon font. */
function PinGlyph({ filled }: { filled: boolean }) {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden>
      <path
        d="M6 1.5h4l-.6 4.2 2.6 2.3v1.2H8.7V15L8 15.7 7.3 15V9.2H3.9V8l2.7-2.3z"
        fill={filled ? "currentColor" : "none"}
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  );
}
