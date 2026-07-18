// The viewport right-click context menu. Right-click always opens it
// (the camera never used the right button; orbit is LMB, pan is MMB, zoom is
// scroll). Tool switches mirror the tool column; object actions apply to the
// selected root geo and are disabled when the selection can't take them.
//
// Pure interpreter of the mirror + view state; every action goes through the
// session (Rust owns the truth). Positioned fixed at the click point.

import { useEffect, useRef } from "react";
import { cameraCommand, dispatch, duplicateSelection, setTool } from "../engine/session";
import type { ParamSource, ToolMode } from "../engine/types";
import { selectGraph, useMirror } from "../store/mirror";
import { useViewState } from "../store/viewState";

const TOOLS: [ToolMode, string, string][] = [
  ["select", "Select", "Q"],
  ["move", "Move", "W"],
  ["rotate", "Rotate", "E"],
  ["scale", "Scale", "R"],
];

const vec3 = (value: [number, number, number]): ParamSource => ({
  kind: "literal",
  type: "vec3",
  value,
});
const floatSrc = (value: number): ParamSource => ({ kind: "literal", type: "float", value });
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
  // Transform / visibility actions only make sense on a geo container.
  const geo = selNode?.typeId === "geo" ? selNode : undefined;
  const visP = geo?.params.visible;
  const visible = visP?.kind === "literal" && visP.type === "bool" ? visP.value : true;

  const run = (fn: () => void) => {
    fn();
    onClose();
  };

  const toggleVisible = () => {
    if (!geo) return;
    dispatch({ type: "setParam", ctx: "root", node: geo.id, key: "visible", value: boolSrc(!visible) });
  };

  const resetTransform = () => {
    if (!geo) return;
    // One undo step for the whole reset.
    dispatch({ type: "beginTransaction", label: "Reset Transform" });
    dispatch({ type: "setParam", ctx: "root", node: geo.id, key: "translate", value: vec3([0, 0, 0]) });
    dispatch({ type: "setParam", ctx: "root", node: geo.id, key: "rotate", value: vec3([0, 0, 0]) });
    dispatch({ type: "setParam", ctx: "root", node: geo.id, key: "scale", value: vec3([1, 1, 1]) });
    dispatch({ type: "setParam", ctx: "root", node: geo.id, key: "uniform_scale", value: floatSrc(1) });
    dispatch({ type: "endTransaction" });
  };

  return (
    <div
      ref={ref}
      className="viewport-context-menu"
      style={{ position: "fixed", left: x, top: y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <div className="ctx-heading">Tools</div>
      {TOOLS.map(([mode, label, key]) => (
        <button
          key={mode}
          type="button"
          className={`ctx-item${tool === mode ? " active" : ""}`}
          onClick={() => run(() => setTool(mode))}
        >
          <span>{label}</span>
          <span className="ctx-key">{key}</span>
        </button>
      ))}
      <div className="ctx-sep" />
      <button
        type="button"
        className="ctx-item"
        onClick={() => run(() => cameraCommand(activePane, { kind: "fit" }))}
      >
        <span>Frame view</span>
        <span className="ctx-key">F</span>
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
      <button type="button" className="ctx-item" disabled={!geo} onClick={() => run(toggleVisible)}>
        {visible ? "Hide" : "Show"}
      </button>
      <button type="button" className="ctx-item" disabled={!geo} onClick={() => run(resetTransform)}>
        Reset transform
      </button>
    </div>
  );
}
