// The attributeName widget: a free-text input whose dropdown offers the
// attribute lanes present on the node's upstream geometry. Free text stays
// first-class (reserved names and forward references are legal); the
// completions are a courtesy fetched ON OPEN from the engine's attribute
// summary, never polled. Reuses the Select dropdown's list styling; like
// Select, deliberately not a portal (the panel scrolls and clips
// predictably).
//
// DELIBERATE COUPLING: picking a lane from the dropdown also dispatches
// the node's sibling `type` enum (same group, a variant matching the
// lane's ty) so the write keeps the lane's type without a second edit.
// Typing a name manually never touches Type; there is no batched
// SetParam, so a pick is two undo steps by design.

import { useEffect, useRef, useState } from "react";
import { dispatch, getClient } from "../../engine/session";
import type {
  AttrLane,
  GraphContext,
  NodeMirror,
  NodeTypeSnapshot,
  ParamSnapshot,
} from "../../engine/types";
import { IconChevronDown } from "../../icons";
import { descriptorFor } from "../../registry/datatypes";
import { selectGraph, useMirror } from "../../store/mirror";

/** The enum param a lane pick should retype: key `type`, the SAME group
 * as the name param, with a variant equal to the lane's ty. Null when the
 * node declares nothing matching (then a pick only fills the name). Pure,
 * exported for tests. */
export function siblingTypeParam(
  desc: NodeTypeSnapshot | undefined,
  spec: ParamSnapshot,
  laneTy: string,
): ParamSnapshot | null {
  const p = desc?.params.find(
    (q) =>
      q.key === "type" &&
      q.group === spec.group &&
      q.paramType === "enum" &&
      q.enumVariants.some(([key]) => key === laneTy),
  );
  return p ?? null;
}

/** The upstream node feeding this node's default (else first) Geometry
 * input, resolved through the mirror's edges; null when unwired. */
function upstreamSource(node: NodeMirror): number | null {
  const s = useMirror.getState();
  const desc = descriptorFor(s.registry, node.typeId);
  const input =
    desc?.inputs.find((i) => i.isDefault && i.dataType === "geometry") ??
    desc?.inputs.find((i) => i.dataType === "geometry");
  if (!input) return null;
  const graph = selectGraph(s, s.current);
  const edge = graph.edges.find((e) => e.to === node.id && e.toPort === input.key);
  return edge ? edge.from : null;
}

export function AttributeNameField({
  ctx,
  node,
  spec,
  value,
  onCommit,
}: {
  ctx: GraphContext;
  node: NodeMirror;
  spec: ParamSnapshot;
  value: string;
  onCommit: (v: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [lanes, setLanes] = useState<AttrLane[]>([]);
  const rootRef = useRef<HTMLDivElement>(null);

  const openList = () => {
    const source = upstreamSource(node);
    const summary = source === null ? undefined : getClient().attributeSummary(source);
    setLanes(summary?.point ?? []);
    setOpen(true);
  };

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!(e.target instanceof Node) || !rootRef.current?.contains(e.target)) setOpen(false);
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
    <div ref={rootRef} className="select attr-name-field">
      <input
        type="text"
        className="input-field text-input"
        value={value}
        onChange={(e) => onCommit(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" && !open) {
            e.preventDefault();
            openList();
          }
        }}
      />
      <button
        type="button"
        className={`attr-name-toggle${open ? " open" : ""}`}
        aria-label="Choose an upstream attribute"
        onClick={() => (open ? setOpen(false) : openList())}
      >
        <IconChevronDown size={11} />
      </button>
      {open && (
        <div className="select-list attr-name-list" role="listbox">
          {lanes.length === 0 && (
            <div className="attr-name-empty">No upstream attributes.</div>
          )}
          {lanes.map((lane) => (
            <button
              key={lane.name}
              type="button"
              role="option"
              aria-selected={lane.name === value}
              className={`select-option${lane.name === value ? " active" : ""}`}
              onClick={() => {
                onCommit(lane.name);
                const typeParam = siblingTypeParam(
                  descriptorFor(useMirror.getState().registry, node.typeId),
                  spec,
                  lane.ty,
                );
                if (typeParam) {
                  dispatch({
                    type: "setParam",
                    ctx,
                    node: node.id,
                    key: typeParam.key,
                    value: { kind: "literal", type: "enum", value: lane.ty },
                  });
                }
                setOpen(false);
              }}
            >
              <span className="select-option-label">{lane.name}</span>
              <span className="select-option-hint">{lane.ty}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
