// The parameter panel: a pure interpreter of the registry snapshot. For the
// selected node it renders one editor per param (grouped), choosing the
// widget from the descriptor's paramType. Edits go through the preview lane
// (no undo spam) during a drag and commit one authoritative SetParam on
// release, which is what makes the param-drag-to-viewport loop cheap.
//
// A new node reusing existing param types needs zero changes here; a new
// ParamType is a deliberate change (a new widget case).

import { useMemo, useRef, useState, type ReactNode } from "react";
import { dispatch, previewParam, stageFile } from "../engine/session";
import type { GraphContext, NodeMirror, ParamSnapshot, ParamSource } from "../engine/types";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";

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

  const label = (
    <label className="param-label" title={spec.doc}>
      {spec.label}
      {unitSuffix && <span className="param-unit">{unitSuffix}</span>}
    </label>
  );

  switch (spec.paramType) {
    case "float":
    case "int": {
      const num = Number(value);
      const step = spec.step ?? (spec.paramType === "int" ? 1 : 0.01);
      const hasSlider = spec.soft !== null;
      return (
        <div className="param-row">
          {label}
          <div className="param-input-group">
            {hasSlider && spec.soft && (
              <input
                type="range"
                min={spec.soft[0]}
                max={spec.soft[1]}
                step={step}
                value={num}
                onChange={(e) => preview(Number(e.target.value))}
                onPointerUp={(e) => commit(Number((e.target as HTMLInputElement).value))}
              />
            )}
            <input
              type="number"
              step={step}
              value={num}
              onChange={(e) => commit(spec.paramType === "int" ? Math.round(Number(e.target.value)) : Number(e.target.value))}
            />
          </div>
        </div>
      );
    }
    case "bool":
      return (
        <div className="param-row">
          {label}
          <input type="checkbox" checked={Boolean(value)} onChange={(e) => commit(e.target.checked)} />
        </div>
      );
    case "enum":
      return (
        <div className="param-row">
          {label}
          <select value={String(value)} onChange={(e) => commit(e.target.value)}>
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
          <input type="text" value={String(value ?? "")} onChange={(e) => commit(e.target.value)} />
        </div>
      );
    case "color": {
      const c = (Array.isArray(value) ? value : [0, 0, 0, 1]) as number[];
      const hex = `#${[0, 1, 2].map((i) => Math.round(Math.min(1, Math.max(0, c[i] ?? 0)) * 255).toString(16).padStart(2, "0")).join("")}`;
      return (
        <div className="param-row">
          {label}
          <input
            type="color"
            value={hex}
            onChange={(e) => {
              const v = e.target.value;
              const r = parseInt(v.slice(1, 3), 16) / 255;
              const g = parseInt(v.slice(3, 5), 16) / 255;
              const b = parseInt(v.slice(5, 7), 16) / 255;
              commit([r, g, b, c[3] ?? 1]);
            }}
          />
        </div>
      );
    }
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
          <div className="param-vec">
            {Array.from({ length: n }, (_, i) => (
              <input
                key={i}
                type="number"
                step={spec.step ?? 0.01}
                value={arr[i] ?? 0}
                onChange={(e) => {
                  const next = [...arr];
                  next[i] = Number(e.target.value);
                  commit(next);
                }}
              />
            ))}
          </div>
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
  const accept = spec.accept.join(",");

  const onFile = async (file: File) => {
    setPending(true);
    try {
      const { hash, name } = await stageFile(file);
      setLocalName(name);
      dispatch({ type: "setParam", ctx, node: node.id, key: spec.key, value: literal("assetRef", hash) });
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
          accept={accept}
          style={{ display: "none" }}
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) void onFile(f);
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

  const selectedId = graph.selection[0];
  const node = useMemo(
    () => graph.nodes.find((n) => n.id === selectedId),
    [graph, selectedId],
  );

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
      <div className="param-body">
        {[...groups.entries()].map(([group, params]) => (
          <fieldset key={group} className="param-group">
            <legend>{group}</legend>
            {params.map((p) => (
              <Field key={p.key} ctx={current} node={node} spec={p} />
            ))}
          </fieldset>
        ))}
        {(desc?.params.length ?? 0) === 0 && <div className="param-empty">No parameters.</div>}
      </div>
    </div>
  );
}
