// A multi-line program param, with line numbers and error-line highlighting.
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

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { shouldCommit } from "./draftCommit";

interface Props {
  value: string;
  ariaLabel: string;
  /** The node's cook error, when it has one. */
  error?: string;
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

export function SnippetField({ value, ariaLabel, error, onCommit }: Props) {
  const [draft, setDraft] = useState(value);
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const guttersRef = useRef<HTMLDivElement>(null);
  // What we last sent. Enter-to-commit is not used here, but blur still
  // races the mirror round trip the same way, so the guard stays.
  const sentRef = useRef(value);

  useEffect(() => {
    setDraft(value);
    sentRef.current = value;
  }, [value]);

  const commit = () => {
    if (!shouldCommit(draft, sentRef.current)) return;
    sentRef.current = draft;
    onCommit(draft);
  };

  // The gutter is a separate scrolling element, so it has to follow the
  // textarea rather than share its scrollbar.
  const syncScroll = () => {
    if (guttersRef.current && areaRef.current) {
      guttersRef.current.scrollTop = areaRef.current.scrollTop;
    }
  };
  useLayoutEffect(syncScroll, [draft]);

  // The error belongs to the COMMITTED program. While the draft differs the
  // highlight would be pointing at a line the engine never saw, so it is
  // hidden until the two agree again.
  const dirty = draft !== value;
  const badLine = dirty ? null : errorLine(error);
  const lines = draft.split("\n");

  return (
    <div className={`snippet-field${badLine ? " has-error" : ""}`}>
      <div className="snippet-body">
        <div className="snippet-gutter" ref={guttersRef} aria-hidden="true">
          {lines.map((_, i) => (
            <div
              key={i}
              className={`snippet-lineno${badLine === i + 1 ? " error" : ""}`}
            >
              {i + 1}
            </div>
          ))}
        </div>
        <textarea
          ref={areaRef}
          className="snippet-input"
          aria-label={ariaLabel}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          rows={Math.min(Math.max(lines.length, 3), 16)}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onScroll={syncScroll}
          onBlur={commit}
          onKeyDown={(e) => {
            // The canvas keymap must not see typing: `b`, `f`, `1`..`7` and
            // the rest are all bound, and a program full of them would
            // otherwise fly the viewport around while you write it.
            e.stopPropagation();
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              commit();
              areaRef.current?.blur();
            }
            if (e.key === "Escape") {
              setDraft(value);
              areaRef.current?.blur();
            }
          }}
        />
      </div>
      {badLine && error ? (
        <p className="snippet-error" role="status">
          {error}
        </p>
      ) : (
        <p className="snippet-hint">
          {dirty ? "Cmd/Ctrl+Enter or click away to run" : " "}
        </p>
      )}
    </div>
  );
}
