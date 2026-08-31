// The parameter panel: a pure interpreter of the registry snapshot. For the
// selected node it renders one editor per param (grouped), choosing the
// widget from the descriptor's paramType. Edits go through the preview lane
// (no undo spam) during a drag and commit one authoritative SetParam on
// release, which is what makes the param-drag-to-viewport loop cheap.
//
// A new node reusing existing param types needs zero changes here; a new
// ParamType is a deliberate change (a new widget case).

import { useMemo, useRef, useState, type ReactNode } from "react";
import {
  assetDisplayName,
  dispatch,
  flyToIssue,
  getClient,
  previewParam,
  refsWithMtlTextures,
  stagedManifestNames,
  stageFile,
} from "../engine/session";
import { saveExportToFile } from "../persistence/opfs";
import { pushToast } from "../store/toasts";
import { hasMissing, missingSidecars, referencedSidecars } from "../engine/sidecars";
import { useUi } from "../store/ui";
import type {
  GraphContext,
  NodeMirror,
  ParamSnapshot,
  ParamSource,
  ValidationIssue,
} from "../engine/types";
import { descriptorFor } from "../registry/datatypes";
import { ExpressionField } from "./inputs/ExpressionField";
import {
  acceptsExpression,
  discardParkedExpression,
  parkExpression,
  parkedExpression,
  paramExpression,
  seedExpression,
} from "./inputs/expressionLane";
import { nodeLabel } from "../flow/nodeLabel";
import { nodePathOf } from "../flow/nodeActions";
import {
  paramSections,
  paramTabs,
  paramVisible,
  resolveActiveTab,
  tabLabel,
  VALIDATION_TAB,
} from "./paramVisibility";
import { selectGraph, useMirror, type ValidationReportData } from "../store/mirror";
import { AttributeNameField } from "./inputs/AttributeNameField";
import { TextField } from "./inputs/TextField";
import { MultilineField } from "./inputs/MultilineField";
import { ColorInput } from "./inputs/ColorInput";
import { Popover, renderDoc } from "./Popover";
import { Select } from "./Select";
import { FloatInput } from "./inputs/FloatInput";
import { VectorInput } from "./inputs/VectorInput";
import { SnippetField } from "./inputs/SnippetField";

/** The current value for a param: the node's override, else the default. */
function paramValue(node: NodeMirror, spec: ParamSnapshot): unknown {
  const src = node.params[spec.key];
  if (src && src.kind === "literal") return (src as { value: unknown }).value;
  return spec.default;
}

/** Builds a literal ParamSource of the given descriptor param type. */
function literal(paramType: string, value: unknown): ParamSource {
  const tag =
    paramType === "assetRef"
      ? "asset"
      : paramType === "nodePath"
        ? "nodeRef"
        : paramType === "attributeName" ||
            paramType === "snippet" ||
            paramType === "multilineText"
          ? "text" // every text-backed widget variant stores plain Text
          : paramType;
  return { kind: "literal", type: tag, value } as ParamSource;
}

interface FieldProps {
  ctx: GraphContext;
  node: NodeMirror;
  spec: ParamSnapshot;
}

function Field({ ctx, node, spec }: FieldProps) {
  const expr = paramExpression(node, spec);
  const canExpress = acceptsExpression(spec.paramType);
  // The mirror's revision changes on every applied command, which is
  // exactly when an expression's value can have moved (something it reads
  // was edited, renamed, or undone).
  const revision = useMirror((s) => s.revision);

  if (expr !== null) {
    return (
      <div className="param-row">
        <label className="param-label">
          {spec.label}
          {/* Parks the text on the way out, so the same click brings it
              back. The field's clear control is the one that discards. */}
          <ExprToggle
            active
            onClick={() => {
              parkExpression(ctx, node.id, spec.key, expr);
              revertToLiteral(ctx, node, spec);
            }}
          />
        </label>
        <ExpressionField
          ctx={ctx}
          node={node.id}
          paramKey={spec.key}
          expr={expr}
          revision={revision}
          onRevert={() => {
            discardParkedExpression(ctx, node.id, spec.key);
            revertToLiteral(ctx, node, spec);
          }}
        />
      </div>
    );
  }
  return (
    <LiteralField ctx={ctx} node={node} spec={spec} canExpress={canExpress} />
  );
}

/** The small `=` affordance that swaps a row between its expression lane
 * and its value widget.
 *
 * It reads as a mode switch and now behaves as one: switching away keeps
 * the expression, switching back restores it. Discarding is the field's
 * clear control, which is the only place that says "remove". The two used
 * to perform the identical destructive action, so an expression could be
 * lost to the control that looked reversible. */
function ExprToggle({ active, onClick }: { active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      className={`param-expr-toggle${active ? " active" : ""}`}
      title={
        active
          ? "Show the value instead, keeping the expression"
          : "Drive this with an expression"
      }
      aria-label={active ? "Switch to the value" : "Switch to an expression"}
      aria-pressed={active}
      onClick={onClick}
    >
      =
    </button>
  );
}

/** Drops the expression, restoring the param to a plain value.
 *
 * Writes the value the expression last resolved to rather than the spec
 * default, so turning an expression off leaves the object where it was
 * instead of snapping it back to 1. */
function revertToLiteral(ctx: GraphContext, node: NodeMirror, spec: ParamSnapshot) {
  let value: unknown = spec.default;
  try {
    const r = getClient().resolvedParam(ctx, node.id, spec.key);
    if (r.ok) value = r.value.value;
  } catch {
    // No readout available: the spec default is the honest fallback.
  }
  dispatch({
    type: "setParam",
    ctx,
    node: node.id,
    key: spec.key,
    value: literal(spec.paramType, value),
  });
}

function LiteralField({
  ctx,
  node,
  spec,
  canExpress,
}: FieldProps & { canExpress: boolean }) {
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
        {canExpress && (
          <ExprToggle
            active={false}
            onClick={() =>
              dispatch({
                type: "setParam",
                ctx,
                node: node.id,
                key: spec.key,
                // An expression switched off earlier in this session comes
                // back verbatim. Otherwise seed from the current value, so
                // the field opens on something that already resolves
                // rather than on a blank that immediately badges the node.
                value: {
                  kind: "expression",
                  expr:
                    parkedExpression(ctx, node.id, spec.key) ??
                    seedExpression(value),
                },
              })
            }
          />
        )}
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
          <Select
            width={140}
            ariaLabel={spec.label}
            value={String(value)}
            options={spec.enumVariants.map(([key, lbl]) => ({ value: key, label: lbl }))}
            onChange={(v) => commit(v)}
          />
        </div>
      );
    case "text":
      return (
        <div className="param-row">
          {label}
          <TextField
            value={String(value ?? "")}
            ariaLabel={spec.label}
            onCommit={commit}
          />
        </div>
      );
    case "multilineText":
      // Stacked, like the snippet row: a full-width editor under its label
      // rather than squeezed into the value column, because prose needs the
      // width more than the row needs its grid.
      return (
        <div className="param-row param-row-stacked">
          {label}
          <MultilineField
            value={String(value ?? "")}
            ariaLabel={spec.label}
            onCommit={commit}
          />
        </div>
      );
    case "attributeName":
      return (
        <div className="param-row">
          {label}
          <AttributeNameField
            ctx={ctx}
            node={node}
            spec={spec}
            value={String(value ?? "")}
            onCommit={commit}
          />
        </div>
      );
    case "snippet":
      return <SnippetRow ctx={ctx} node={node} spec={spec} label={label} onCommit={commit} />;
    case "color":
      return (
        <div className="param-row">
          {label}
          <ColorInput value={(Array.isArray(value) ? value : [0, 0, 0, 1]) as number[]} onCommit={commit} />
        </div>
      );
    case "assetRef":
      return <AssetField ctx={ctx} node={node} spec={spec} label={label} />;
    case "nodePath":
      return <NodePathField ctx={ctx} node={node} spec={spec} label={label} />;
    case "action":
      return <ActionField ctx={ctx} node={node} spec={spec} label={label} />;
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
/** A Snippet param: the multi-line program editor.
 *
 * The error line is read from the node's own cook status rather than from a
 * separate diagnostics channel, so the highlight and the node badge can
 * never disagree: a wrangle parse failure IS the cook error, formatted by
 * the engine as "line N, column M: ...". */
function SnippetRow({
  ctx,
  node,
  spec,
  label,
  onCommit,
}: FieldProps & { label: ReactNode; onCommit: (v: string) => void }) {
  const status = useMirror((s) => s.cook[node.id]?.status);
  const error = status?.state === "error" ? status.message : undefined;
  const value = paramValue(node, spec);
  return (
    <div className="param-row param-row-stacked">
      {label}
      <SnippetField
        value={String(value ?? "")}
        ariaLabel={spec.label}
        error={error}
        // The window titles itself with the node path and the param key, so
        // several open editors are tellable apart.
        path={`${nodePathOf(ctx, node)}/${spec.key}`}
        node={node}
        onCommit={onCommit}
      />
    </div>
  );
}

/** An Action param: a button whose press is routed by node
 * type. Export nodes run the engine's encoder and save the bytes through
 * the File System Access flow; the render node is HOST-interpreted: the engine
 * resolves what the node asks for and the still dialog opens on it, without
 * moving any pane. */
function ActionField({ ctx, node, spec, label }: FieldProps & { label: ReactNode }) {
  const run = async () => {
    if (node.typeId === "render") {
      // The engine answers what the node says. This side used to work it out
      // from the node's params, which is how two of them came to be authored
      // and read by nothing, and it is not a rule two implementations can hold
      // in step.
      //
      // The viewport is deliberately left alone. A shot is a property of the
      // scene and the pane is where someone happens to be looking, so the job
      // builds its own camera from the node's and nothing moves.
      try {
        const settings = getClient().renderSettings(ctx, node.id);
        useUi.getState().setStillRequest({ ctx, node: node.id, settings });
      } catch (e) {
        pushToast(e instanceof Error ? e.message : String(e), "error");
      }
      return;
    }
    try {
      const result = getClient().invokeAction(ctx, node.id, spec.key);
      await saveExportToFile(result.bytes, result.filename, result.mime);
      pushToast(`Exported ${result.filename}`);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), "error");
    }
  };
  return (
    <div className="param-row">
      {label}
      <button className="tbtn param-action" onClick={() => void run()}>
        {spec.label}
      </button>
    </div>
  );
}

/** The cross-context reference picker: candidates come from
 * the root graph filtered by the descriptor's accept constraint (`opens`
 * containers or one exact type), and the stored value is the target's
 * stable node id, so renames never break a reference. A value pointing at
 * a vanished node stays selectable as a labelled Missing entry rather
 * than being silently rewritten. */
function NodePathField({ ctx, node, spec, label }: FieldProps & { label: ReactNode }) {
  const registry = useMirror((s) => s.registry);
  const rootNodes = useMirror((s) => selectGraph(s, "root").nodes);
  const value = paramValue(node, spec) as number | null;
  const accept = spec.nodePath;
  const candidates = rootNodes.filter((n) => {
    if (!accept) return false;
    if (accept.kind === "opens") return descriptorFor(registry, n.typeId)?.opens === accept.opens;
    return n.typeId === accept.typeIs;
  });
  const missing = value != null && !candidates.some((n) => n.id === value);
  const commit = (v: number | null) =>
    dispatch({
      type: "setParam",
      ctx,
      node: node.id,
      key: spec.key,
      value: { kind: "literal", type: "nodeRef", value: v },
    });
  return (
    <div className="param-row">
      {label}
      <Select
        width={140}
        ariaLabel={spec.label}
        value={value == null ? "" : String(value)}
        options={[
          { value: "", label: "None" },
          ...(missing ? [{ value: String(value), label: `Missing node ${value}` }] : []),
          ...candidates.map((n) => ({
            value: String(n.id),
            label: nodeLabel(n, descriptorFor(registry, n.typeId)),
          })),
        ]}
        onChange={(v) => commit(v === "" ? null : Number(v))}
      />
    </div>
  );
}

function AssetField({ ctx, node, spec, label }: FieldProps & { label: ReactNode }) {
  const [pending, setPending] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const current = String(paramValue(node, spec) ?? "");

  // Multi-file staging: a multi-file model (gltf + bin + textures) selects
  // everything at once (or via the folder picker); every file stages (the
  // sidecar resolver matches companions by name at parse time) and the
  // param points at the primary (the first file matching the accepted
  // extensions). If the primary references companions that are still
  // missing, the missing-sidecars dialog opens and the param write defers
  // to its completion, so the import never has to fail on a lone .gltf.
  const onFiles = async (files: File[]) => {
    setPending(true);
    try {
      const exts = spec.accept.map((a) => a.toLowerCase());
      const primary =
        files.find((f) => exts.some((ext) => f.name.toLowerCase().endsWith(ext))) ?? files[0];
      let primaryHash = "";
      for (const file of files) {
        const { hash } = await stageFile(file);
        if (file === primary) primaryHash = hash;
      }
      if (!primaryHash) return;
      const shallow = referencedSidecars(primary.name, new Uint8Array(await primary.arrayBuffer()));
      const refs = await refsWithMtlTextures(shallow, files);
      const missing = missingSidecars(refs, stagedManifestNames());
      if (hasMissing(missing)) {
        useUi.getState().setSidecarPrompt({
          primaryName: primary.name,
          primaryHash,
          missing,
          complete: { kind: "setParam", ctx, node: node.id, key: spec.key },
        });
        return;
      }
      dispatch({ type: "setParam", ctx, node: node.id, key: spec.key, value: literal("assetRef", primaryHash) });
    } finally {
      setPending(false);
    }
  };

  const clear = () => {
    dispatch({ type: "setParam", ctx, node: node.id, key: spec.key, value: literal("assetRef", "") });
  };

  const onInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    if (files.length > 0) void onFiles(files);
    e.target.value = "";
  };

  const display = current ? assetDisplayName(current) ?? `${current.slice(0, 10)}…` : "no file";

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
          onChange={onInputChange}
        />
      </div>
    </div>
  );
}

/** The parameter editor, hosted by both the dock panel and the floating
 * P panel.
 *
 * `surface` names which of the two this instance is, which is all it needs
 * to find its own pin. Everything else is identical, deliberately: one
 * editor with two hosts, rather than a second editor that drifts. */
export function ParameterPanel({
  surface = "docked",
}: {
  surface?: "docked" | "floating";
} = {}) {
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const cook = useMirror((s) => s.cook);
  const reports = useMirror((s) => s.reports);
  const pinnedId = useUi((s) => s.propsPin[surface]);

  // A pin wins over the selection, and survives the pinned node being in
  // another context: `graph` is the CURRENT canvas, so a pin followed
  // across a dive would silently show nothing. Falling back to the
  // selection in that case would be worse (the header would name one node
  // while the rows edited another), so an out-of-context pin renders as a
  // clearly-labelled empty state below.
  const selectedId = graph.selection[0];
  const targetId = pinnedId ?? selectedId;
  const node = useMemo(
    () => graph.nodes.find((n) => n.id === targetId),
    [graph, targetId],
  );
  // The active tab lives in the ui store so the Properties menu bar can
  // read it (Reset Current Tab); falls back to the first tab whenever the
  // selection's group set no longer contains it (switching node types).
  const tab = useUi((s) => s.paramTab);
  const setTab = useUi((s) => s.setParamTab);

  if (!node) {
    return (
      <div className="param-panel empty">
        {pinnedId !== null ? (
          <span>
            Pinned to a node in another network.{" "}
            <button
              className="crumb-link"
              onClick={() => useUi.getState().setPropsPin(surface, null)}
            >
              Unpin
            </button>{" "}
            to follow the selection again.
          </span>
        ) : (
          <span>Select a node to edit its parameters.</span>
        )}
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
  // Tabs (Minimystix underline pattern, D1): general first, the
  // rest in declaration order, plus a Validation tab when a report exists.
  const report = reports[node.id];
  const isVisible = (p: ParamSnapshot) => paramVisible(p, desc?.params ?? [], node.params);
  const tabs = paramTabs(desc?.params ?? [], Boolean(report), isVisible);
  const active = resolveActiveTab(tabs, tab);

  return (
    <div className="param-panel">
      <div className="param-header">
        <span className="param-title">{nodeLabel(node, desc)}</span>
        {pinnedId !== null && (
          // The panel must never silently lie about what it is editing: a
          // pinned surface looks identical to an unpinned one until you
          // click another node and nothing happens.
          <button
            className="param-pinned"
            title="Pinned to this node. Click to follow the selection again."
            onClick={() => useUi.getState().setPropsPin(surface, null)}
          >
            pinned
          </button>
        )}
        {stats?.image != null ? (
          <span className="param-stats">
            {stats.image[0]} × {stats.image[1]}
          </span>
        ) : stats?.points !== undefined ? (
          <span className="param-stats">
            {stats.points} pts · {stats.prims} tris · {stats.meshes} mesh
          </span>
        ) : null}
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
            {paramSections((groups.get(active) ?? []).filter(isVisible)).map((section, i) => (
              <div key={section.subgroup ?? `_${i}`} className="param-section">
                {section.subgroup !== undefined && (
                  <div className="param-subgroup">{section.subgroup}</div>
                )}
                {section.params.map((p) => {
                  // Registry-driven map-overrides-factor indicator: a param
                  // declaring drivenByPort dims while that input port is
                  // connected (the map fully drives the channel; the factor
                  // value is preserved for when the map disconnects).
                  const driven =
                    p.drivenByPort != null &&
                    graph.edges.some((e) => e.to === node.id && e.toPort === p.drivenByPort);
                  if (!driven) {
                    return <Field key={p.key} ctx={current} node={node} spec={p} />;
                  }
                  return (
                    <div key={p.key} className="param-driven" title="Driven by the connected input">
                      <Field ctx={current} node={node} spec={p} />
                      <div className="param-driven-hint">Driven by connected input</div>
                    </div>
                  );
                })}
              </div>
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


/** "general" -> "General". */
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
