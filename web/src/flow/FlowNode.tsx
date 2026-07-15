// The one generic node component: every node type renders through it, its
// ports, fill, glyph, and silhouette derived from the registry snapshot (so
// a node added in Rust needs no new component). Evo anatomy (revamp R2,
// decisions D-4, D-10..D-12, D-15, D-16, D-18): a 4:1 instrument body with
// the category pastel fill, a per-type glyph on a light chip, role
// silhouettes (rect, container square, gather dome, branch hexagon,
// terminal donut, analyzer trapezoid, imageSource file, light lampshade),
// bypass and display wings on rect bodies, the display halo behind the
// display-flag holder, and a label stack to the right (type label, name,
// status row, param preview, authored description) with zoom-responsive
// LOD. Rings mean selection only: stale is a tag plus body wash, cooking
// is the arc around the chip. The UX-spec systems survive the restyle:
// typed handles (color by DataType family plus the shape channel), cook /
// stale / error / validation feedback, and bypass hatching.

import { useEffect, useRef, useState } from "react";
import { Handle, Position, useStore, type NodeProps } from "@xyflow/react";
import { InlineEdit } from "../components/InlineEdit";
import { Popover, renderDoc } from "../components/Popover";
import { descriptorFor, DATA_TYPE_COLOR, dataTypeShape } from "../registry/datatypes";
import { assetDisplayName, dispatch } from "../engine/session";
import type { NodeMirror, PortSnapshot } from "../engine/types";
import { useMirror } from "../store/mirror";
import { useRadial } from "../store/radial";
import { useUi } from "../store/ui";
import { IconBypass, IconEye } from "../icons";
import { nodeInfoLine } from "./infoLine";
import { nodeLabel } from "./nodeLabel";
import {
  glyphPath,
  nodeRole,
  IMAGE_SOURCE_FOLD_PATH,
  ROLE_BODY_PATHS,
  SLOT_XS,
} from "./nodeVisual";
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

/** Handles float clear of the body (D-4): 11px dot, ~3px air gap; gather
 * inputs sit above the dome. */
function handleStyle(
  color: string,
  shape: string,
  side: "in" | "out",
  gather: boolean,
): React.CSSProperties {
  const base: React.CSSProperties = {
    width: 11,
    height: 11,
    background: color,
    border: "2px solid var(--handle-border)",
  };
  if (side === "in") base.top = gather ? -22 : -14;
  else base.bottom = -14;
  if (shape === "diamond") return { ...base, transform: "rotate(45deg)", borderRadius: 2 };
  if (shape === "square") return { ...base, borderRadius: 2 };
  if (shape === "hexagon")
    return {
      ...base,
      // A clipped hexagon reads as a "resource" (image) handle; the clip
      // also removes the border, so the fill carries the full 11px.
      clipPath: "polygon(50% 0%, 100% 25%, 100% 75%, 50% 100%, 0% 75%, 0% 25%)",
      border: "none",
    };
  return { ...base, borderRadius: "50%" };
}

/** The node's authored description (the `description` text param), or null.
 * Registry-gated: only nodes declaring the param can carry one. */
function authoredDescription(node: NodeMirror): string | null {
  const src = node.params["description"];
  if (src && src.kind === "literal" && src.type === "text") {
    const text = src.value.trim();
    if (text !== "") return text;
  }
  return null;
}

/** Shaped silhouette body as inline SVG (crisp 1px stroke, which CSS
 * clip-path cannot give), with the D-12 instrument seams clipped inside. */
function ShapedBody({ role, path }: { role: string; path: string }) {
  const clipId = useRef(`body-${Math.random().toString(36).slice(2, 9)}`);
  return (
    <svg className="node-body-svg" viewBox="0 0 112 28" aria-hidden>
      <defs>
        <clipPath id={clipId.current}>
          <path d={path} />
        </clipPath>
      </defs>
      <path d={path} className="body-fill" />
      <g clipPath={`url(#${clipId.current})`}>
        {SLOT_XS.map((x) => (
          <line key={x} x1={x} y1={0} x2={x} y2={28} className="body-slot" />
        ))}
      </g>
      {role === "imageSource" && <path d={IMAGE_SOURCE_FOLD_PATH} className="body-fold" />}
      <path d={path} className="body-stroke" />
    </svg>
  );
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
  // Zoom LOD (D-16): the label stack degrades with zoom. Bucketed to the
  // two D-11 thresholds so panning never re-renders nodes, only threshold
  // crossings do.
  const zoomBucket = useStore((s) => (s.transform[2] >= 0.9 ? 2 : s.transform[2] >= 0.7 ? 1 : 0));
  // Manual mode: the stale tag plus body wash (D-15: no ring; rings mean
  // selection). Auto mode: a subtle pending tint while the cook catches up.
  const stale = cookMode === "manual" && inStale;
  const pending = cookMode === "auto" && inStale;
  const validation = cook?.validation;

  const desc = descriptorFor(registry, node.typeId);
  const title = nodeLabel(node, desc);
  const role = nodeRole(desc);
  const glyph = glyphPath(desc);
  const shapedPath = ROLE_BODY_PATHS[role];

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
  const isContainer = role === "container";
  const isDisplay = data.isDisplay;

  const status = cook?.status;
  // An async job parks the node as "pending" while the worker parses --
  // that IS the loading state (large imports), so it lights the same
  // cook arc as "cooking".
  const loading = status?.state === "cooking" || status?.state === "pending";

  // Wings (D-4) live on rect bodies only (standard, gather); shaped
  // silhouettes keep bypass and display reachable through the radial and
  // keyboard. The display wing is a subflow affordance (the root uses the
  // additive visibility eye instead).
  const hasWings = role === "standard" || role === "gather";
  const bypassable = desc?.bypass.mode !== "notBypassable";
  const showBypassWing = hasWings && bypassable;
  const showDisplayWing = hasWings && ctx !== "root";

  const emptyGeo =
    isContainer && (!subflow || subflow.nodes.length === 0 || subflow.activeOutput === null);
  const infoLine = nodeInfoLine(desc, node, assetDisplayName);
  const description = authoredDescription(node);
  // The grey type label disambiguates a renamed node (an un-renamed node's
  // title IS the type name); LOD-gated at zoom 0.7 (D-11).
  const showTypeLabel = zoomBucket >= 1 && desc !== undefined && title !== desc.displayName;
  const showDescription = zoomBucket >= 2 && description !== null;

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
        bypassable,
      });
    }, RADIAL_DELAY_MS);
  };

  const statusRow =
    status?.state === "error" ||
    (validation && (validation.errors > 0 || validation.warnings > 0)) ||
    node.bypassed ||
    stale;

  return (
    <div
      ref={rootRef}
      className={`flow-node role-${role}${desc ? ` cat-${desc.category}` : ""}${selected ? " selected" : ""}${node.bypassed ? " bypassed" : ""}${stale ? " stale" : ""}${pending ? " pending" : ""}${loading ? " cooking" : ""}${emptyGeo ? " empty-geo" : ""}${isDisplay ? " is-display" : ""}`}
      onPointerEnter={armRadial}
      onPointerDown={cancelRadialTimer}
      onPointerLeave={cancelRadialTimer}
    >
      {/* The display halo (D-3): the one cue readable from across the
          graph; the wing (or radial) is the click target. */}
      {isDisplay && <span className="display-halo" aria-hidden />}

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
              ...handleStyle(
                DATA_TYPE_COLOR[p.dataType],
                dataTypeShape(p.dataType),
                "in",
                role === "gather",
              ),
              left: handleLeft(i, inputs.length),
            }}
            isConnectable
          />
        </Popover>
      ))}

      <div className="node-body">
        {shapedPath ? <ShapedBody role={role} path={shapedPath} /> : null}
        {role === "gather" && <span className="gather-dome" aria-hidden />}

        {showBypassWing && (
          <button
            className={`node-wing wing-bypass nodrag${node.bypassed ? " on" : ""}`}
            title={node.bypassed ? "Bypassed (click to re-enable)" : "Bypass"}
            onClick={(e) => {
              e.stopPropagation();
              dispatch({ type: "setBypass", ctx, node: node.id, bypassed: !node.bypassed });
            }}
            onDoubleClick={(e) => e.stopPropagation()}
          />
        )}
        {showDisplayWing && (
          <button
            className={`node-wing wing-display nodrag${isDisplay ? " on" : ""}`}
            title={isDisplay ? "Display node" : "Set the display flag"}
            onClick={(e) => {
              e.stopPropagation();
              if (!isDisplay) dispatch({ type: "setActiveOutput", ctx, node: node.id });
            }}
            onDoubleClick={(e) => e.stopPropagation()}
          />
        )}

        {role !== "terminal" && (
          <span className="node-chip" aria-hidden>
            <svg viewBox="0 0 16 16" className="node-glyph">
              <path d={glyph} />
            </svg>
          </span>
        )}
        {role === "terminal" && <span className="terminal-core" aria-hidden />}
        {loading && <span className="cook-arc" aria-hidden />}
      </div>

      <div className="flow-node-label">
        {showTypeLabel && <span className="node-type-label">{desc.displayName}</span>}
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
        {statusRow && (
          <span className="node-status-row">
            {status?.state === "error" && (
              <Popover title="Cook error" content={<p>{status.message}</p>}>
                <span className="st-err">!</span>
              </Popover>
            )}
            {validation && (validation.errors > 0 || validation.warnings > 0) && (
              <Popover
                title="Validation"
                content={
                  <p>
                    {validation.errors} error(s), {validation.warnings} warning(s). Select the
                    node to read the report in the parameter panel.
                  </p>
                }
              >
                <span className={`st-val${validation.errors > 0 ? " val-err" : ""}`}>
                  {validation.errors > 0 ? validation.errors : validation.warnings}
                </span>
              </Popover>
            )}
            {node.bypassed && (
              <span className="st-bypass" title="Bypassed">
                <IconBypass size={10} />
              </span>
            )}
            {stale && (
              <span className="st-stale" title="Stale (edit not yet cooked)">
                stale
              </span>
            )}
          </span>
        )}
        {infoLine && <span className="flow-node-info">{infoLine}</span>}
        {showDescription && <span className="node-desc">{description}</span>}
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
              ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType), "out", false),
              left: handleLeft(i, outputs.length),
            }}
            isConnectable
          />
        </Popover>
      ))}
    </div>
  );
}
