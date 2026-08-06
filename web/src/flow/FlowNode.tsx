// The one generic node component: every node type renders through it, its
// ports, fill, glyph, and silhouette derived from the registry snapshot (so
// a node added in Rust needs no new component). Evo anatomy on the one
// geometric contract: every role occupies the 112x32 layout box; the
// visible body may sit smaller inside it (light fixture, text datablock,
// terminal donut) and risers (folder tab, viewfinder bump, gather dome)
// ride above the box, so the glyph on its light chip, the cook arc, the
// display halo and the terminal core all share the box centre with no
// per-role offsets. The category pastel fills the body, silhouettes stay
// registry-derived, and the label stack rides to the right (type label,
// name, status row, param preview, authored description) with
// zoom-responsive LOD. Wings sit on the full-width bodies: bypass on the
// left; on the right, display in subflows and root visibility at root
// (both registry-gated), so the two affordances stay symmetric in every
// context. Rings mean selection only: stale is a tag plus body wash,
// cooking is the arc around the glyph. The UX-spec systems survive the
// restyle: typed handles (color by DataType family plus the shape
// channel), cook / stale / error / validation feedback, and bypass
// hatching.

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
import { IconBypass, IconDisplay } from "../icons";
import { nodeInfoLine } from "./infoLine";
import { nodeLabel } from "./nodeLabel";
import { glyphPath, NODE_BOX, nodeRole, ROLE_BODIES, type RoleBody } from "./nodeVisual";
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

/** Handles float clear of the layout box: 11px dot, ~3px air gap off the
 * box edge. One axis for every silhouette, so the wire endpoints are the
 * same whatever the body inside the box looks like. */
function handleStyle(color: string, shape: string, side: "in" | "out"): React.CSSProperties {
  const base: React.CSSProperties = {
    width: 11,
    height: 11,
    background: color,
    border: "2px solid var(--handle-border)",
  };
  if (side === "in") base.top = -14;
  else base.bottom = -14;
  if (shape === "diamond")
    return {
      ...base,
      // Compose with the library's centring translate: an inline transform
      // replaces the stylesheet's wholesale, so a bare rotate() would
      // un-centre the handle before rotating it.
      transform:
        side === "in" ? "translate(-50%, -50%) rotate(45deg)" : "translate(-50%, 50%) rotate(45deg)",
      borderRadius: 2,
    };
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
 * clip-path cannot give).
 *
 * Every shaped body is authored in `NODE_BOX` coordinates, so one viewBox
 * serves all of them; a riser at negative y paints through the SVG's
 * visible overflow rather than consuming the box. */
function ShapedBody({ body }: { body: RoleBody }) {
  return (
    <svg className="node-body-svg" viewBox={`0 0 ${NODE_BOX.w} ${NODE_BOX.h}`} aria-hidden>
      <path d={body.path} className="body-fill" />
      <path d={body.path} className="body-stroke" />
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
  // Zoom LOD: the label stack degrades with zoom. Bucketed to the
  // two thresholds so panning never re-renders nodes, only threshold
  // crossings do.
  const zoomBucket = useStore((s) => (s.transform[2] >= 0.9 ? 2 : s.transform[2] >= 0.7 ? 1 : 0));
 // Manual mode: the stale tag plus body wash (no ring; rings mean
  // selection). Auto mode: a subtle pending tint while the cook catches up.
  const stale = cookMode === "manual" && inStale;
  const pending = cookMode === "auto" && inStale;
  const validation = cook?.validation;

  const desc = descriptorFor(registry, node.typeId);
  const title = nodeLabel(node, desc);
  const role = nodeRole(desc);
  const glyph = glyphPath(desc);
  const shapedBody = ROLE_BODIES[role];

  // Inline rename: opened by double-clicking the label (the node
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

  // Wings live on the full-width bodies (the subflow rects plus the
  // root-graph silhouettes); the narrow subflow silhouettes (branch,
  // analyzer, imageSource, terminal, text) keep bypass and display
  // reachable through the radial and keyboard. Both wings are
  // registry-gated facts: bypass renders when the type is bypassable, and
  // the right wing carries the display flag in subflows and the additive
  // `visible` param at root, so the pair stays symmetric in every context
  // and a node without the fact simply has no wing for it.
  const hasWings =
    role === "standard" ||
    role === "gather" ||
    role === "container" ||
    role === "camera" ||
    role === "light";
  const bypassable = desc?.bypass.mode !== "notBypassable";
  const showBypassWing = hasWings && bypassable;
  const showDisplayWing = hasWings && ctx !== "root";
  const showVisibilityWing = hasWings && ctx === "root" && hasVisibleParam(desc);
  const visible = nodeVisible(node);

  // The single sub-row's text, by priority. Cook time is suppressed while
  // the clock runs: a figure that changes sixty times a second is unreadable
  // noise, and hiding it is the other half of keeping the stack still (the
  // engine no longer emits a status event per frame either -- see
  // `CookStatus::same_state`). Empty string keeps the reserved row blank.
  //
  // An empty container is deliberately NOT marked. A container you have not
  // filled in yet is the normal state of one you just made, and both the
  // dashed border and the word read as a fault rather than a stage.
  const playing = useMirror((s) => s.playing);
  const subLine =
    status?.state === "pending"
      ? "loading geometry..."
      : status?.state === "ok" && status.ms > 0 && !playing
        ? `${status.ms.toFixed(1)} ms`
        : "";

  const infoLine = nodeInfoLine(desc, node, assetDisplayName);
  const description = authoredDescription(node);
  // The grey type label disambiguates a renamed node (an un-renamed node's
  // title IS the type name); LOD-gated at zoom 0.7.
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
      // Identity only: the ring derives its screen anchor from the
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
      className={`flow-node role-${role}${desc ? ` cat-${desc.category} nt-${desc.typeId}` : ""}${selected ? " selected" : ""}${node.bypassed ? " bypassed" : ""}${stale ? " stale" : ""}${pending ? " pending" : ""}${loading ? " cooking" : ""}${isDisplay ? " is-display" : ""}`}
      onPointerEnter={armRadial}
      onPointerDown={cancelRadialTimer}
      onPointerLeave={cancelRadialTimer}
    >
      {/* The display halo: the one cue readable from across the
          graph; the wing (or radial) is the click target. */}
      {isDisplay && <span className="display-halo" aria-hidden />}

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
              ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType), "in"),
              left: handleLeft(i, inputs.length),
            }}
            isConnectable
          />
        </Popover>
      ))}

      <div className="node-body">
        {shapedBody ? <ShapedBody body={shapedBody} /> : null}
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
          >
            <IconBypass size={9} />
          </button>
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
          >
            <IconDisplay size={9} />
          </button>
        )}
        {showVisibilityWing && (
          // The root visibility wing: the right-wing slot carries the
          // additive `visible` param at root (note declares no `visible`,
          // so it gets no wing). The dot is always shown -- filled
          // display-blue when visible, hollow when hidden -- so the wing
          // reads at rest the way the retired floating eye did. An
          // ordinary setParam, so it undoes like any edit.
          <button
            className={`node-wing wing-visibility nodrag${visible ? "" : " off"}`}
            title={visible ? "Hide (stays cooked)" : "Show"}
            onClick={(e) => {
              e.stopPropagation();
              dispatch({
                type: "setParam",
                ctx,
                node: node.id,
                key: "visible",
                value: { kind: "literal", type: "bool", value: !visible },
              });
            }}
            onDoubleClick={(e) => e.stopPropagation()}
          >
            <span className="vis-dot" aria-hidden />
          </button>
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
        {/* ONE sub row, always present. The label stack is vertically
            centered, so a row that comes and goes moves every other row by
            half a line -- which is exactly what made nodes jump while the
            clock ran. Reserving the row costs one line of height and buys a
            stack that never shifts. */}
        <span className="flow-node-sub">{subLine}</span>
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
              ...handleStyle(DATA_TYPE_COLOR[p.dataType], dataTypeShape(p.dataType), "out"),
              left: handleLeft(i, outputs.length),
            }}
            isConnectable
          />
        </Popover>
      ))}
    </div>
  );
}
