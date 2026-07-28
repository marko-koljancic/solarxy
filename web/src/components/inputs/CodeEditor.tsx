// The CodeMirror-backed wrangle editor.
//
// Loaded lazily by `SnippetField`, so CodeMirror lands in its own Vite chunk
// and nothing pays for it until a snippet param is actually shown. The
// player never imports this file at all, and a source-level rule in
// `tokens_drift.rs` keeps it that way.
//
// The commit contract is `SnippetField`'s, unchanged and non-negotiable: a
// wrangle edit is ONE `SetParam` and therefore one undo step. CodeMirror
// fires a transaction per keystroke, so the draft lives here and only blur
// or Cmd/Ctrl+Enter turns it into a command.

import { useEffect, useRef } from "react";
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
} from "@codemirror/autocomplete";
import {
  defaultKeymap,
  history,
  historyKeymap,
  toggleComment,
} from "@codemirror/commands";
import {
  HighlightStyle,
  bracketMatching,
  syntaxHighlighting,
} from "@codemirror/language";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import type { CompletionSource } from "@codemirror/autocomplete";
import { Compartment, EditorState, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  keymap,
  lineNumbers,
  type DecorationSet,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { wrangleLanguage } from "./wrangleLang";

/** Sets (or clears) the marked error position, 1-based line and column. */
const setErrorAt = StateEffect.define<ErrorMark | null>();

export interface ErrorMark {
  line: number;
  /** 1-based column. The engine reports one; only the line was used before. */
  column: number;
  message: string;
}

/** The span to underline for an error reported at `line:column`.
 *
 * The engine's message carries a position, not a range -- it formats
 * `line N, column M` from the failing expression's span but does not send
 * the span itself. So the word starting at that column is underlined, or a
 * single character when the position lands on punctuation or past the end.
 * That is an approximation, and a deliberate one: it points at the token
 * rather than washing the whole line, which is what "column" was for. */
function errorRange(doc: { line: (n: number) => { from: number; to: number; text: string } }, mark: ErrorMark) {
  const line = doc.line(mark.line);
  const start = Math.min(line.from + Math.max(0, mark.column - 1), line.to);
  const rest = line.text.slice(start - line.from);
  const word = /^[A-Za-z_@$][\w.]*/.exec(rest);
  const end = word ? start + word[0].length : Math.min(start + 1, line.to);
  return { from: start, to: Math.max(end, start + 1) > line.to ? line.to : Math.max(end, start + 1) };
}

const errorField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    deco = deco.map(tr.changes);
    for (const e of tr.effects) {
      if (!e.is(setErrorAt)) continue;
      const mark = e.value;
      if (mark === null || mark.line < 1 || mark.line > tr.state.doc.lines) {
        deco = Decoration.none;
        continue;
      }
      const { from, to } = errorRange(tr.state.doc, mark);
      const decorations = [Decoration.line({ class: "cm-errorLine" }).range(tr.state.doc.line(mark.line).from)];
      // A zero-width range would be dropped; only mark a span when there
      // is one to mark.
      if (to > from) {
        decorations.push(
          Decoration.mark({ class: "cm-errorSpan", attributes: { title: mark.message } }).range(from, to),
        );
      }
      deco = Decoration.set(decorations, true);
    }
    return deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** Colors come from the theme tokens, so light and dark follow the rest of
 * the app instead of shipping a second palette that drifts from it. */
const highlight = HighlightStyle.define([
  { tag: tags.comment, color: "var(--text-tertiary)", fontStyle: "italic" },
  { tag: tags.number, color: "var(--cm-number)" },
  { tag: tags.string, color: "var(--cm-string)" },
  { tag: tags.attributeName, color: "var(--cm-attribute)", fontWeight: "600" },
  { tag: tags.constant(tags.variableName), color: "var(--cm-var)", fontWeight: "600" },
  { tag: tags.function(tags.standard(tags.variableName)), color: "var(--cm-builtin)" },
  { tag: tags.special(tags.variableName), color: "var(--cm-query)" },
  { tag: tags.typeName, color: "var(--cm-type)" },
  { tag: tags.operator, color: "var(--text-secondary)" },
]);

const theme = EditorView.theme({
  "&": { fontSize: "1.2rem", backgroundColor: "transparent" },
  ".cm-content": {
    fontFamily: "var(--font-mono)",
    caretColor: "var(--text-primary)",
    padding: "0.4rem 0",
  },
  ".cm-gutters": {
    backgroundColor: "transparent",
    border: "none",
    color: "var(--text-tertiary)",
  },
  ".cm-activeLine": { backgroundColor: "transparent" },
  ".cm-errorLine": { backgroundColor: "var(--cm-error-bg)" },
  // The token the engine pointed at, underlined rather than boxed: a
  // squiggle reads as "here" without hiding the character underneath.
  ".cm-errorSpan": {
    textDecoration: "underline wavy var(--error-badge)",
    textUnderlineOffset: "0.25em",
  },
  ".cm-tooltip": {
    backgroundColor: "var(--surface-overlay)",
    border: "1px solid var(--border-default)",
    borderRadius: "3px",
    color: "var(--text-primary)",
  },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    backgroundColor: "var(--hover-bg)",
    color: "var(--text-primary)",
  },
  ".cm-completionDetail": { color: "var(--text-tertiary)", fontStyle: "normal" },
  ".cm-panels": { backgroundColor: "var(--background-secondary)", color: "var(--text-primary)" },
  ".cm-searchMatch": { backgroundColor: "var(--cm-error-bg)" },
  ".cm-selectionMatch": { backgroundColor: "var(--hover-bg)" },
  "&.cm-focused": { outline: "none" },
});

export interface CodeEditorProps {
  value: string;
  ariaLabel: string;
  /** Where the last cook error was, or undefined. */
  error?: ErrorMark;
  /** Called on blur and on Cmd/Ctrl+Enter, never per keystroke. */
  onCommit: (next: string) => void;
  /** Fired as the user types, so the host can suppress a stale error mark. */
  onDirty?: (dirty: boolean) => void;
  /** Rows to size to when the content is shorter. */
  minLines?: number;
  /** Completion source; omitted for a plain text buffer with no language. */
  completions?: CompletionSource;
  /** Editor preferences (word wrap, line numbers, font size). */
  prefs?: EditorPrefs;
}

/** The editor's own display preferences, saved in Preferences > Display. */
export interface EditorPrefs {
  wordWrap: boolean;
  lineNumbers: boolean;
  /** Font size in px. */
  fontSize: number;
}

export const DEFAULT_EDITOR_PREFS: EditorPrefs = {
  wordWrap: true,
  lineNumbers: true,
  fontSize: 12,
};

export function CodeEditor({
  value,
  ariaLabel,
  error,
  onCommit,
  onDirty,
  minLines = 3,
  completions,
  prefs = DEFAULT_EDITOR_PREFS,
}: CodeEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  // What we last sent upward. Not the same as `value`: a commit followed by
  // a blur runs before the new value has travelled back through the mirror,
  // and comparing against `value` alone would send the edit twice and cost
  // the user a second undo step to get past a no-op.
  const sent = useRef(value);
  // Latest callbacks, read through refs so re-rendering the parent never
  // rebuilds the editor (which would drop the cursor and the undo history).
  const commitRef = useRef(onCommit);
  const dirtyRef = useRef(onDirty);
  commitRef.current = onCommit;
  dirtyRef.current = onDirty;

  useEffect(() => {
    if (!host.current) return;
    const editable = new Compartment();

    const commit = (v: EditorView) => {
      const next = v.state.doc.toString();
      if (next === sent.current) return false;
      sent.current = next;
      commitRef.current(next);
      dirtyRef.current?.(false);
      return true;
    };

    const v = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          ...(prefs.lineNumbers ? [lineNumbers()] : []),
          history(),
          wrangleLanguage,
          syntaxHighlighting(highlight),
          errorField,
          bracketMatching(),
          closeBrackets(),
          search({ top: true }),
          highlightSelectionMatches(),
          ...(completions
            ? [autocompletion({ override: [completions], icons: false })]
            : []),
          theme,
          EditorView.theme({ "&": { fontSize: `${prefs.fontSize}px` } }),
          EditorState.allowMultipleSelections.of(true),
          ...(prefs.wordWrap ? [EditorView.lineWrapping] : []),
          keymap.of([
            {
              // The explicit-commit chord, matching the plain textarea this
              // replaced and `MultilineField` beside it.
              key: "Mod-Enter",
              run: (view) => {
                commit(view);
                return true;
              },
            },
            {
              key: "Escape",
              run: (view) => {
                // Revert to the last committed value and hand focus back, so
                // Escape means the same thing it does in every other field.
                view.dispatch({
                  changes: { from: 0, to: view.state.doc.length, insert: sent.current },
                });
                dirtyRef.current?.(false);
                view.contentDOM.blur();
                return true;
              },
            },
            { key: "Mod-/", run: toggleComment },
            // Before defaultKeymap so completion and search own their keys
            // (Escape closes a completion popup rather than reverting the
            // whole edit, which is what makes Escape safe here).
            ...closeBracketsKeymap,
            ...completionKeymap,
            ...searchKeymap,
            ...historyKeymap,
            ...defaultKeymap,
          ]),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              dirtyRef.current?.(u.state.doc.toString() !== sent.current);
            }
          }),
          EditorView.domEventHandlers({
            blur: (_e, view) => {
              commit(view);
              return false;
            },
            // The canvas keymap must never see typing: a program containing
            // the letter "b" would otherwise bypass the node.
            keydown: (e) => {
              e.stopPropagation();
              return false;
            },
          }),
          editable.of(EditorView.editable.of(true)),
        ],
      }),
    });
    view.current = v;
    return () => {
      // Commit anything in flight before tearing down: switching nodes
      // while mid-edit should not silently discard the edit.
      commit(v);
      v.destroy();
      view.current = null;
    };
    // Built once. `value` is synced by the effect below rather than by
    // rebuilding, so the cursor and undo history survive an external edit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The stored value wins: an undo, a redo, or an edit from the other
  // properties surface all change it underneath us.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    if (value === sent.current) return;
    sent.current = value;
    if (v.state.doc.toString() === value) return;
    v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
    dirtyRef.current?.(false);
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({ effects: setErrorAt.of(error ?? null) });
  }, [error]);

  return (
    <div
      ref={host}
      className="code-editor"
      role="textbox"
      aria-label={ariaLabel}
      aria-multiline="true"
      style={{ minHeight: `${minLines * 1.6}em` }}
    />
  );
}

export default CodeEditor;
