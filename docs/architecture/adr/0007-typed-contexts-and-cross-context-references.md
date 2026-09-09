# 0007. Networks are typed, and data crosses between them by reference rather than by wire

- **Status**: accepted
- **Date**: 2026-09-07

## Context

A node graph in this kind of tool is not one canvas. Geometry operators, material shaders,
image operators and scene-level objects are four different vocabularies with four different
notions of what a wire carries. Putting them all on one canvas means a palette nobody can
navigate, connections that are legal to draw and impossible to evaluate, and a data type
enumeration that has to be the union of everything.

Splitting them raises the question the split creates: how does a value reach from one network
into another. A material has to be reachable from a geometry network. A texture has to be
reachable from a material. A camera in the scene network has to be able to name a rendering
setup that lives elsewhere.

## Options considered

### Option A: typed networks, with crossing by reference

Every graph carries a kind. A node type declares which kinds it may be placed in, and a
container type declares which kind of child canvas it opens. Data crosses a boundary by a
parameter that names another node, never by a wire.

### Option B: one untyped canvas

Every node type available everywhere, with the cook rejecting nonsense.

It makes an illegal graph a runtime error instead of an editor-time one, so the first signal a
user gets that a texture cannot feed a transform is a badge after the fact. The palette can no
longer be filtered, so it is 77 entries deep at every point. And it removes the invariant that
makes the reference model coherent, because if anything can be wired to anything then wires
become the crossing mechanism by default.

### Option C: separate documents per context, linked by file reference

Materials in one file, geometry in another, as some pipelines do.

It converts an in-document reference into a file reference, which brings back the path problem
[0006](0006-slxy-scene-file-format.md) exists to avoid, and it splits undo and save across
documents so a single edit is no longer one transaction.

## Decision

Every graph carries a `ContextKind`, one of `Obj`, `Sop`, `Mat` or `Cop`
(`crates/solarxy-graph/src/document/mod.rs:69`). The kind lives on the graph, not in its
address: the address is separately `Root` or `Subflow(node)`
(`crates/solarxy-graph/src/document/mod.rs:101`), so a subflow is any child network regardless
of its kind.

A node type declares the set of kinds it is placeable in, and a container type declares the
kind of child canvas it opens. Placement is judged against the target graph's kind, and the
same predicate filters the palette.

Data crosses a context boundary through a parameter that names another node, never through a
wire.

## Consequences

The rule is enforced by a registry invariant rather than by convention: any type placeable in
the object network must declare zero ports
(`crates/solarxy-graph/src/registry/mod.rs:537`). The object canvas therefore has no handles
at all, and cross-context data has no wire available to travel on. That single invariant is
what makes "by reference, never by wire" true rather than aspirational.

Container creation is genuinely generic. The engine creates and kinds a child network purely
from the descriptor's declared opener, with no test on the type identifier, and the registry
documentation says so. That is the one place the no-special-casing claim in
[0004](0004-node-type-descriptor-is-the-single-source.md) fully holds.

The reference is declared as a path and stored as an identifier, and the vocabulary is
inconsistent about it. The parameter type is `NodePath`, documented as referencing across
contexts by path, and the user interface renders a path; the stored value is a stable node
identifier, chosen so a rename cannot break a reference. A reader searching the codebase for
path resolution lands in the expression subsystem, which resolves a different and unrelated
kind of path.

That is the second reference mechanism, and the two coexist with different properties.
Expression references address by name and are backed by a rebuilt index
([0005](0005-expressions-read-document-state.md)); node references address by identifier and
their reverse lookup is a scan of the document, which the code documents as a deliberate
choice for interactive-sized documents. So one document contains references that survive a
rename and references that do not, chosen by which widget the user reached for, with nothing in
the file distinguishing them.

Cook order across contexts is a second topological pass over container reference edges, with an
identifier-order fallback if it fails to converge. Reference resolution itself is not
recursive: the cook driver opens the referenced network, takes its active output, and reads the
committed value.

Cycle refusal happens when a reference is written, and only then. The check builds a forbidden
set of the referring node plus every enclosing container up to the root, walks the target's
dependency closure including its whole child-network tree, and refuses on contact
(`crates/solarxy-graph/src/engine/mod.rs:4021`). It is called from exactly one site, the
parameter write. Loading a document and pasting a fragment do not call it, so a hand-edited or
foreign scene can install a reference cycle that the code's stated invariant says is
impossible. The failure is degraded rather than fatal, because resolution is non-recursive and
the context ordering has an append-the-remainder fallback, and the ordering function's own
documentation names the hole. Unlike expressions, node reference chains have no depth cap.

Enforcement: the portless-object-network invariant and the placement check are code and run at
registry construction and at every node insertion. The cycle refusal covers the write path
only.

## Notes

The four kinds are a ceiling in the same sense that two attribute domains are
([0014](0014-two-attribute-domains.md)): a fifth kind is a document-format change, a palette
change and a container type, so it is a decision rather than an addition. Nothing currently
asks for one.
