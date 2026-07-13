// The one generic node component: every node type renders through it, its
// ports and colors derived from the registry snapshot (so a node added in
// Rust needs no new component). Minimystix silhouette (phase-6 design
// adoption): a compact pill (square for containers) flowing top-to-bottom
// with the label outside to the right. The UX-spec systems survive the
// restyle: typed handles (color by DataType family plus the round/diamond/
// square shape channel), display-flag dot, cook/stale/error/validation
// badges, and
// bypass hatching.

import { useEffect, useRef, useState } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";
import { InlineEdit } from "../components/InlineEdit";
import { Popover, renderDoc } from "../components/Popover";
import { descriptorFor, DATA_TYPE_COLOR, dataTypeShape } from "../registry/datatypes";
import { assetDisplayName, dispatch } from "../engine/session";
import type { NodeMirror, PortSnapshot } from "../engine/types";
import { useMirror } from "../store/mirror";
import { useRadial } from "../store/radial";
import { useUi } from "../store/ui";
import { IconEye } from "../icons";
import { nodeInfoLine } from "./infoLine";
import { nodeLabel } from "./nodeLabel";
import { hasVisibleParam, nodeVisible } from "./visibility";

/** Hover dwell before the radial opens (drag-safe dead time). */
const RADIAL_DELAY_MS = 400;

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
  const cookMode = useMirror((s) => s.cookMode);
  const inStale = useMirror((s) => s.stale.includes(node.id));
  const ctx = useMirror((s) => s.current);
  // Empty-geo indicator: a container whose subflow has no nodes or no
  // display flag produces nothing in the scene; render it hollow.
  const subflow = useMirror((s) => s.contexts[`sub:${node.id}`]);
  // Manual mode: the amber corner tag (spec sec. 8 header-count pattern).
  // Auto mode: a subtle pending tint while the budgeted cook catches up.
  const stale = cookMode === "manual" && inStale;
  const pending = cookMode === "auto" && inStale;
  const validation = cook?.validation;

  const desc = descriptorFor(registry, node.typeId);
  const title = nodeLabel(node, desc);

  // Inline rename (Phase 8): opened by double-clicking the label (the node
  // body keeps its container-dive double-click) or by F2 via the ui-store
  // rename request. Committing is one ordinary setParam on `name`.
  const [renaming, setRenaming] = useState(false);
  const renameRequest = useUi((s) => s.renameRequest);
  useEffect(() => {
    if (renameRequest === node.id) {
      setRenaming(true);
      useUi.getState().setRenameRequest(null);
    }
  }, [renameRequest, node.id]);
  const commitRename = (next: string) => {
    dispatch({
      type: "setParam",
      ctx,
      node: node.id,
      key: "name",
      value: { kind: "literal", type: "text", value: next },
    });
  };
  const inputs = desc?.inputs ?? [];
  const outputs = desc?.outputs ?? [];
  const isContainer = desc?.category === "container";
  const isDisplay = data.isDisplay;

  const status = cook?.status;
  // An async job parks the node as "pending" while the worker parses --
  // that IS the loading state (large imports), so it lights the same
  // spinner ring as "cooking".
  const loading = status?.state === "cooking" || status?.state === "pending";
  const badge =
    status?.state === "error"
      ? "var(--error-badge)"
      : loading
        ? "var(--cooking-color)"
        : null;

  const shapeClass = isContainer ? "square" : "pill";
  // Houdini-inspired pastel fill per registry category (a pure snapshot
  // interpreter: new categories fall back to the neutral pill fill).
  const fillClass = desc ? ` cat-${desc.category}` : "";
  const emptyGeo =
    isContainer && (!subflow || subflow.nodes.length === 0 || subflow.activeOutput === null);
  const infoLine = nodeInfoLine(desc, node, assetDisplayName);

  // Hover radial orchestration: a 400 ms dwell with no pointer buttons
  // down opens the ring; pressing anything or leaving cancels the timer
  // (the open ring closes itself on strays and pointerdowns).
  const rootRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<number | null>(null);
  const cancelRadialTimer = () => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  };
  useEffect(() => cancelRadialTimer, []);
  const armRadial = (e: React.PointerEvent) => {
    if (e.buttons !== 0) return;
    cancelRadialTimer();
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      // Identity only (Phase 10): the ring derives its screen anchor from the
      // live xyflow transform, and its mutable flags from the mirror, so it
      // tracks the node through pan, zoom, bypass and display changes.
      useRadial.getState().openRadial({
        nodeId: node.id,
        ctx,
        isContainer,
        bypassable: desc?.bypass.mode !== "notBypassable",
      });
    }, RADIAL_DELAY_MS);
  };

  return (
    <div
      ref={rootRef}
      className={`flow-node ${shapeClass}${fillClass}${selected ? " selected" : ""}${node.bypassed ? " bypassed" : ""}${stale ? " stale" : ""}${pending ? " pending" : ""}${loading ? " cooking" : ""}${emptyGeo ? " empty-geo" : ""}`}
      onPointerEnter={armRadial}
      onPointerDown={cancelRadialTimer}
      onPointerLeave={cancelRadialTimer}
    >
      {isDisplay && <span className="display-dot" title="display flag" />}
      {ctx === "root" && hasVisibleParam(desc) && (
        // Root visibility eye (Phase 8): registry-gated (note declares no
        // `visible`, so it gets no eye), distinct from the subflow display
        // flag. An ordinary setParam, so it undoes like any edit.
        <button
          className={`visibility-eye nodrag${nodeVisible(node) ? "" : " off"}`}
          title={nodeVisible(node) ? "Hide (stays cooked)" : "Show"}
          onClick={(e) => {
            e.stopPropagation();
            dispatch({
              type: "setParam",
              ctx,
              node: node.id,
              key: "visible",
              value: { kind: "literal", type: "bool", value: !nodeVisible(node) },
            });
          }}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <IconEye size={11} />
        </button>
      )}
      {stale && <span className="stale-tag" title="stale (edit not yet cooked)" />}
      {status?.state === "error" && (
        <Popover title="Cook error" content={<p>{status.message}</p>}>
          <span className="err-tag">!</span>
        </Popover>
      )}
      {validation && (validation.errors > 0 || validation.warnings > 0) && (
        <Popover
          title="Validation"
          content={
            <p>
              {validation.errors} error(s), {validation.warnings} warning(s). Select the node to
              read the report in the parameter panel.
            </p>
          }
        >
          <span className={`val-tag${validation.errors > 0 ? " val-err" : ""}`}>
            {validation.errors > 0 ? validation.errors : validation.warnings}
          </span>
        </Popover>
      )}

      {inputs.map((p: PortSnapshot, i: number) => (
        <Popover
          key={`in-${p.key}`}
          title={`${p.label} (${p.dataType}${p.variadic ? ", variadic" : ""})`}
          content={p.doc ? renderDoc(p.doc) : <p>Input port.</p>}
        >
          <Handle
            type="target"
            position={Position.Top}
            id={p.key}
            style={{
              ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType)),
              left: handleLeft(i, inputs.length),
            }}
            isConnectable
          />
        </Popover>
      ))}

      <div className="flow-node-body">
        {badge && <span className="cook-badge" style={{ background: badge }} />}

      </div>

      <div className="flow-node-label">
        {renaming ? (
          <InlineEdit
            value={title}
            placeholder={desc?.displayName ?? node.typeId}
            className="flow-node-rename"
            onCommit={commitRename}
            onClose={() => setRenaming(false)}
          />
        ) : (
          <Popover title={title} content={renderDoc(desc?.doc ?? "")}>
            <span
              className="flow-node-title"
              onDoubleClick={(e) => {
                e.stopPropagation();
                setRenaming(true);
              }}
            >
              {title}
            </span>
          </Popover>
        )}
        {infoLine && <span className="flow-node-info">{infoLine}</span>}
        {emptyGeo && <span className="flow-node-sub">empty</span>}
        {status?.state === "ok" && status.ms > 0 && (
          <span className="flow-node-sub">{status.ms.toFixed(1)} ms</span>
        )}
        {status?.state === "pending" && (
          <span className="flow-node-sub">loading geometry...</span>
        )}
      </div>

      {outputs.map((p: PortSnapshot, i: number) => (
        <Popover
          key={`out-${p.key}`}
          title={`${p.label} (${p.dataType})`}
          content={p.doc ? renderDoc(p.doc) : <p>Output port.</p>}
        >
          <Handle
            type="source"
            position={Position.Bottom}
            id={p.key}
            style={{
              ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType)),
              left: handleLeft(i, outputs.length),
            }}
            isConnectable
          />
        </Popover>
      ))}
    </div>
  );
}
