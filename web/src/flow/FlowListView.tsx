// The flat node list view (C11, Minimystix FlowListView): a
// per-context table alternative to the graph canvas sharing the mirror
// selection. Row click selects; double-clicking a container enters its
// subflow; the Select button mirrors the Minimystix column. Each row
// carries the radial menu's action set as hover-revealed square buttons,
// through the same flow/nodeActions vocabulary so the two surfaces
// cannot drift.

import { useEffect, useState } from "react";
import { InlineEdit } from "../components/InlineEdit";
import { NodeGlyph } from "../components/NodeGlyph";
import { dispatch } from "../engine/session";
import { assetDisplayName } from "../engine/session";
import { IconBypass, IconDisplay, IconDive, IconRename, IconTrash } from "../icons";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { useUi } from "../store/ui";
import { nodeInfoLine } from "./infoLine";
import {
  diveIntoSubflow,
  isBypassable,
  isContainerType,
  openNodeInfo,
  removeNode,
  setDisplayFlag,
  toggleBypass,
} from "./nodeActions";
import { nodeLabel } from "./nodeLabel";
import { hasVisibleParam, nodeVisible } from "./visibility";

export function FlowListView() {
  const registry = useMirror((s) => s.registry);
  const ctx = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const cook = useMirror((s) => s.cook);

  // Inline rename: double-click on the node cell or F2 via the
  // ui-store rename request; commit is one setParam on `name`.
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const renameRequest = useUi((s) => s.renameRequest);
  useEffect(() => {
    if (renameRequest !== null) {
      setRenamingId(renameRequest);
      useUi.getState().setRenameRequest(null);
    }
  }, [renameRequest]);

  if (graph.nodes.length === 0) {
    return <div className="flow-list-empty">No nodes in this context yet.</div>;
  }

  return (
    <div className="flow-list">
      <table>
        <thead>
          <tr>
            <th>Node</th>
            <th>Type</th>
            <th>Info</th>
            <th>Status</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {graph.nodes.map((n) => {
            const desc = descriptorFor(registry, n.typeId);
            const selected = graph.selection.includes(n.id);
            const status = cook[n.id]?.status;
            const statusText =
              status?.state === "error"
                ? "error"
                : status?.state === "ok"
                  ? `${status.ms.toFixed(1)} ms`
                  : status?.state ?? "";
            const select = () => dispatch({ type: "setSelection", ctx, ids: [n.id] });
            return (
              <tr
                key={n.id}
                className={selected ? "selected" : undefined}
                onClick={select}
                onDoubleClick={() => {
                  if (isContainerType(desc)) diveIntoSubflow(n.id);
                }}
              >
                <td
                  onDoubleClick={(e) => {
                    e.stopPropagation();
                    setRenamingId(n.id);
                  }}
                >
                  {graph.activeOutput === n.id && (
                    <span className="display-dot inline" title="display flag" />
                  )}
                  <NodeGlyph desc={desc} size={13} />
                  {renamingId === n.id ? (
                    <InlineEdit
                      value={nodeLabel(n, desc)}
                      placeholder={desc?.displayName ?? n.typeId}
                      onCommit={(next) =>
                        dispatch({
                          type: "setParam",
                          ctx,
                          node: n.id,
                          key: "name",
                          value: { kind: "literal", type: "text", value: next },
                        })
                      }
                      onClose={() => setRenamingId(null)}
                    />
                  ) : (
                    nodeLabel(n, desc)
                  )}
                </td>
                <td className="flow-list-type">{n.typeId}</td>
                <td className="flow-list-info">{nodeInfoLine(desc, n, assetDisplayName) ?? ""}</td>
                <td className={status?.state === "error" ? "flow-list-error" : undefined}>
                  {statusText}
                  {n.bypassed ? " bypassed" : ""}
                </td>
                <td className="flow-list-actions-cell">
                  {ctx === "root" && hasVisibleParam(desc) && (
                    <button
                      className={`visibility-eye list${nodeVisible(n) ? "" : " off"}`}
                      title={nodeVisible(n) ? "Hide (stays cooked)" : "Show"}
                      onClick={(e) => {
                        e.stopPropagation();
                        dispatch({
                          type: "setParam",
                          ctx,
                          node: n.id,
                          key: "visible",
                          value: { kind: "literal", type: "bool", value: !nodeVisible(n) },
                        });
                      }}
                    >
                      <span className="vis-dot" aria-hidden />
                    </button>
                  )}
                  <span className="row-actions">
                    <button
                      className="row-action"
                      title="Rename (F2)"
                      onClick={(e) => {
                        e.stopPropagation();
                        setRenamingId(n.id);
                      }}
                    >
                      <IconRename size={12} />
                    </button>
                    {ctx !== "root" && (
                      <button
                        className={`row-action${graph.activeOutput === n.id ? " active" : ""}`}
                        title="Set the display flag"
                        onClick={(e) => {
                          e.stopPropagation();
                          setDisplayFlag(ctx, n.id);
                        }}
                      >
                        <IconDisplay size={12} />
                      </button>
                    )}
                    <button
                      className="row-action"
                      title={isContainerType(desc) ? "Enter subflow" : "Not a container"}
                      disabled={!isContainerType(desc)}
                      onClick={(e) => {
                        e.stopPropagation();
                        diveIntoSubflow(n.id);
                      }}
                    >
                      <IconDive size={12} />
                    </button>
                    <button
                      className="row-action"
                      title="Node info"
                      onClick={(e) => {
                        e.stopPropagation();
                        openNodeInfo(n.id, ctx, e.clientX + 16, e.clientY - 20);
                      }}
                    >
                      <span className="row-action-glyph">i</span>
                    </button>
                    <button
                      className={`row-action${n.bypassed ? " active" : ""}`}
                      title={isBypassable(desc) ? "Toggle bypass" : "Not bypassable"}
                      disabled={!isBypassable(desc)}
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleBypass(ctx, n);
                      }}
                    >
                      <IconBypass size={12} />
                    </button>
                    <button
                      className="row-action"
                      title="Delete node"
                      onClick={(e) => {
                        e.stopPropagation();
                        removeNode(ctx, n.id);
                      }}
                    >
                      <IconTrash size={12} />
                    </button>
                  </span>
                  <button
                    className="btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      select();
                    }}
                  >
                    Select
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
