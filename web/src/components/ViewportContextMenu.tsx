// The viewport right-click context menu. Right-click always opens it
// (the camera never used the right button; orbit is LMB, pan is MMB, zoom is
// scroll). Tool switches mirror the tool column; object actions are disabled
// when the selection can't take them.
//
// Nothing here narrows the selection to a `geo`. It used to, in one
// comparison, which made right-clicking a light a menu of disabled entries the
// moment lights became selectable. What each action needs is asked instead:
// the host says which params the selection's transform is made of, and the
// registry says whether it declares a `visible`. Replacing the geo check with
// a light check would have been the same mistake in a new place.
//
// Pure interpreter of the mirror + view state; every action goes through the
// session (Rust owns the truth). Positioned fixed at the click point.

import { useEffect, useRef } from "react";
import { cameraCommand, dispatch, duplicateSelection, setTool } from "../engine/session";
import type { ParamSource, ToolMode } from "../engine/types";
import { selectGraph, useMirror } from "../store/mirror";
import { toolApplies, useViewState } from "../store/viewState";

/** Mode, label, and the key that arms it (blank for a tool the keymap does
 * not bind). Mirrors the tool column, including its order. */
const TOOLS: [ToolMode, string, string][] = [
  ["select", "Select", "Q"],
  ["move", "Move", "W"],
  ["rotate", "Rotate", "E"],
  ["scale", "Scale", "R"],
  ["aim", "Aim", ""],
];

const boolSrc = (value: boolean): ParamSource => ({ kind: "literal", type: "bool", value });

export function ViewportContextMenu({
  x,
  y,
  onClose,
}: {
  x: number;
  y: number;
  onClose: () => void;
}) {
  const rootGraph = useMirror((s) => selectGraph(s, "root"));
  const tool = useViewState((s) => s.toolMode);
  const availableTools = useViewState((s) => s.selectionTools);
  const transformParams = useViewState((s) => s.selectionTransformParams);
  const registry = useMirror((s) => s.registry);
  const activePane = useViewState((s) => s.view?.activePane ?? 0);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      if (!(e.target instanceof Element) || !ref.current?.contains(e.target)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  const sel = rootGraph.selection;
  const hasSel = sel.length > 0;
  const selNode = sel.length === 1 ? rootGraph.nodes.find((n) => n.id === sel[0]) : undefined;

  // Hiding asks the registry whether this node type declares a `visible`,
  // which a geo and every light do. Registry-driven like the palette and the
  // parameter panel, so a node type that gains one needs no change here.
  const declaresVisible =
    selNode !== undefined &&
    (registry?.nodes
      .find((n) => n.typeId === selNode.typeId)
      ?.params.some((p) => p.key === "visible") ??
      false);
  const visP = declaresVisible ? selNode?.params.visible : undefined;
  const visible = visP?.kind === "literal" && visP.type === "bool" ? visP.value : true;

  // Resetting asks the host which params this selection's transform is made
  // of, which is the same answer the manipulator writes through, so a reset
  // undoes exactly what the handles do. A point light's is one param; a geo's
  // is four.
  const canReset = selNode !== undefined && transformParams.length > 0;

  const run = (fn: () => void) => {
    fn();
    onClose();
  };

  const toggleVisible = () => {
    if (!selNode || !declaresVisible) return;
    dispatch({
      type: "setParam",
      ctx: "root",
      node: selNode.id,
      key: "visible",
      value: boolSrc(!visible),
    });
  };

  // One command, one undo step, and it REMOVES the overrides rather than
  // writing the defaults back as literals: the document is left honestly
  // unset, and the values it falls back to are the descriptor's own, so this
  // needs no table of what a default is per node type.
  const resetTransform = () => {
    if (!selNode || !canReset) return;
    dispatch({
      type: "resetParams",
      ctx: "root",
      node: selNode.id,
      keys: transformParams,
    });
  };

  return (
    <div
      ref={ref}
      className="viewport-context-menu"
      style={{ position: "fixed", left: x, top: y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <div className="ctx-heading">Tools</div>
      {TOOLS.map(([mode, label, key]) => {
        // Same rule as the tool column: a tool the selection cannot use shows
        // as unavailable rather than looking armed.
        const enabled = toolApplies(mode, availableTools);
        return (
          <button
            key={mode}
            type="button"
            className={`ctx-item${tool === mode && enabled ? " active" : ""}`}
            disabled={!enabled}
            onClick={() => run(() => setTool(mode))}
          >
            <span>{label}</span>
            <span className="ctx-key">{key}</span>
          </button>
        );
      })}
      <div className="ctx-sep" />
      <button
        type="button"
        className="ctx-item"
        onClick={() => run(() => cameraCommand(activePane, { kind: "fit" }))}
      >
        <span>Frame view</span>
        {/* Z, not F: over the viewport F is the Front view, and this chip has
            been naming the wrong key since Fit moved off it. */}
        <span className="ctx-key">Z</span>
      </button>
      {/* The action for a light dragged out of frame, which is otherwise hard
          to recover. No key of its own: Z keeps framing the whole scene. */}
      <button
        type="button"
        className="ctx-item"
        disabled={!hasSel}
        onClick={() => run(() => cameraCommand(activePane, { kind: "fitSelection" }))}
      >
        Frame selection
      </button>
      <div className="ctx-sep" />
      <button type="button" className="ctx-item" disabled={!hasSel} onClick={() => run(duplicateSelection)}>
        Duplicate
      </button>
      <button
        type="button"
        className="ctx-item"
        disabled={!hasSel}
        onClick={() => run(() => dispatch({ type: "removeNodes", ctx: "root", ids: sel }))}
      >
        Delete
      </button>
      <div className="ctx-sep" />
      <button
        type="button"
        className="ctx-item"
        disabled={!declaresVisible}
        onClick={() => run(toggleVisible)}
      >
        {visible ? "Hide" : "Show"}
      </button>
      <button
        type="button"
        className="ctx-item"
        disabled={!canReset}
        onClick={() => run(resetTransform)}
      >
        Reset transform
      </button>
    </div>
  );
}
