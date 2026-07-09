// The one generic node component: every node type renders through it, its
// ports and colors derived from the registry snapshot (so a node added in
// Rust needs no new component). Minimystix silhouette (phase-6 design
// adoption): a compact pill (square for containers) flowing top-to-bottom
// with the label outside to the right. The UX-spec systems survive the
// restyle: typed handles (color by DataType family plus the round/diamond/
// square shape channel), display-flag dot, cook/stale/error/validation
// badges, and
// bypass hatching.

import { Handle, Position, type NodeProps } from "@xyflow/react";
import { descriptorFor, DATA_TYPE_COLOR, dataTypeShape } from "../registry/datatypes";
import type { NodeMirror, PortSnapshot } from "../engine/types";
import { useMirror } from "../store/mirror";

export interface FlowNodeData {
  node: NodeMirror;
  isDisplay: boolean;
  [key: string]: unknown;
}

/** Evenly-spaced horizontal offset (in %) for a handle in a row. */
function handleLeft(index: number, count: number): string {
  return `${((index + 1) / (count + 1)) * 100}%`;
}

function handleStyle(color: string, shape: string): React.CSSProperties {
  const base: React.CSSProperties = {
    width: 11,
    height: 11,
    background: color,
    border: "2px solid var(--handle-border)",
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
  const validation = cook?.validation;

  const desc = descriptorFor(registry, node.typeId);
  const title = desc?.displayName ?? node.typeId;
  const inputs = desc?.inputs ?? [];
  const outputs = desc?.outputs ?? [];
  const isContainer = desc?.category === "container";
  const isLight = desc?.category === "lights";
  const isDisplay = data.isDisplay;

  const status = cook?.status;
  const badge =
    status?.state === "error"
      ? "var(--error-badge)"
      : status?.state === "cooking"
        ? "var(--cooking-color)"
        : null;

  const shapeClass = isContainer ? "square" : "pill";
  const fillClass = isLight ? " light-node" : "";

  return (
    <div
      className={`flow-node ${shapeClass}${fillClass}${selected ? " selected" : ""}${node.bypassed ? " bypassed" : ""}${stale ? " stale" : ""}`}
      title={desc?.doc}
    >
      {isDisplay && <span className="display-dot" title="display flag" />}
      {stale && <span className="stale-tag" title="stale (edit not yet cooked)" />}
      {status?.state === "error" && (
        <span className="err-tag" title={status.message}>
          !
        </span>
      )}
      {validation && (validation.errors > 0 || validation.warnings > 0) && (
        <span
          className={`val-tag${validation.errors > 0 ? " val-err" : ""}`}
          title={`validation: ${validation.errors} error(s), ${validation.warnings} warning(s)`}
        >
          {validation.errors > 0 ? validation.errors : validation.warnings}
        </span>
      )}

      {inputs.map((p: PortSnapshot, i: number) => (
        <Handle
          key={`in-${p.key}`}
          type="target"
          position={Position.Top}
          id={p.key}
          style={{
            ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType)),
            left: handleLeft(i, inputs.length),
          }}
          title={`${p.label} (${p.dataType}${p.variadic ? ", variadic" : ""})`}
          isConnectable
        />
      ))}

      <div className="flow-node-body">
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

      <div className="flow-node-label">
        <span className="flow-node-title">{title}</span>
        {status?.state === "ok" && status.ms > 0 && (
          <span className="flow-node-sub">{status.ms.toFixed(1)} ms</span>
        )}
      </div>

      {outputs.map((p: PortSnapshot, i: number) => (
        <Handle
          key={`out-${p.key}`}
          type="source"
          position={Position.Bottom}
          id={p.key}
          style={{
            ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType)),
            left: handleLeft(i, outputs.length),
          }}
          title={`${p.label} (${p.dataType})`}
          isConnectable
        />
      ))}
    </div>
  );
}
