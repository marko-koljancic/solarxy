// The parameter panel: a pure interpreter of the registry snapshot. For the
// selected node it renders one editor per param (grouped), choosing the
// widget from the descriptor's paramType. Edits go through the preview lane
// (no undo spam) during a drag and commit one authoritative SetParam on
// release, which is what makes the param-drag-to-viewport loop cheap.
//
// A new node reusing existing param types needs zero changes here; a new
// ParamType is a deliberate change (a new widget case).

import { useMemo, useRef, useState, type ReactNode } from "react";
import { dispatch, flyToIssue, previewParam, stageFile } from "../engine/session";
import type {
  GraphContext,
  NodeMirror,
  ParamSnapshot,
  ParamSource,
  ValidationIssue,
} from "../engine/types";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror, type ValidationReportData } from "../store/mirror";
import { ColorInput } from "./inputs/ColorInput";
import { Popover, renderDoc } from "./Popover";
import { FloatInput } from "./inputs/FloatInput";
import { VectorInput } from "./inputs/VectorInput";

/** The current value for a param: the node's override, else the default. */
function paramValue(node: NodeMirror, spec: ParamSnapshot): unknown {
  const src = node.params[spec.key];
  if (src && src.kind === "literal") return (src as { value: unknown }).value;
  return spec.default;
}

/** Builds a literal ParamSource of the given descriptor param type. */
function literal(paramType: string, value: unknown): ParamSource {
  const tag = paramType === "assetRef" ? "asset" : paramType;
  return { kind: "literal", type: tag, value } as ParamSource;
}

interface FieldProps {
  ctx: GraphContext;
  node: NodeMirror;
  spec: ParamSnapshot;
}

function Field({ ctx, node, spec }: FieldProps) {
  const value = paramValue(node, spec);
  const commit = (v: unknown) => dispatch({ type: "setParam", ctx, node: node.id, key: spec.key, value: literal(spec.paramType, v) });
  const preview = (v: unknown) => previewParam(ctx, node.id, spec.key, literal(spec.paramType, v));
  const unitSuffix = spec.unit === "degrees" ? "°" : spec.unit === "meters" ? " m" : "";

  // Spec section 7: every param label carries a hover doc popover from its
  // descriptor doc string.
  const label = (
    <Popover
      title={spec.label}
      content={
        <>
          {spec.doc ? renderDoc(spec.doc) : null}
          {spec.soft && (
            <p className="doc-popover-meta">
              Range {spec.soft[0]}-{spec.soft[1]}
              {unitSuffix ? ` ${unitSuffix.trim()}` : ""}
            </p>
          )}
        </>
      }
    >
      <label className="param-label">
        {spec.label}
        {unitSuffix && <span className="param-unit">{unitSuffix}</span>}
      </label>
    </Popover>
  );

  switch (spec.paramType) {
    case "float":
    case "int": {
      const isInt = spec.paramType === "int";
      return (
        <div className="param-row">
          {label}
          <FloatInput
            value={Number(value)}
            int={isInt}
            soft={spec.soft}
            min={spec.hard?.[0]}
            max={spec.hard?.[1]}
            step={spec.step ?? undefined}
            onPreview={preview}
            onCommit={commit}
          />
        </div>
      );
    }
    case "bool":
      return (
        <div className="param-row">
          {label}
          <input
            type="checkbox"
            className="checkbox-input"
            checked={Boolean(value)}
            onChange={(e) => commit(e.target.checked)}
          />
        </div>
      );
    case "enum":
      return (
        <div className="param-row">
          {label}
          <select
            className="input-field select-input"
            value={String(value)}
            onChange={(e) => commit(e.target.value)}
          >
            {spec.enumVariants.map(([key, lbl]) => (
              <option key={key} value={key}>
                {lbl}
              </option>
            ))}
          </select>
        </div>
      );
    case "text":
      return (
        <div className="param-row">
          {label}
          <input
            type="text"
            className="input-field text-input"
            value={String(value ?? "")}
            onChange={(e) => commit(e.target.value)}
          />
        </div>
      );
    case "color":
      return (
        <div className="param-row">
          {label}
          <ColorInput value={(Array.isArray(value) ? value : [0, 0, 0, 1]) as number[]} onCommit={commit} />
        </div>
      );
    case "assetRef":
      return <AssetField ctx={ctx} node={node} spec={spec} label={label} />;
    case "vec2":
    case "vec3":
    case "vec4": {
      const n = spec.paramType === "vec2" ? 2 : spec.paramType === "vec3" ? 3 : 4;
      const arr = (Array.isArray(value) ? value : Array(n).fill(0)) as number[];
      return (
        <div className="param-row">
          {label}
          <VectorInput
            value={arr}
            size={n as 2 | 3 | 4}
            step={spec.step ?? undefined}
            onPreview={preview}
            onCommit={commit}
          />
        </div>
      );
    }
    default:
      return (
        <div className="param-row">
          {label}
          <span className="param-unsupported">{spec.paramType}</span>
        </div>
      );
  }
}

/** The `assetRef` widget: a hidden file input plus a select/change/clear
 * control. Selecting a file stages its bytes (content-addressed) and commits
 * the asset hash as the param, which dirties the import node so the next cook
 * yields a parse job to the worker. */
function AssetField({ ctx, node, spec, label }: FieldProps & { label: ReactNode }) {
  const [pending, setPending] = useState(false);
  const [localName, setLocalName] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const current = String(paramValue(node, spec) ?? "");

  // Multi-file staging: a multi-file model (gltf + bin + textures) selects
  // everything at once; every file stages (the sidecar resolver matches
  // companions by name at parse time) and the param points at the primary
  // (the first file matching the accepted extensions).
  const onFiles = async (files: File[]) => {
    setPending(true);
    try {
      const exts = spec.accept.map((a) => a.toLowerCase());
      const primary =
        files.find((f) => exts.some((ext) => f.name.toLowerCase().endsWith(ext))) ?? files[0];
      let primaryHash = "";
      for (const file of files) {
        const { hash, name } = await stageFile(file);
        if (file === primary) {
          primaryHash = hash;
          setLocalName(name);
        }
      }
      if (primaryHash) {
        dispatch({ type: "setParam", ctx, node: node.id, key: spec.key, value: literal("assetRef", primaryHash) });
      }
    } finally {
      setPending(false);
    }
  };

  const clear = () => {
    setLocalName("");
    dispatch({ type: "setParam", ctx, node: node.id, key: spec.key, value: literal("assetRef", "") });
  };

  const display = localName || (current ? `${current.slice(0, 10)}…` : "no file");

  return (
    <div className="param-row">
      {label}
      <div className="param-asset">
        <button
          type="button"
          className="param-asset-btn"
          disabled={pending}
          onClick={() => inputRef.current?.click()}
        >
          {pending ? "Staging…" : current ? "Change" : "Select File"}
        </button>
        <span className="param-asset-name" title={current}>
          {display}
        </span>
        {current && !pending && (
          <button type="button" className="param-asset-clear" title="Clear" onClick={clear}>
            ×
          </button>
        )}
        <input
          ref={inputRef}
          type="file"
          multiple
          style={{ display: "none" }}
          onChange={(e) => {
            const files = Array.from(e.target.files ?? []);
            if (files.length > 0) void onFiles(files);
            e.target.value = "";
          }}
        />
      </div>
    </div>
  );
}

export function ParameterPanel() {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const cook = useMirror((s) => s.cook);
  const reports = useMirror((s) => s.reports);

  const selectedId = graph.selection[0];
  const node = useMemo(
    () => graph.nodes.find((n) => n.id === selectedId),
    [graph, selectedId],
  );
  // The active tab; falls back to the first tab whenever the selection's
  // group set no longer contains it (switching node types).
  const [tab, setTab] = useState<string>("");

  if (!node) {
    return (
      <div className="param-panel empty">
        <span>Select a node to edit its parameters.</span>
      </div>
    );
  }

  const desc = descriptorFor(registry, node.typeId);
  const stats = cook[node.id];
  // Group params by their `group` (presentation only), preserving order.
  const groups = new Map<string, ParamSnapshot[]>();
  for (const p of desc?.params ?? []) {
    const g = groups.get(p.group) ?? [];
    g.push(p);
    groups.set(p.group, g);
  }
  // Tabs (Minimystix underline pattern, Phase 7b D1): general first, the
  // rest in declaration order, plus a Validation tab when a report exists.
  const groupNames = [...groups.keys()];
  const orderedGroups = [
    ...groupNames.filter((g) => g.toLowerCase() === "general"),
    ...groupNames.filter((g) => g.toLowerCase() !== "general"),
  ];
  const report = reports[node.id];
  const tabs = report ? [...orderedGroups, VALIDATION_TAB] : orderedGroups;
  const active = tabs.includes(tab) ? tab : tabs[0];

  return (
    <div className="param-panel">
      <div className="param-header">
        <span className="param-title">{desc?.displayName ?? node.typeId}</span>
        {stats?.points !== undefined && (
          <span className="param-stats">
            {stats.points} pts · {stats.prims} tris · {stats.meshes} mesh
          </span>
        )}
      </div>
      {tabs.length > 1 && (
        <div className="param-tabs" role="tablist">
          {tabs.map((t) => (
            <button
              key={t}
              role="tab"
              aria-selected={t === active}
              className={`param-tab${t === active ? " active" : ""}${t === VALIDATION_TAB && report && report.errors > 0 ? " has-errors" : ""}`}
              onClick={() => setTab(t)}
            >
              {tabLabel(t)}
              {t === VALIDATION_TAB && report && report.issues.length > 0 && (
                <span className="param-tab-count">{report.errors + report.warnings}</span>
              )}
            </button>
          ))}
        </div>
      )}
      <div className="param-body">
        {active !== undefined && active !== VALIDATION_TAB && (
          <div className="param-tab-body" role="tabpanel">
            {(groups.get(active) ?? []).map((p) => (
              <Field key={p.key} ctx={current} node={node} spec={p} />
            ))}
          </div>
        )}
        {active === VALIDATION_TAB && report && (
          <ValidationSection ctx={current} sourceNode={node.id} report={report} />
        )}
        {tabs.length === 0 && <div className="param-empty">No parameters.</div>}
      </div>
    </div>
  );
}

/** Sentinel tab name for the validation report (registry groups are
 * lowercase, so the capitalized sentinel cannot collide). */
const VALIDATION_TAB = "Validation";

/** "general" -> "General". */
function tabLabel(group: string): string {
  if (group === VALIDATION_TAB) return group;
  return group.charAt(0).toUpperCase() + group.slice(1);
}

/** "degenerateTriangles" -> "Degenerate Triangles". */
function prettyKind(kind: string): string {
  const spaced = kind.replace(/([a-z])([A-Z])/g, "$1 $2");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

/** The selected node's validation report (validate node or import load
 * validation): counts header plus one row per issue. A row click flies the
 * active pane's camera to the issue's mesh and enables that pane's
 * validation overlay (subflow contexts only: the owning geo container is
 * the scene object). */
function ValidationSection({
  ctx,
  sourceNode,
  report,
}: {
  ctx: GraphContext;
  sourceNode: number;
  report: ValidationReportData;
}) {
  const objectNode = ctx === "root" ? null : ctx.subflow;
  const clean = report.issues.length === 0;
  return (
    <div className="validation-section" role="tabpanel">
      <div className="validation-summary">
        {clean ? (
          <span className="validation-clean">No issues found.</span>
        ) : (
          <>
            {report.errors > 0 && <span className="validation-count err">{report.errors} error{report.errors === 1 ? "" : "s"}</span>}
            {report.warnings > 0 && <span className="validation-count warn">{report.warnings} warning{report.warnings === 1 ? "" : "s"}</span>}
          </>
        )}
      </div>
      {!clean && (
        <ul className="validation-list">
          {report.issues.map((issue: ValidationIssue, i: number) => (
            <li key={i}>
              <button
                type="button"
                className="validation-row"
                disabled={objectNode === null}
                title={objectNode === null ? issue.message : `${issue.message} (click to frame)`}
                onClick={() => {
                  if (objectNode !== null) flyToIssue(objectNode, sourceNode, i);
                }}
              >
                <span className={`validation-dot ${issue.severity}`} />
                <span className="validation-kind">{prettyKind(issue.kind)}</span>
                <span className="validation-msg">{issue.message}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {report.truncated && (
        <div className="validation-truncated">
          List truncated to {report.issues.length} rows; counts above are complete.
        </div>
      )}
    </div>
  );
}
