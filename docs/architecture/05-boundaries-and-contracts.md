# Boundaries and contracts

A boundary that exists only in prose is not a boundary. This document states each one as a
rule a machine could check, gives the dependency allow-list in a form a test can read, and
says for every crossing what actually enforces it today. Where the honest answer is that
nothing does, it says nothing does, and that entry becomes work in
[09-evolution-and-roadmap.md](09-evolution-and-roadmap.md).

The responsibility cards in [04-target-architecture.md](04-target-architecture.md) are the
per-unit view of the same contract. If this document and a card disagree, one of them is a
defect.

## The dependency rule

> **A crate may depend only on crates listed in its allow-list, in every dependency kind
> including `dev-dependencies` and target-conditional dependencies. An edge that is not in
> the allow-list is a defect, whether or not it compiles.**

The phrase "in every dependency kind" is the load-bearing part, and it is not pedantry. Cargo
permits a development-dependency cycle, so the compiler will happily accept an edge that
inverts a layer as long as it only exists for tests. The workspace has exactly one such edge
today: `solarxy-core` takes a dev-dependency on `solarxy-formats`, meaning the foundation
crate's own test suite reaches one layer up. It is benign in effect and instructive in kind,
because it demonstrates that "it builds" is not evidence a layer holds.

Three of the deny-list entries are not merely tidy. They are the reason parts of the system
can exist at all, and each buys something concrete:

**`solarxy-graph` must not depend on `solarxy-renderer`, and the reverse.** Neither edge
exists in any kind. This is what lets the cook be tested without a GPU device, lets the engine
compile into a WebAssembly instance that never touches wgpu, and lets the import worker run a
second GPU-free instance of the same code. The two meet at `solarxy_core::scene::SceneDelta`
and nowhere else.

**`solarxy-host` must not depend on `solarxy-graph`.** This is the single most load-bearing
deny-list entry in the workspace. It is what keeps the shared render host a render host. It is
also the reason the shared application layer has to be a new crate rather than more code in
this one, which is the whole of
[ADR 0012](adr/0012-shared-application-layer-is-a-new-crate.md).

**`solarxy-scenefile` must not depend on anything.** It is the root of the graph and the only
member that can be changed and tested in complete isolation. An edge from it to
`solarxy-core` would look harmless and would couple the file format to the type system it is
deliberately decoupled from.

None of these three is enforced by anything today. Two of them are recorded in comments:
`crates/solarxy-host/Cargo.toml` and `crates/solarxy-host/src/lib.rs` both state the
`solarxy-graph` rule in prose. A comment is a good place to explain a rule and a bad place to
keep one.

## The allow-matrix

This is the canonical list. It is written as an adjacency list rather than a grid because that
is the shape a test would read, and because a fifteen-by-fifteen grid of mostly-empty cells is
harder to check by eye than fifteen short lines.

An entry lists every workspace crate the row may depend on, in any dependency kind. Anything
absent is denied. Third-party dependencies are governed separately, in the per-card
`Must not depend on` fields in [04](04-target-architecture.md), because the interesting
third-party rules are exclusions such as "no windowing toolkit here" rather than an allow-list.

| Crate | May depend on |
|---|---|
| `solarxy-scenefile` | nothing |
| `solarxy-core` | nothing |
| `solarxy-formats` | `solarxy-core` |
| `solarxy-imaging` | `solarxy-core` |
| `solarxy-kernel` | `solarxy-core` |
| `solarxy-bvh` | `solarxy-core` |
| `solarxy-graph` | `solarxy-core`, `solarxy-kernel`, `solarxy-imaging`, `solarxy-formats`, `solarxy-scenefile` |
| `solarxy-renderer` | `solarxy-core`, `solarxy-formats`, `solarxy-bvh` |
| `solarxy-host` | `solarxy-core`, `solarxy-kernel`, `solarxy-renderer` |
| `solarxy-validate` | `solarxy-core`, `solarxy-formats` |
| `solarxy-studio` | `solarxy-graph`, `solarxy-host`, `solarxy-renderer`, `solarxy-core`, `solarxy-formats` |
| `solarxy-render` | `solarxy-studio`, `solarxy-host`, `solarxy-renderer`, `solarxy-graph`, `solarxy-formats`, `solarxy-core` |
| `solarxy-app` | `solarxy-studio`, `solarxy-core` |
| `solarxy-web` | `solarxy-studio`, `solarxy-host`, `solarxy-renderer`, `solarxy-kernel`, `solarxy-core`, `solarxy-scenefile` |
| `solarxy-cli` | `solarxy-studio`, `solarxy-render`, `solarxy-validate`, `solarxy-formats`, `solarxy-core` |
| `solarxy` (root binary) | `solarxy-app`, `solarxy-core` |

`solarxy-studio` does not exist yet. Its row is the target, and the two shell rows above
assume it: today `solarxy-app` depends on `solarxy-graph`, `solarxy-host`, `solarxy-renderer`,
`solarxy-formats` and `solarxy-scenefile` directly, and `solarxy-web` depends on
`solarxy-graph`, `solarxy-formats` and `solarxy-bvh` directly. Those extra edges are the
measure of how much application logic still sits in the shells. They shrink as the migration
in [09](09-evolution-and-roadmap.md) proceeds, and the matrix above is what "done" looks like.

Two present-day edges are also absent from the target and should be read as intentional
removals rather than oversights. `solarxy-renderer` depends on `solarxy-formats` today so it
can decode texture and environment bytes; the target moves that decode to the caller so the
renderer takes pixels rather than files. `solarxy-app` depends on `solarxy-formats` and
`solarxy-scenefile` today because it loads models and scenes itself; the target routes both
through `solarxy-studio`.

### Enforcement

Nothing. There is no `[lints]` table, no dependency assertion, and no CI step that reads this
matrix. `cargo metadata` exposes exactly the data needed, so the check is small: read the
adjacency list, read the metadata, and fail on any edge not in the list, in any kind.

That check is proposed in [09](09-evolution-and-roadmap.md). It is worth stating why it is
the highest-value single check in the set: every layering finding recorded in
[03-current-architecture.md](03-current-architecture.md) is an edge or a near-edge, and a
machine reading a fifteen-line table catches all of them for the cost of one test.

## Crossing 1: TypeScript to WebAssembly

The most contract-like boundary in the system, and the least mechanically protected.

### Shape

The frontend holds exactly one `SolarxyApp` instance, constructed through
`web/src/engine/client.ts`, which wraps the `wasm-bindgen` exports in typed methods. Data
crosses in one of three forms: `Command` values going in, `EventBatch` values coming out, and
asset bytes as typed arrays. Serialization is `serde-wasm-bindgen` on the Rust side against
hand-authored TypeScript interfaces on the other.

Cooked geometry deliberately does not cross. On the web the engine and the renderer compile
into one WebAssembly instance, so geometry moves between them as a reference-count bump inside
the wasm heap and JavaScript never sees a vertex buffer. This is the single most important
performance property of the boundary and it is a consequence of the crate rule above, not of
anything the boundary itself does.

### Who owns the schema

Rust owns it. The serde representation of `solarxy_graph::engine::Command`,
`solarxy_graph::engine::EngineEvent` and the host's own event enum is the contract; the
TypeScript in `web/src/engine/types.ts` is a mirror of it, hand-written, 931 lines, covering
roughly 80 types.

Hand-mirroring is a legitimate choice. It avoids a code generator in the build, it lets the
TypeScript express things the Rust shape does not, and it keeps the frontend readable. What it
does not do is stay correct by itself.

### What actually pins the two sides today

Three tests and one source grep, and between them they cover a small fraction of the surface.

- `crates/solarxy-graph/src/engine/tests.rs:383`, `:1897` and `:4210` each serialize a handful
  of command variants and assert the JSON shape is camelCase. Six variants across the three.
- `crates/solarxy-core/tests/tokens_drift.rs:1088` is a source-level scan asserting that two
  named enums carry `rename_all_fields = "camelCase"`.

Nothing asserts variant exhaustiveness. The Rust `Command` enum has 35 variants and
`EngineEvent` has 21; nothing checks that the TypeScript union has 35 and 21 members, that the
names match, or that each variant's fields match. A variant added in Rust and forgotten in
TypeScript compiles on both sides and fails at runtime as an unhandled case.

### The trap this boundary has already sprung

`#[serde(rename_all = "camelCase")]` on an enum renames the **variants** and nothing else. A
struct variant's **fields** need `rename_all_fields = "camelCase"` as well.

The host event enum carried only the first attribute. Every field in it was a single word, so
nothing was wrong until a variant gained multi-word fields, which then crossed the boundary in
snake case while the TypeScript declared them camel. The frontend read `undefined`, and two
readouts in the still-render dialog were blank for a whole release. Neither side was wrong on
its own. Only the pair was.

`tokens_drift.rs:1088` exists because of that incident, and it is deliberately a source-level
grep rather than a serialization test, because the host event enum lives behind
`cfg(target_arch = "wasm32")` and no native test can construct one.

### The target contract

State it as three rules.

1. **Rust owns the schema; TypeScript mirrors it.** Unchanged, and correct.
2. **Exhaustiveness is checked, not trusted.** A test emits the variant names and field names
   of every boundary enum, and a frontend test asserts the mirror matches that list exactly.
   This catches the whole class the current spot-checks miss, and it does not require a code
   generator.
3. **Errors cross as values, not as panics.** A fallible boundary call returns a typed result
   the frontend can discriminate. A panic in wasm poisons the instance, so any path where the
   engine can refuse must refuse in the return type.

Rule 2 is the one with teeth and it is not written yet.

## Crossing 2: the engine to the renderer

### Shape

`solarxy_core::scene::SceneDelta` at `crates/solarxy-core/src/scene.rs:581`, carrying a list of
`SceneOp` at `:484`. The engine produces a delta; the renderer consumes it. Neither crate
knows the other exists.

The operations cover geometry upsert and removal, transforms, visibility, shadow casting,
validation overlays, lights, cameras, and the environment.

### The contract

The delta is the **only** thing that crosses. A change visible in the viewport that is not
expressible as a `SceneOp` is a request to extend the delta, not a reason to reach across.

### Enforcement

Genuinely good, and by construction rather than by check: the absent Cargo edge makes the
alternative impossible to write. This is the model the other boundaries should be judged
against. You cannot violate this one by accident because there is no name in scope to violate
it with.

The one weakness is on the producing side rather than the boundary itself. The delta is never
empty: the lowering pushes lights, cameras and an environment operation on every frame, so a
consumer that guards work on `delta.ops.is_empty()` never takes the cheap path.
[06-cross-cutting-concerns.md](06-cross-cutting-concerns.md) covers the consequence. The
contract is sound; the producer is imprecise.

## Crossing 3: the document to persistence

### Shape

`solarxy-scenefile` owns the on-disk types and the container. `solarxy-graph` maps a live
document to and from them in one place, `crates/solarxy-graph/src/engine/scenefile.rs`. The
scene file crate never depends on the engine, which is what lets the format be read by
something that is not this engine.

### The contract

Three parts, and the third is the one that gets forgotten.

1. **The archive is content-addressed and integrity-checked.** Asset blobs are stored under
   their own hash.
2. **The file declares a schema version and a minimum reader.** A build refuses a file it
   cannot honestly read rather than opening it partially.
3. **A node type declares its own version and its own migration.** The registry carries an
   optional migration function per type, so the format's version and a node type's version are
   two independent axes. A reader must satisfy both.

### Enforcement

Partial, and unevenly.

`crates/solarxy-graph/tests/registry_drift.rs:40` asserts the checked-in
`schemas/registry.json` matches what the current registry produces, and its failure message
names the command that regenerates it. That is a real contract check on the registry snapshot
the frontend consumes, and it works.

The migration side is weaker in two specific ways, both recorded in
[03](03-current-architecture.md) with their evidence. The schema migration entry point is
called once rather than in a loop, so it steps one version even though its own documentation
describes stepwise behaviour, which is correct today with one step defined and wrong on the
day a second is added. And no fixture at an old version is committed: every migration test
constructs its input with the current writer, so the tests prove the migration is
self-consistent rather than that it reads a file some earlier release actually wrote.

The fix for the second is cheap and is the kind of thing only a person can do: commit a real
file, produced by a real old build, and read it in a test.

## Crossing 4: the worker boundaries

### Shape

The browser runs a second, GPU-free WebAssembly instance in a Web Worker. Geometry parsing and
hierarchy building happen there and results return as bytes. Two codecs carry them:
`crates/solarxy-kernel/src/transfer.rs` for geometry and
`crates/solarxy-bvh/src/transfer.rs` for the ray hierarchy.

### The contract, and where the two codecs differ

The hierarchy codec carries a magic word and a version, at
`crates/solarxy-bvh/src/transfer.rs:29` and `:34`, and refuses a blob that does not match. The
geometry codec carries neither.

That asymmetry is deliberate and worth stating, because it looks like an oversight. Both ends
of each codec are compiled from the same source into the same artefact, so a version mismatch
cannot arise from a deployment skew the way it could across a network. The hierarchy codec
carries a version anyway because a hierarchy is expensive enough to be worth caching beyond a
single session, and a cached blob outlives the build that wrote it.

The rule that follows: **a codec carries a version if and only if its output can outlive the
build that produced it.** Under that rule both current choices are right, and the rule is what
tells you which way to go for the next one.

### Enforcement

Round-trip tests on both codecs. Nothing checks the rule above, and nothing needs to, because
it is a design question answered per codec rather than an invariant that could drift.

## Crossing 5: inside the frontend

Not a wasm boundary, but a real one, and the least respected in the system.

The module allow-list is in [04](04-target-architecture.md), one entry per `web/src` module.
The rule is the same shape as the crate rule: a module may import only from its allow-list.

Four violations exist today and each is the same mistake, state reaching sideways into the
view: `engine` imports components, `store` imports `engine/session`, `dock` and components,
`flow` imports components. Each is a cycle. The consequence is concrete rather than
theoretical: a module that imports the view cannot be tested without a DOM, and cannot be
moved into `solarxy-studio` when its logic belongs there.

### Enforcement

Nothing. TypeScript is happy to compile a cycle. The check is the same shape as the crate one
and is worth doing at the same time, since the allow-list is already written.

## What this document owes

Two things are stated here as target and are not true yet. The `solarxy-studio` rows in the
allow-matrix describe a crate that does not exist. The boundary exhaustiveness test for the
TypeScript mirror does not exist. Both are steps in
[09-evolution-and-roadmap.md](09-evolution-and-roadmap.md), and until they land, this document
is a specification rather than a description.

## Open questions

- Whether the frontend mirror should stay hand-written with an exhaustiveness check, or become
  generated. Generation removes the class of error entirely and adds a build step and a
  generated file to review. The exhaustiveness check is cheaper and leaves the readable
  hand-written types in place. This document proposes the check; the choice is not settled.
- Whether `solarxy-cli` should depend on `solarxy-studio` at all. The terminal surfaces share
  a keymap and a progress view model with the graphical shells, which argues yes, and nothing
  else, which argues that two small duplications are cheaper than a dependency.
- Whether the target removal of `solarxy-renderer`'s dependency on `solarxy-formats` is worth
  its cost. It buys a cleaner renderer that takes pixels rather than files; it costs every
  caller a decode step that the renderer currently performs once.
