// The node info modal (C8, opened from the radial menu): a
// modeless, draggable card aggregating everything the mirror knows about
// one node: identity + doc, cook status/time/error, geometry stats,
// validation counts with a jump to the full report, and the bypass/stale/
// pending flags. Closes on outside mousedown or Esc; dragging is a plain
// header pointer-capture (no library).

import { useEffect, useRef, useState } from "react";
import { dispatch } from "../engine/session";
import { ctxKey, type PortSnapshot } from "../engine/types";
import { descriptorFor } from "../registry/datatypes";
import { useMirror } from "../store/mirror";
import { useRadial } from "../store/radial";
import { renderDoc } from "./Popover";

export function NodeInfoModal() {
  const info = useRadial((s) => s.infoNode);
  const closeInfo = useRadial((s) => s.closeInfo);
  const registry = useMirror((s) => s.registry);
  const node = useMirror((s) =>
    info ? s.contexts[ctxKey(info.ctx)]?.nodes.find((n) => n.id === info.nodeId) : undefined,
  );
  const cook = useMirror((s) => (info ? s.cook[info.nodeId] : undefined));
  const report = useMirror((s) => (info ? s.reports[info.nodeId] : undefined));
  const stale = useMirror((s) => (info ? s.stale.includes(info.nodeId) : false));

  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ dx: number; dy: number } | null>(null);

  // Reset position when the modal retargets to another node.
  useEffect(() => {
    if (!info) return;
    setPos({
      x: Math.min(Math.max(info.x, 8), window.innerWidth - 340),
      y: Math.min(Math.max(info.y, 8), window.innerHeight - 260),
    });
  }, [info]);

  useEffect(() => {
    if (!info) return;
    const onDown = (e: MouseEvent) => {
      if (!(e.target instanceof Element)) return;
      if (cardRef.current?.contains(e.target)) return;
      // Clicking the radial that spawned another info view is not "outside".
      if (e.target.closest(".radial-anchor")) return;
      closeInfo();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeInfo();
    };
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [info, closeInfo]);

  if (!info || !pos) return null;
  if (!node) {
    // The node vanished (deleted, context reload) while the modal was up.
    return null;
  }
  const desc = descriptorFor(registry, node.typeId);
  const title = desc?.displayName ?? node.typeId;
  const status = cook?.status;

  const ports = [
    ...(desc?.inputs ?? []).map((port) => ({ dir: "in", port })),
    ...(desc?.outputs ?? []).map((port) => ({ dir: "out", port })),
  ];
  // `general` is the implicit name/description group every node carries; it
  // is not what someone opens this modal to read about.
  const params = (desc?.params ?? []).filter((p) => p.group !== "general");

  const startDrag = (e: React.PointerEvent) => {
    dragRef.current = { dx: e.clientX - pos.x, dy: e.clientY - pos.y };
    (e.target as Element).setPointerCapture(e.pointerId);
  };
  const onDrag = (e: React.PointerEvent) => {
    if (!dragRef.current) return;
    setPos({ x: e.clientX - dragRef.current.dx, y: e.clientY - dragRef.current.dy });
  };
  const endDrag = () => {
    dragRef.current = null;
  };

  const showReport = () => {
    // Selecting the node surfaces its validation report in the parameter
    // panel; retarget the canvas context first if the modal outlived a
    // context switch.
    const mirror = useMirror.getState();
    if (ctxKey(mirror.current) !== ctxKey(info.ctx)) mirror.setCurrent(info.ctx);
    dispatch({ type: "setSelection", ctx: info.ctx, ids: [info.nodeId] });
  };

  const statusText =
    status?.state === "ok"
      ? `cooked in ${status.ms.toFixed(1)} ms`
      : status?.state === "error"
        ? `error: ${status.message}`
        : status?.state === "cooking"
          ? "cooking..."
          : status?.state === "pending"
            ? "loading geometry..."
            : "not cooked yet";

  return (
    <div ref={cardRef} className="node-info-modal" style={{ left: pos.x, top: pos.y }}>
      <div
        className="node-info-header"
        onPointerDown={startDrag}
        onPointerMove={onDrag}
        onPointerUp={endDrag}
      >
        <span className="node-info-title">{title}</span>
        <span className="node-info-type">
          {node.typeId} v{node.typeVersion}
        </span>
        <span className="spacer" />
        <button className="btn" onClick={closeInfo} title="Close">
          x
        </button>
      </div>
      <div className="node-info-body">
        {desc && (
          <div className="node-info-row">
            <span className="node-info-key">Kind</span>
            <span>
              {desc.categoryLabel}
              {desc.contexts.length > 0 && ` · in ${desc.contexts.join(", ")}`}
              {desc.opens && ` · opens a ${desc.opens} network`}
            </span>
          </div>
        )}
        {desc?.doc && <div className="node-info-doc">{renderDoc(desc.doc)}</div>}
        <div className="node-info-row">
          <span className="node-info-key">Status</span>
          <span className={status?.state === "error" ? "node-info-error" : undefined}>
            {statusText}
            {node.bypassed ? " (bypassed)" : ""}
            {stale ? " (stale)" : ""}
          </span>
        </div>
        {(cook?.points !== undefined || cook?.prims !== undefined) && (
          <div className="node-info-row">
            <span className="node-info-key">Geometry</span>
            <span>
              {cook.points ?? 0} points, {cook.prims ?? 0} prims, {cook.meshes ?? 0} mesh(es)
            </span>
          </div>
        )}
        {cook?.validation && (cook.validation.errors > 0 || cook.validation.warnings > 0) && (
          <div className="node-info-row">
            <span className="node-info-key">Validation</span>
            <span>
              {cook.validation.errors} error(s), {cook.validation.warnings} warning(s)
              {report && report.issues.length > 0 && (
                <>
                  {" "}
                  <button className="crumb-link" onClick={showReport}>
                    show report
                  </button>
                </>
              )}
            </span>
          </div>
        )}

        {/* Ports and params come straight off the registry snapshot, so a
         * node added in Rust documents itself here with no frontend change.
         * The modal used to show only the node's own doc, which meant the
         * per-port and per-param docs the registry carries had exactly one
         * surface: a hover popover you had to already know about. */}
        {desc && ports.length > 0 && (
          <PortSection ports={ports} />
        )}
        {desc && params.length > 0 && (
          <details className="node-info-section">
            <summary>Parameters ({params.length})</summary>
            <dl className="node-info-defs">
              {params.map((p) => (
                <div key={p.key} className="node-info-def">
                  <dt>
                    {p.label}
                    <span className="node-info-meta">
                      {p.paramType}
                      {p.unit !== "none" && ` · ${p.unit}`}
                      {p.hard && ` · ${p.hard[0]} to ${p.hard[1]}`}
                    </span>
                  </dt>
                  <dd>{p.doc ? renderDoc(p.doc) : <em>No description.</em>}</dd>
                </div>
              ))}
            </dl>
          </details>
        )}
      </div>
    </div>
  );
}

/** Inputs then outputs, each with its wire type and what it expects. */
function PortSection({ ports }: { ports: { dir: string; port: PortSnapshot }[] }) {
  return (
    <details className="node-info-section" open>
      <summary>Ports ({ports.length})</summary>
      <dl className="node-info-defs">
        {ports.map(({ dir, port }) => (
          <div key={`${dir}:${port.key}`} className="node-info-def">
            <dt>
              {port.label}
              <span className="node-info-meta">
                {dir} · {port.dataType}
                {port.variadic && " · variadic"}
                {port.required && " · required"}
              </span>
            </dt>
            <dd>{port.doc ? renderDoc(port.doc) : <em>No description.</em>}</dd>
          </div>
        ))}
      </dl>
    </details>
  );
}
