// Header controls: undo/redo, the auto/manual cook-mode toggle, the Cook
// button (manual), and the stale count.

import { dispatch } from "../engine/session";
import { useMirror } from "../store/mirror";

export function Toolbar() {
  const cookMode = useMirror((s) => s.cookMode);
  const staleCount = useMirror((s) => s.stale.length);

  return (
    <div className="toolbar">
      <button className="tbtn" title="Undo (Cmd/Ctrl+Z)" onClick={() => dispatch({ type: "undo" })}>
        ↶
      </button>
      <button className="tbtn" title="Redo (Cmd/Ctrl+Shift+Z)" onClick={() => dispatch({ type: "redo" })}>
        ↷
      </button>
      <span className="tsep" />
      <button
        className="tbtn mode"
        title="Toggle cook mode"
        onClick={() => dispatch({ type: "setCookMode", mode: cookMode === "auto" ? "manual" : "auto" })}
      >
        {cookMode === "auto" ? "Auto" : "Manual"}
      </button>
      {cookMode === "manual" && (
        <>
          {staleCount > 0 && <span className="stale-count">{staleCount} stale</span>}
          <button
            className="tbtn cook"
            title="Cook (Cmd/Ctrl+Enter)"
            disabled={staleCount === 0}
            onClick={() => dispatch({ type: "cookNow" })}
          >
            Cook
          </button>
        </>
      )}
    </div>
  );
}
