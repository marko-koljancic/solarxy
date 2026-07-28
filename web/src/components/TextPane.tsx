// The Text panel: every text snippet in the document, with an editor.
//
// A pure mirror consumer, like the Tree panel. It creates and edits `text`
// NODES rather than owning a store of its own, which is what makes
// persistence, undo, copy/paste and `.slxy` round-tripping free: a snippet
// is a parameter on a node, and every one of those already works.
//
// It lists snippets from EVERY context, not just the current canvas. A
// script library is a property of the document, and having to remember
// which network you left something in would defeat the point of the panel.

import { lazy, Suspense, useMemo, useState } from "react";
import { dispatch } from "../engine/session";
// Type-only, so the panel does not pull CodeMirror onto its own chunk.
import type { SnippetLanguage } from "./inputs/CodeEditor";
import { wrangleCompletions } from "./inputs/wrangleComplete";
import { ctxKey, type GraphContext, type NodeMirror } from "../engine/types";
import { nodeLabel } from "../flow/nodeLabel";
import { descriptorFor } from "../registry/datatypes";
import { useMirror } from "../store/mirror";
import { usePrefs } from "../store/prefs";
import { toggleMaximize } from "../dock/api";
import { MenuItem, type MenuEntry } from "./menu/MenuItem";

const CodeEditor = lazy(() => import("./inputs/CodeEditor"));

/** The panel's completion source. Builtins, queries and locals only: a
 * snippet has no upstream geometry, so there are no `@attr` lanes to offer
 * and pretending otherwise would complete names the program cannot resolve
 * where it eventually runs. */
const COMPLETIONS = wrangleCompletions(() => []);

const LANGUAGES: { value: SnippetLanguage; label: string }[] = [
  { value: "plain", label: "Plain" },
  { value: "wrangle", label: "Wrangle" },
];

/** One snippet: the node, plus which network it lives in. */
interface Snippet {
  ctx: GraphContext;
  node: NodeMirror;
  name: string;
  body: string;
  language: SnippetLanguage;
}

/** A node's text param, or "" when unset (a fresh snippet). */
function textParam(node: NodeMirror, key: string): string {
  const src = node.params[key];
  if (src && src.kind === "literal" && (src.type === "text" || src.type === "enum")) {
    return String(src.value);
  }
  return "";
}

/** Every `text` node in the document, in context then name order. */
function collectSnippets(contexts: Record<string, { nodes: NodeMirror[] }>): Snippet[] {
  const out: Snippet[] = [];
  for (const [key, graph] of Object.entries(contexts)) {
    // `ctxKey` maps a context to its string; this is the inverse, and the
    // only two shapes it ever produces are "root" and "sub:<id>".
    const ctx: GraphContext =
      key === "root" ? "root" : { subflow: Number(key.slice("sub:".length)) };
    for (const node of graph.nodes) {
      if (node.typeId !== "text") continue;
      out.push({
        ctx,
        node,
        name: textParam(node, "name") || "text",
        body: textParam(node, "body"),
        language: textParam(node, "language") === "wrangle" ? "wrangle" : "plain",
      });
    }
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

export function TextPane() {
  const contexts = useMirror((s) => s.contexts);
  const registry = useMirror((s) => s.registry);
  const current = useMirror((s) => s.current);
  const editorPrefs = usePrefs((s) => s.prefs.editor);
  const [selectedId, setSelectedId] = useState<number | null>(null);

  const snippets = useMemo(() => collectSnippets(contexts), [contexts]);
  // Falls back to the first snippet rather than showing an empty editor
  // beside a populated list, and survives the selected one being deleted.
  const selected = snippets.find((s) => s.node.id === selectedId) ?? snippets[0];

  const create = () => {
    // Created in the CURRENT network, so it lands where you are looking.
    // Position is off to one side of the origin; the canvas is not where
    // this node is meant to be arranged.
    dispatch({ type: "addNode", ctx: current, nodeType: "text", position: [0, 0] });
  };

  const remove = () => {
    if (!selected) return;
    dispatch({ type: "removeNodes", ctx: selected.ctx, ids: [selected.node.id] });
  };

  const setLanguage = (language: SnippetLanguage) => {
    if (!selected) return;
    dispatch({
      type: "setParam",
      ctx: selected.ctx,
      node: selected.node.id,
      key: "language",
      value: { kind: "literal", type: "enum", value: language },
    });
  };

  const setBody = (body: string) => {
    if (!selected) return;
    dispatch({
      type: "setParam",
      ctx: selected.ctx,
      node: selected.node.id,
      key: "body",
      value: { kind: "literal", type: "text", value: body },
    });
  };

  const fileEntries: MenuEntry[] = [
    { label: "New Snippet", onClick: create },
    {
      label: "Delete Snippet",
      disabled: !selected,
      onClick: remove,
    },
    { divider: true },
    {
      label: "Language",
      disabled: !selected,
      submenu: LANGUAGES.map((l) => ({
        label: l.label,
        checked: selected?.language === l.value,
        onClick: () => setLanguage(l.value),
      })),
    },
  ];
  const viewEntries: MenuEntry[] = [
    {
      label: "Maximize Panel",
      shortcut: "Esc to restore",
      onClick: () => toggleMaximize("text"),
    },
  ];

  return (
    <div className="text-pane">
      <nav className="menu-bar text-pane-menu">
        <MenuItem title="File" entries={fileEntries} />
        <MenuItem title="View" entries={viewEntries} />
      </nav>
      <div className="text-pane-body">
        <div className="text-pane-list">
          {snippets.length === 0 ? (
            <div className="text-pane-empty">
              No snippets yet. <button className="crumb-link" onClick={create}>Create one</button> to
              keep a wrangle program or a note with the scene.
            </div>
          ) : (
            snippets.map((s) => (
              <button
                key={s.node.id}
                className={`text-pane-item${s.node.id === selected?.node.id ? " active" : ""}`}
                onClick={() => setSelectedId(s.node.id)}
              >
                <span className="text-pane-name">
                  {nodeLabel(s.node, descriptorFor(registry, s.node.typeId))}
                </span>
                {/* Which network it lives in, because a snippet in a
                    material network is easy to lose track of otherwise. */}
                <span className="text-pane-ctx">
                  {ctxKey(s.ctx) === "root" ? "scene" : "in a container"}
                </span>
              </button>
            ))
          )}
        </div>
        <div className="text-pane-editor">
          {selected ? (
            <Suspense fallback={<div className="snippet-loading" aria-busy="true" />}>
              <CodeEditor
                // Keyed by node so switching snippets rebuilds the editor
                // rather than carrying one buffer's undo history into
                // another's. The language is in the key too, because the
                // editor installs its language extensions once at build.
                key={`${selected.node.id}:${selected.language}`}
                value={selected.body}
                ariaLabel={`${selected.name} body`}
                minLines={20}
                language={selected.language}
                completions={selected.language === "wrangle" ? COMPLETIONS : undefined}
                prefs={editorPrefs}
                onCommit={setBody}
              />
            </Suspense>
          ) : (
            <div className="text-pane-empty">Select a snippet to edit it.</div>
          )}
        </div>
      </div>
    </div>
  );
}
