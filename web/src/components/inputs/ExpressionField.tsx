// The expression lane for a parameter row.
//
// A numeric param is either a literal (its normal widget) or an
// expression, and this is the second state: a monospace field, the value
// it currently resolves to underneath, and the parser's message when it
// does not resolve.
//
// The readout is PULLED (`resolvedParam`) rather than pushed as an event.
// Under a playing runtime a resolved value pushed per cook would be one
// event per expression per frame across the wasm boundary, which is
// exactly the traffic the mirror-and-command model exists to avoid.
//
// Editing is draft-based through the shared `useDraftCommit`: committing
// per keystroke would re-cook the graph on every character, and a
// half-typed expression is a cook error by design, so the node would badge
// and unbadge as you type.

import { useEffect, useRef, useState } from "react";
import { dispatch, getClient } from "../../engine/session";
import type { GraphContext, NodeId } from "../../engine/types";
import { useDraftCommit } from "./draftCommit";
import { formatResolved } from "./expressionLane";

interface Props {
  ctx: GraphContext;
  node: NodeId;
  paramKey: string;
  /** The stored expression text. */
  expr: string;
  /** Bumped by the caller whenever the document changed, so the readout
   * re-pulls (an expression's value moves when what it reads moves). */
  revision: number;
  /** Drops back to the literal widget, restoring the last literal value. */
  onRevert: () => void;
}

export function ExpressionField({
  ctx,
  node,
  paramKey,
  expr,
  revision,
  onRevert,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const { draft, setDraft, commit, revert } = useDraftCommit(expr, (next) =>
    dispatch({
      type: "setParam",
      ctx,
      node,
      key: paramKey,
      value: { kind: "expression", expr: next },
    }),
  );
  const [readout, setReadout] = useState<{ text: string; error: boolean }>({
    text: "",
    error: false,
  });

  useEffect(() => {
    try {
      const r = getClient().resolvedParam(ctx, node, paramKey);
      setReadout(
        r.ok
          ? { text: formatResolved(r.value), error: false }
          : { text: r.error, error: true },
      );
    } catch (e) {
      setReadout({ text: e instanceof Error ? e.message : String(e), error: true });
    }
  }, [ctx, node, paramKey, expr, revision]);

  return (
    <div className="param-expr">
      <input
        ref={inputRef}
        type="text"
        className={`input-field param-expr-input${readout.error ? " has-error" : ""}`}
        spellCheck={false}
        autoComplete="off"
        aria-label={`${paramKey} expression`}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") {
            commit();
            inputRef.current?.blur();
          }
          if (e.key === "Escape") {
            // Escape abandons the edit. It does not remove the
            // expression: that is the `=` toggle's job, and conflating
            // them would make a mistyped character destroy the whole
            // expression.
            revert();
            inputRef.current?.blur();
          }
        }}
      />
      <div
        className={`param-expr-readout${readout.error ? " has-error" : ""}`}
        title={readout.text}
      >
        {readout.text}
      </div>
      <button
        type="button"
        className="param-expr-clear"
        title="Remove the expression and go back to a value"
        onClick={onRevert}
      >
        ×
      </button>
    </div>
  );
}
