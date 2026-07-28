// A multi-line program param: a syntax-highlighted editor with line numbers
// and error-line marking, plus a button that opens the same editor in a
// resizable window for real work.
//
// Same draft-commit contract as TextField and ExpressionField: the edit is
// ONE command, not one per keystroke. That matters more here than anywhere
// else, because every commit reparses the program and recooks the geometry,
// and a wrangle's input can be hundreds of thousands of elements. Enter
// inserts a newline (this is a code editor, not a field), so committing is
// blur or Cmd/Ctrl+Enter, and Escape abandons the draft.
//
// The error line comes from the node's own cook status rather than from a
// second channel: a wrangle parse error is a cook error whose message the
// engine already formats as "line N, column M: ...". Parsing that back is a
// small price for not inventing a parallel diagnostics path that could
// disagree with the badge the user is looking at.
//
// CodeMirror is loaded lazily. It is by far the heaviest thing the editor
// depends on, and a scene with no wrangle in it should never download a code
// editor; the Suspense fallback is a plain box of the right height, so the
// panel does not jump when it arrives.

import { lazy, Suspense, useMemo, useRef, useState } from "react";
import { getClient } from "../../engine/session";
import type { AttrLane, NodeMirror } from "../../engine/types";
import { usePrefs } from "../../store/prefs";
import { upstreamSource } from "./AttributeNameField";
import { wrangleCompletions } from "./wrangleComplete";
import { errorPosition } from "./snippetError";
import { SnippetEditorModal } from "./SnippetEditorModal";

const CodeEditor = lazy(() => import("./CodeEditor"));

interface Props {
  value: string;
  ariaLabel: string;
  /** The node's cook error, when it has one. */
  error?: string;
  /** The node path shown in the window title, e.g. `/obj/geo1/wrangle1`. */
  path?: string;
  /** The node this program belongs to, for attribute completions. */
  node?: NodeMirror;
  onCommit: (v: string) => void;
}

export function SnippetField({ value, ariaLabel, error, path, node, onCommit }: Props) {
  const editorPrefs = usePrefs((s) => s.prefs.editor);

  // The lane inventory is pulled on demand rather than mirrored: it changes
  // whenever the graph upstream is edited, and a list captured when the
  // editor was built would be stale the moment somebody wired something up.
  // Cached per completion session so one keystroke does not cost a boundary
  // crossing per candidate.
  const lanesRef = useRef<AttrLane[]>([]);
  const completions = useMemo(
    () =>
      wrangleCompletions(() => {
        if (!node) return lanesRef.current;
        const source = upstreamSource(node);
        const summary = source === null ? undefined : getClient().attributeSummary(source);
        lanesRef.current = summary?.point ?? [];
        return lanesRef.current;
      }),
    [node],
  );

  // The error belongs to the COMMITTED program. While an edit is in flight
  // the highlight would point at a line the engine never saw, so the editor
  // reports its dirty state and the mark hides until the two agree again.
  const [dirty, setDirty] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const badAt = dirty ? null : errorPosition(error);

  return (
    <div className={`snippet-field${badAt ? " has-error" : ""}`}>
      <div className="snippet-body">
        <Suspense fallback={<div className="snippet-loading" aria-busy="true" />}>
          <CodeEditor
            value={value}
            ariaLabel={ariaLabel}
            error={badAt ?? undefined}
            onCommit={onCommit}
            onDirty={setDirty}
            completions={completions}
            prefs={editorPrefs}
          />
        </Suspense>
        <button
          type="button"
          className="snippet-expand"
          title="Edit in a window"
          aria-label="Edit in a window"
          onClick={() => setExpanded(true)}
        >
          <ExpandGlyph />
        </button>
      </div>
      {badAt && error ? (
        <p className="snippet-error" role="status">
          {error}
        </p>
      ) : (
        <p className="snippet-hint">
          {dirty ? "Cmd/Ctrl+Enter or click away to run" : " "}
        </p>
      )}
      {expanded && (
        <SnippetEditorModal
          value={value}
          ariaLabel={ariaLabel}
          error={badAt ?? undefined}
          errorText={badAt ? error : undefined}
          path={path}
          onCommit={onCommit}
          onClose={() => setExpanded(false)}
        />
      )}
    </div>
  );
}

function ExpandGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden>
      <path
        d="M9.5 2h4.5v4.5M6.5 14H2V9.5M14 2l-5 5M2 14l5-5"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        fill="none"
      />
    </svg>
  );
}
