# 0003. Rust owns document state; a shell mirrors it and mutates only by command

- **Status**: accepted
- **Date**: 2026-09-07

## Context

The browser shell is TypeScript and React on one side of a WebAssembly boundary and Rust on
the other. Both sides can hold state, and the node canvas in particular wants to: a graph
library has its own notion of nodes, edges and selection, and it will happily be the source
of truth for them.

If it is, the document exists twice. Two copies of node positions, two notions of what is
selected, two answers to whether an edge is legal, and a reconciliation problem at every
gesture. Worse, the copy that the user's file is written from would be the Rust one while the
copy the user sees would be the JavaScript one, so any divergence is invisible until a save.

A boundary crossing is also not free. `serde_wasm_bindgen` builds a full plain-JavaScript
object graph per call, so a design that pushed every piece of state across on every change
would pay for the whole document per frame.

## Options considered

### Option A: mirror and command

Rust owns all document state. A shell holds a read-only mirror, fed by batches of events. A
gesture becomes a command dispatched into the engine; the engine applies it, returns the
resulting events, and the mirror updates from them. A monotonic revision number rides every
batch so the mirror can tell it has fallen behind, and recovery is a full snapshot.

### Option B: shared state across the boundary

The frontend writes into the document directly, through fine-grained setters or shared linear
memory.

It is cheaper per gesture and it dissolves the invariant that makes the rest of the system
work. Undo, cycle refusal, coercion, cook invalidation and the reference index all key on the
engine seeing every mutation. A direct write skips all of them, so the first shortcut taken
for latency silently disables the parts of the engine that make a document trustworthy.

### Option C: the frontend owns the document, Rust is a compute service

TypeScript holds the graph, and calls into Rust to cook geometry.

This is the shape that produces two engines. Node semantics, parameter resolution and
validity would live in the frontend, so the desktop shell and the headless render command
would each need their own, and the scene file would be written by whichever surface happened
to be open.

## Decision

Rust owns all document state. A shell mirrors it through event batches and mutates it only by
dispatching a command. A batch carries a monotonic revision; a gap in that sequence, or an
explicit document-replaced event, means the mirror is untrustworthy and it recovers by taking
a full snapshot rather than by patching.

## Consequences

The mirror is genuinely read-only in the browser. `web/src/store/mirror.ts` has no mutator for
document state except `applyEvent`, driven by a batch, and `replaceFromSnapshot`. Every edit
goes through `dispatch` in `web/src/engine/session.ts` into `SolarxyApp::dispatch`, and the
returned batch is applied through one function, `applyToMirror`
(`web/src/engine/session.ts:484`).

The desync mechanism is small enough to read. `applyBatch` sets a resnapshot flag when a batch
carries `documentReplaced` or when `batch.revision > s.revision + 1`, and applies nothing in
that case; the caller then calls `snapshot()` and replaces wholesale
(`web/src/store/mirror.ts:258`).

The decision is honoured by one shell and not yet by the other, and this is the honest state
rather than a nuance. `solarxy_graph::Command` has 35 variants. The browser drives effectively
all of them. `solarxy-app` production code dispatches two: a `SetSelection` at
`crates/solarxy-app/src/state/input/mod.rs:827` and a `SetParam` writing the `visible` key at
the same file's line 864. The desktop is a viewer of documents the browser authors. Reaching
parity is not a matter of porting a UI, and that is the argument of
[0012](0012-shared-application-layer-is-a-new-crate.md).

Two further limits are worth stating rather than discovering.

The command enum is the frontend's whole mutation vocabulary, but it is not the engine
facade's. `Engine` exposes mutating methods outside `apply`, including `preview_param`, which
parks a value that the cook, the scene lowering and picking all read through
`effective_params`. So a parameter drag is document-visible state that produces no command, no
event and no undo entry. The rule binds a shell's editing gestures; it does not bind every
caller of the facade.

Recovery is not yet complete. `replaceFromSnapshot` (`web/src/store/mirror.ts:286`) rebuilds
the graph contexts and the revision and leaves the per-node cook statistics and validation
reports in place, keyed by numeric node id. Loading a document restarts node ids low, so those
entries can land on unrelated nodes. The one path whose job is to make the mirror exactly right
does not clear everything it should.

Enforcement: partial, and by inspection rather than by a check. The mirror store's shape is
what makes a stray write hard, but nothing forbids one. The boundary's serde shapes are pinned
for a handful of variants only, which is [0010](0010-frontend-interprets-the-registry.md)'s
neighbour problem and appears in [10-risks-and-open-questions.md](../10-risks-and-open-questions.md).

## Notes

Selection is the one document concept the frontend legitimately touches twice: React Flow
holds its own `selected` flag per node and is reconciled against the mirror on every re-seed,
with the change stream dispatching `setSelection` back. Node position behaves the same way
during a drag. Both are deliberate, because a gesture that waited for a boundary round trip
per frame would not feel like a drag.
