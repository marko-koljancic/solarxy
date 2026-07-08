// The one generic node component: every node type renders through it, its
// ports and colors derived from the registry snapshot (so a node added in
// Rust needs no new component). Only bespoke nodes (e.g. a note) would ever
// need their own component.

import { Handle, Position, type NodeProps } from "@xyflow/react";
import { descriptorFor, DATA_TYPE_COLOR, dataTypeShape } from "../registry/datatypes";
import type { NodeMirror, PortSnapshot } from "../engine/types";
import { useMirror } from "../store/mirror";

export interface FlowNodeData {
  node: NodeMirror;
  isDisplay: boolean;
  [key: string]: unknown;
}

/** Evenly-spaced vertical offset (in %) for a handle in a column. */
function handleTop(index: number, count: number): string {
  return `${((index + 1) / (count + 1)) * 100}%`;
}

function handleStyle(color: string, shape: string): React.CSSProperties {
  const base: React.CSSProperties = {
    width: 11,
    height: 11,
    background: color,
    border: "2px solid #0d1017",
  };
  if (shape === "diamond") return { ...base, transform: "rotate(45deg)", borderRadius: 2 };
  if (shape === "square") return { ...base, borderRadius: 2 };
  return { ...base, borderRadius: "50%" };
}

export function FlowNode({ data, selected }: NodeProps & { data: FlowNodeData }) {
  const node = data.node;
  const registry = useMirror((s) => s.registry);
  const cook = useMirror((s) => s.cook[node.id]);
  const stale = useMirror((s) => s.cookMode === "manual" && s.stale.includes(node.id));

  const desc = descriptorFor(registry, node.typeId);
  const title = desc?.displayName ?? node.typeId;
  const inputs = desc?.inputs ?? [];
  const outputs = desc?.outputs ?? [];
  const isContainer = desc?.category === "container";
  const isDisplay = data.isDisplay;

  const status = cook?.status;
  const badge =
    status?.state === "error" ? "#e5484d" : status?.state === "cooking" ? "#ffcc66" : null;

  return (
    <div
      className={`flow-node${selected ? " selected" : ""}${node.bypassed ? " bypassed" : ""}${stale ? " stale" : ""}`}
      style={{ borderRadius: isContainer ? 4 : 14 }}
      title={desc?.doc}
    >
      {stale && <span className="stale-tag" title="stale (edit not yet cooked)" />}
      {status?.state === "error" && (
        <span className="err-tag" title={status.message}>
          !
        </span>
      )}
      {inputs.map((p: PortSnapshot, i: number) => (
        <Handle
          key={`in-${p.key}`}
          type="target"
          position={Position.Left}
          id={p.key}
          style={{ ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType)), top: handleTop(i, inputs.length) }}
          title={`${p.label} (${p.dataType}${p.variadic ? ", variadic" : ""})`}
          isConnectable
        />
      ))}

      <div className="flow-node-title">
        {isDisplay && <span className="display-dot" title="display flag" />}
        <span>{title}</span>
        {badge && <span className="cook-badge" style={{ background: badge }} />}
        {isContainer && (
          <button
            className="enter-subflow"
            title="Enter subflow (or double-click)"
            onClick={(e) => {
              e.stopPropagation();
              useMirror.getState().setCurrent({ subflow: node.id });
            }}
          >
            ↳
          </button>
        )}
      </div>
      {status?.state === "ok" && status.ms > 0 && (
        <div className="flow-node-sub">{status.ms.toFixed(1)} ms</div>
      )}

      {outputs.map((p: PortSnapshot, i: number) => (
        <Handle
          key={`out-${p.key}`}
          type="source"
          position={Position.Right}
          id={p.key}
          style={{ ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType)), top: handleTop(i, outputs.length) }}
          title={`${p.label} (${p.dataType})`}
          isConnectable
        />
      ))}
    </div>
  );
}
