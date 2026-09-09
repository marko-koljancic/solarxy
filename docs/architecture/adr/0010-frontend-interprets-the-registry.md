# 0010. The frontend interprets the registry snapshot, so a node added in Rust needs no frontend change

- **Status**: accepted
- **Date**: 2026-09-07

## Context

There are 77 node types and the number grows every release. Each one has ports with data
types, parameters with types, ranges, units, groups and visibility conditions, a category, a
glyph, a set of contexts it may be placed in, and help text.

If the browser knew any of that per node type, adding a node type in Rust would be a frontend
task as well, and every release would carry a second, parallel edit that nothing forces to
happen. The failure is not that the node is missing from the palette, which someone would
notice, but that it appears with the wrong handle colour, a missing parameter, or a widget
that cannot express its range.

The engine already publishes the whole registry as a snapshot, because
[0004](0004-node-type-descriptor-is-the-single-source.md) makes the descriptor the single
declaration. The question is whether the frontend consumes that snapshot or duplicates it.

## Options considered

### Option A: the frontend is a pure interpreter of the snapshot

The palette, the typed handles, the coercion rules and the parameter panel are written once,
against the snapshot's vocabulary, and know nothing about any particular node type.

### Option B: a component per node type

The conventional React shape: a module per node, each rendering its own parameters.

It is 77 modules that must be written, and one more per node type forever. Worse, each is an
independent opportunity to disagree with the descriptor about a default, a range or a unit, and
the disagreement shows up as a value the user set and the cook did not receive.

### Option C: generate a component per node type from the registry at build time

Keep the per-node component but derive it, so the duplication is mechanical.

It adds a build step that has to run in the right order against a Rust artefact, puts a
generated tree in the repository or in the build output, and moves the drift into the
generator without removing it. It also helps only this shell: the desktop parameter panel that
does not yet exist would read the same snapshot and would need the same interpreter anyway.

## Decision

The frontend renders from the registry snapshot and contains no per-node-type code. A node
type added in Rust appears in the palette, wires with correctly typed and coloured handles,
and renders a complete parameter panel with zero changes under `web/`.

Adding a new parameter type or a new data type is the deliberate exception: that is a new
vocabulary word, and teaching the interpreter a word is a frontend change on purpose.

## Consequences

The contract is written as a test rather than asserted in prose.
`web/src/registry/extensibility.test.ts` fabricates a node type the frontend has never seen
into a snapshot and requires that the same registry-driven helpers the palette and the
parameter panel use can fully interpret it: it appears in the context-filtered palette, its
ports colour and coerce, and every one of its parameters maps to a widget. Its header states
the rule it is guarding.

The snapshot arrives live across the WebAssembly boundary rather than from a file. Nothing
under `web/` reads the checked-in registry JSON; that file exists for the generated
documentation and for drift detection, which is why regenerating it is a documentation
obligation and not a frontend one.

The acknowledged exception is the note node, which is an on-canvas sticky rather than a node
with ports, and is the one bespoke component
(`web/src/flow/NodeCanvas.tsx:48` and `:76`, with the comment saying so).

The corpus found six more branches on a node type identifier, and they belong here rather than
in a footnote. Auto-layout excludes notes (`web/src/flow/layout.ts:46`). The pane toolbar's
look-through picker filters root nodes for `"camera"`
(`web/src/components/PaneToolbar.tsx:280`). Double-click-to-enter-a-container is gated on
the SOP container's type id (`web/src/components/Viewport.tsx`), where the engine now asks its
descriptor instead. The text pane collects nodes of type `"text"`
(`web/src/components/TextPane.tsx:65`). And the parameter panel's generic action button is
diverted for `"render"` into the still-render dialog rather than invoking the engine action
(`web/src/components/ParameterPanel.tsx:410`), which is the most consequential of them because
it makes the action contract non-uniform.

At least seven parameter keys are hardcoded the same way: `name`, `visible`, `description`,
`cast_shadow`, the note node's four, and an attribute widget's `type`. The `visible` key is
additionally hardcoded on the Rust side in the desktop shell, so one concept is spelled by hand
in two languages with no shared constant.

None of those seven type-id branches sit on the registry path, so the guard test does not see
them. Renaming or splitting any of those five node types breaks a user-interface behaviour with
no compile error and no failing test.

One more duplication is worth naming because it crosses a different rule: the handle colour per
data type is 14 hexadecimal literals in TypeScript with no Rust source
(`web/src/registry/datatypes.ts:17`), in a codebase whose palette is owned by
`solarxy_core::theme` and generated into CSS.

And one capability declared in the registry is evaluated only here: a parameter's visibility
condition is declared on the Rust parameter specification and validated by the registry, but no
Rust code evaluates one. The sole evaluator is in the frontend, so a second consumer of the
registry, such as a desktop parameter panel or a documentation generator, has nothing to reuse.

Enforcement: the extensibility test covers the registry path. Nothing covers the branches
outside it, and a check that fails on a node-type string literal in `web/src` outside the
allowed list is a candidate in
[09-evolution-and-roadmap.md](../09-evolution-and-roadmap.md).
