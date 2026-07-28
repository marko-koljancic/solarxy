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

import { lazy, Suspense, useState } from "react";
import { SnippetEditorModal } from "./SnippetEditorModal";

const CodeEditor = lazy(() => import("./CodeEditor"));

interface Props {
  value: string;
  ariaLabel: string;
  /** The node's cook error, when it has one. */
  error?: string;
  /** The node path shown in the window title, e.g. `/obj/geo1/wrangle1`. */
  path?: string;
  onCommit: (v: string) => void;
}

/** The 1-based line an engine error points at, if it names one. */
export function errorLine(message: string | undefined): number | null {
  if (!message) return null;
  const m = /\bline (\d+)/.exec(message);
  if (!m) return null;
  const n = Number(m[1]);
  return Number.isFinite(n) && n > 0 ? n : null;
}

export function SnippetField({ value, ariaLabel, error, path, onCommit }: Props) {
  // The error belongs to the COMMITTED program. While an edit is in flight
  // the highlight would point at a line the engine never saw, so the editor
  // reports its dirty state and the mark hides until the two agree again.
  const [dirty, setDirty] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const badLine = dirty ? null : errorLine(error);

  return (
    <div className={`snippet-field${badLine ? " has-error" : ""}`}>
      <div className="snippet-body">
        <Suspense fallback={<div className="snippet-loading" aria-busy="true" />}>
          <CodeEditor
            value={value}
            ariaLabel={ariaLabel}
            errorLine={badLine ?? undefined}
            onCommit={onCommit}
            onDirty={setDirty}
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
      {badLine && error ? (
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
          error={badLine && error ? error : undefined}
          errorLine={badLine ?? undefined}
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
