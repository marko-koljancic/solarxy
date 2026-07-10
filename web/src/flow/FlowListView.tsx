// The flat node list view (Phase 7b C11, Minimystix FlowListView): a
// per-context table alternative to the graph canvas sharing the mirror
// selection. Row click selects; double-clicking a container enters its
// subflow; the Select button mirrors the Minimystix column.

import { dispatch } from "../engine/session";
import { assetDisplayName } from "../engine/session";
import { descriptorFor } from "../registry/datatypes";
import { selectGraph, useMirror } from "../store/mirror";
import { nodeInfoLine } from "./infoLine";

export function FlowListView() {
  const registry = useMirror((s) => s.registry);
  const ctx = useMirror((s) => s.current);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const cook = useMirror((s) => s.cook);

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
                  if (desc?.category === "container") {
                    useMirror.getState().setCurrent({ subflow: n.id });
                  }
                }}
              >
                <td>
                  {graph.activeOutput === n.id && (
                    <span className="display-dot inline" title="display flag" />
                  )}
                  {desc?.displayName ?? n.typeId}
                </td>
                <td className="flow-list-type">{n.typeId}</td>
                <td className="flow-list-info">{nodeInfoLine(desc, n, assetDisplayName) ?? ""}</td>
                <td className={status?.state === "error" ? "flow-list-error" : undefined}>
                  {statusText}
                  {n.bypassed ? " bypassed" : ""}
                </td>
                <td>
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
