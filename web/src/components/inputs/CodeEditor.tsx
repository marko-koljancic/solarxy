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
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import {
  HighlightStyle,
  syntaxHighlighting,
} from "@codemirror/language";
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

/** Sets (or clears) the marked error line, 1-based. */
const setErrorLine = StateEffect.define<number | null>();

const errorLineField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    deco = deco.map(tr.changes);
    for (const e of tr.effects) {
      if (!e.is(setErrorLine)) continue;
      const line = e.value;
      if (line === null || line < 1 || line > tr.state.doc.lines) {
        deco = Decoration.none;
      } else {
        const at = tr.state.doc.line(line);
        deco = Decoration.set([
          Decoration.line({ class: "cm-errorLine" }).range(at.from),
        ]);
      }
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
  "&.cm-focused": { outline: "none" },
});

export interface CodeEditorProps {
  value: string;
  ariaLabel: string;
  /** 1-based line to mark, or undefined. */
  errorLine?: number;
  /** Called on blur and on Cmd/Ctrl+Enter, never per keystroke. */
  onCommit: (next: string) => void;
  /** Fired as the user types, so the host can suppress a stale error mark. */
  onDirty?: (dirty: boolean) => void;
  /** Rows to size to when the content is shorter. */
  minLines?: number;
}

export function CodeEditor({
  value,
  ariaLabel,
  errorLine,
  onCommit,
  onDirty,
  minLines = 3,
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
          lineNumbers(),
          history(),
          wrangleLanguage,
          syntaxHighlighting(highlight),
          errorLineField,
          theme,
          EditorState.allowMultipleSelections.of(true),
          EditorView.lineWrapping,
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
    view.current?.dispatch({ effects: setErrorLine.of(errorLine ?? null) });
  }, [errorLine]);

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
