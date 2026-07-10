// The bespoke note node (the ONE non-generic node component, mirroring
// Minimystix's NoteNode): an on-canvas sticky rendering the engine's
// existing text/color/width/height params. Double-click edits (Esc
// reverts, Ctrl+Enter or blur commits), the React Flow NodeResizer
// persists size, and the corner swatch cycles a pastel palette. All
// mutations are ordinary setParam commands, so notes undo/redo and
// round-trip .slxy like any node.

import { NodeResizer, type NodeProps } from "@xyflow/react";
import { useEffect, useRef, useState } from "react";
import { dispatch } from "../engine/session";
import type { NodeMirror, ParamSource } from "../engine/types";
import { useMirror } from "../store/mirror";
import type { FlowNodeData } from "./FlowNode";

/** The Minimystix pastel set, first entry matching the catalog default. */
const NOTE_COLORS: [number, number, number][] = [
  [0.992, 0.902, 0.541], // #FDE68A amber (engine default)
  [0.655, 0.953, 0.816], // #A7F3D0 mint
  [0.749, 0.859, 0.996], // #BFDBFE blue
  [0.984, 0.812, 0.91], //  #FBCFE8 pink
  [0.929, 0.914, 0.996], // #EDE9FE violet
  [0.996, 0.894, 0.902], // #FFE4E6 rose
];

function literalOf(node: NodeMirror, key: string): ParamSource | undefined {
  return node.params[key];
}

function textOf(node: NodeMirror): string {
  const p = literalOf(node, "text");
  return p && p.kind === "literal" && p.type === "text" ? p.value : "";
}

function numOf(node: NodeMirror, key: string, fallback: number): number {
  const p = literalOf(node, key);
  return p && p.kind === "literal" && (p.type === "float" || p.type === "int")
    ? Number(p.value)
    : fallback;
}

function colorOf(node: NodeMirror): [number, number, number] {
  const p = literalOf(node, "color");
  if (p && p.kind === "literal" && p.type === "color") {
    return [p.value[0], p.value[1], p.value[2]];
  }
  return NOTE_COLORS[0];
}

function css(rgb: [number, number, number], alpha: number): string {
  const c = rgb.map((v) => Math.round(v * 255));
  return `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${alpha})`;
}

export function NoteNode({ data, selected }: NodeProps & { data: FlowNodeData }) {
  const node = data.node;
  const ctx = useMirror((s) => s.current);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const taRef = useRef<HTMLTextAreaElement>(null);

  const text = textOf(node);
  const width = numOf(node, "width", 160);
  const height = numOf(node, "height", 80);
  const rgb = colorOf(node);

  useEffect(() => {
    if (editing) {
      taRef.current?.focus();
      taRef.current?.select();
    }
  }, [editing]);

  const commitText = () => {
    setEditing(false);
    if (draft !== text) {
      dispatch({
        type: "setParam",
        ctx,
        node: node.id,
        key: "text",
        value: { kind: "literal", type: "text", value: draft },
      });
    }
  };

  const cycleColor = (e: React.MouseEvent) => {
    e.stopPropagation();
    const idx = NOTE_COLORS.findIndex(
      (c) => Math.abs(c[0] - rgb[0]) + Math.abs(c[1] - rgb[1]) + Math.abs(c[2] - rgb[2]) < 0.02,
    );
    const next = NOTE_COLORS[(idx + 1) % NOTE_COLORS.length];
    dispatch({
      type: "setParam",
      ctx,
      node: node.id,
      key: "color",
      value: { kind: "literal", type: "color", value: [next[0], next[1], next[2], 1] },
    });
  };

  return (
    <div
      className={`note-node${selected ? " selected" : ""}`}
      style={{ width, height, background: css(rgb, 0.78) }}
      onDoubleClick={(e) => {
        e.stopPropagation();
        setDraft(text);
        setEditing(true);
      }}
    >
      <NodeResizer
        isVisible={selected}
        minWidth={120}
        minHeight={60}
        color="var(--accent-primary)"
        onResizeEnd={(_e, params) => {
          dispatch({ type: "beginTransaction", label: "resize note" });
          dispatch({
            type: "setParam",
            ctx,
            node: node.id,
            key: "width",
            value: { kind: "literal", type: "float", value: Math.round(params.width) },
          });
          dispatch({
            type: "setParam",
            ctx,
            node: node.id,
            key: "height",
            value: { kind: "literal", type: "float", value: Math.round(params.height) },
          });
          dispatch({ type: "endTransaction" });
        }}
      />
      <button
        className="note-swatch nodrag"
        title="Cycle note color"
        style={{ background: css(rgb, 1) }}
        onClick={cycleColor}
      />
      {editing ? (
        <textarea
          ref={taRef}
          className="note-text-edit nodrag"
          placeholder="Enter note text..."
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commitText}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Escape") {
              setDraft(text);
              setEditing(false);
            }
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) commitText();
          }}
        />
      ) : (
        <div className="note-text">{text || "Double-click to edit"}</div>
      )}
    </div>
  );
}
