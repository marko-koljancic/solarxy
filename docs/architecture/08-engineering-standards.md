# 08. Engineering standards

This is the document a reviewer points at. Each rule states what to do, why it exists, and
where the codebase currently stands against it. Rules with a `Today` line are describing the
present state honestly, including where the present state fails the rule.

Three things about how to read it.

**Rules are prescriptive; `Today` lines are descriptive.** Where a rule is not met, the gap is
named with a count and a citation rather than smoothed over. Nothing here claims the codebase
already complies.

**Size rules are a ratchet, not a retroactive gate.** A cap that is violated 277 times on the
day it ships is decoration. The size section states the baseline and the burn-down rule
explicitly, and no existing file is in violation of anything until it is picked up by an owner.

**Every example is real.** Every path was opened before it was cited. Where a claim could not
be verified from the code it is in the open questions at the end rather than in a rule.

## 1. Comments and documentation

The problem in this codebase is not too few comments. It is that some comments carry release
narrative, unresolvable referents, or claims the code stopped honouring, and a reader cannot
tell those from the ones that are load-bearing. The rules below are written to separate them.

### 1.1 A public item's doc comment states the contract, not the signature

**Rule.** A doc comment on a public item says what it does, what invariants it maintains, what
units and coordinate spaces its values are in, what it panics on, what its error conditions
are, and what its performance characteristics are where those matter to a caller. A comment
that restates the name and the types is noise and should be deleted.

**Why.** The signature is already in the rustdoc output next to the prose. Repeating it costs
a line and buys nothing, and worse, it trains readers to skip doc comments, which is how a
comment that does carry a contract gets skimmed past.

A good example already in the tree, `crates/solarxy-graph/src/refs.rs:27`:

```rust
/// How deep a chain of `ch()` references may go.
///
/// `SetParam` refuses a cycle at write time, so a loop should be
/// unreachable; this is the backstop for the paths that bypass it (a
/// hand-edited document, a pasted fragment) and it is what stops a cycle
/// from being a stack overflow instead of an error message.
pub const MAX_REF_DEPTH: usize = 32;
```

Six lines, and every one of them answers a question the declaration does not: why the constant
exists at all, which callers reach it, and what failure it converts into a message. The
alternative, `/// The maximum reference depth.`, would be strictly worse than nothing.

**Today.** This is largely held. A sweep of trivial accessors (`len`, `is_empty`, `id`) across
`solarxy-graph`, `solarxy-core`, `solarxy-kernel` and `solarxy-host` found them mostly
undocumented rather than documented redundantly, which is the correct outcome. Where one does
carry a comment it earns it: `crates/solarxy-kernel/src/set.rs:72` says "Number of per-vertex
elements in the buffer", which names the domain and is not derivable from `len(&self) -> usize`.

### 1.2 Inline comments explain why, never what

**Rule.** An inline comment is for a non-obvious trade-off, an invariant a future reader would
break by accident, the source of an algorithm, or a workaround with a link to the thing being
worked around. It is never a paraphrase of the line below it.

**Why.** A comment that restates the code has to be maintained in lockstep with it and buys
nothing when it is correct, so its expected value is negative.

A good example, `crates/solarxy-renderer/src/pathtrace/backend.rs:729`:

```rust
// Swapped **before** the dispatch, not after it, and the difference is
// not cosmetic. Everything that reads the accumulator reads the write
// slot, so a swap after the last dispatch of a run would leave the
// converged branch above resolving the slot from the dispatch before
// it -- or, after a single dispatch, one nothing has ever written.
```

The comment protects an ordering that reads as arbitrary and is not. Deleting it makes the two
lines below it look like they could be moved.

### 1.3 If a comment exists because the code is unclear, fix the code

**Rule.** Before writing a comment that explains what a block does, try renaming the binding,
extracting a function, or introducing a named type. Only when none of those works does a
comment become the right fix.

**Why.** The extracted function's name is checked by the compiler every time it is called. The
comment is checked by nobody.

`crates/solarxy-renderer/src/pathtrace/scene.rs` shows the pattern applied: `BuildPolicy` is an
enum with two variants rather than a boolean plus a paragraph explaining what `true` means, so
`BuildPolicy::Deferred` at a call site needs no comment at all.

### 1.4 Banned outright

Each of these is a defect on sight, not a matter of taste.

- ASCII banner dividers. A row of `=` or `-` characters delimiting a section of a file is a
  file-size symptom wearing a decoration; split the file instead.
- Step-by-step narration of self-evident code.
- Commented-out code. Git holds it.
- Changelog or authorship history in a comment. Git holds that too.
- `TODO` or `FIXME` without a linked issue. An unlinked marker is a wish, and the codebase
  already has no place to put the follow-up, since the planning documents that would carry it
  live outside this repository.
- Restating a constraint the compiler already enforces.
- Marketing or grandiose language. Nothing here is powerful, robust, or seamless.
- Invented terminology that appears nowhere else in the codebase, and unresolvable referents.
  See 1.6 for a real instance.

### 1.5 A comment that contradicts the code is a defect, not a cosmetic issue

**Rule.** A doc comment stating something the code does not do is treated as a bug: it gets a
fix, not a formatting pass. A reviewer who spots one blocks the change that would have shipped
past it.

**Why.** A wrong comment is worse than no comment, because it is trusted. Three instances in
this repository, all verified.

**Instance one, the worked example.** `crates/solarxy-app/src/state/render.rs:1` opens:

```rust
//! `State::render`: per-frame entry point. Computes the pane rectangles,
//! assembles each pane's parameters and hands them to `solarxy_host::render_pane`,
//! then drives the egui sidebar/menu/HUD/console paint at the end.
//!
//! The pane body itself is not here. It lives in `solarxy-host` beside the web
//! shell's copy of the same call, which is the point: what remains in this file
//! is the assembly only a desktop shell can do.
```

There is no `solarxy_host::render_pane`. A workspace-wide search for `fn render_pane` returns
exactly two definitions, `crates/solarxy-app/src/state/render.rs:165` and
`crates/solarxy-web/src/app.rs:5458`, which is to say the function the comment claims is shared
is implemented separately in each shell. The shared function that does exist is
`solarxy_host::pane::encode_pane_passes` at `crates/solarxy-host/src/pane.rs:410`, and it is a
narrower thing: it encodes the pass chain, not the pane. The paragraph beginning "The pane body
itself is not here" is false in its entirety.

The cost is not aesthetic. A contributor reading that header concludes the two shells already
share their per-pane path and that a change there lands on both. It does not, which is exactly
the class of drift that put `render_pane` on the list in
[ADR 0012](adr/0012-shared-application-layer-is-a-new-crate.md).

**Instance two.** `crates/solarxy-validate/src/lib.rs:20` says of the crate's public types:

```rust
//! Public types in this crate are part of Solarxy's stable wire format and
//! are guarded by `cargo-semver-checks` against the published baseline.
```

A search for `semver-checks` or `semver_checks` across `.github/`, the workspace `Cargo.toml`,
every member manifest and `scripts/` returns nothing. There is no semver gate. The wire-format
stability promise this crate makes to external consumers is enforced by nobody. The fix is
either the tool or a corrected sentence, and the choice is a real decision rather than an
editing pass, which is precisely why it is a defect and not a typo.

**Instance three.** `crates/solarxy-renderer/src/pipelines.rs:1` says:

```rust
//! [`InspectionPipelines`] (reserved for inspection-mode-specific pipelines
//! — empty in 0.6.0 Stream A; populated by the Overdraw work in Stream D).
```

`InspectionPipelines` at `pipelines.rs:196` carries two fields, `overdraw_count` and
`overdraw_show`. It is not empty. The comment also carries two work-stream identifiers from a
planning document that is not in this repository, which is the pattern `no_planning_codes_in_comments`
(`crates/solarxy-core/tests/tokens_drift.rs:915`) exists to stop. That test recognises work-item
codes, numbered stages and phases, and hyphenated decision codes; the "Stream A" shape is not in
its families, so this one survives. The rule and its enforcement disagree here, and the
enforcement is the half that should move.

### 1.6 Length is earned or it is not; long is not the same as bad

**Rule.** A module header is as long as the reasoning a reader needs and no longer. The test is
per paragraph: does removing this paragraph make the file harder to change correctly? Release
narrative, decision history and unresolvable referents fail that test at any length.

Two headers in the tree, both long, and the difference between them is instructive.

**Earned.** `crates/solarxy-renderer/src/env_dist.rs:1` runs 46 lines to explain an importance
sampling distribution. Its three sections answer: why an environment needs a distribution at
all (an outdoor sky is mostly dim with a tiny bright sun, so uniform sampling produces specks
rather than slow convergence); why each weight carries the `sin(theta)` its row subtends (an
equirectangular projection is not equal-area, and the correction is one multiply at build time
against a three-hundred-to-one oversampling of the poles); and why the tables are cumulative
and binary-searched rather than inverted and interpolated (an interpolated inverse produces a
sampler whose real density is not the one the shader reports). Every one of those is a thing a
maintainer would otherwise "simplify" and break. Forty-six lines is cheap for that.

**Not earned, in one specific respect.** The same header says, three times, "the source", "the
source material", and "the source records this as a known bug of its own". A reader cannot
resolve that referent from anything in the repository. The prose is load-bearing and the
citation is missing, so the reader is told a correction was made against a reference they
cannot consult. This is the concrete instance of the banned "unexplained referent" in 1.4, and
the fix is one sentence naming the class of thing being corrected, not a shorter header.

**Not earned, a different way.** `crates/solarxy-host/src/lib.rs:10` spends a paragraph on why
the backend trait was written at one release rather than an earlier one, and
`crates/solarxy-renderer/src/pathtrace/scene.rs:25` spends one on what a particular release's
verification depended on. Both are decision records, and both are true. Neither belongs in a
source file: a decision record is what [adr/](adr/) is for, and a source file that carries one
accumulates a second, then a third, and eventually a reader has to date-sort a header to know
what is current. Move the reasoning to an ADR and leave the source header with the invariant
and a pointer.

### 1.7 Every crate and every non-trivial module gets a header

**Rule.** A crate root says what lives in the crate, what deliberately does not, and how it
relates to its neighbours. A module that is more than a bag of small helpers does the same at
its own scale.

**Why.** The `Does not own` half is the part that prevents drift, and it is the half that only
exists if someone writes it. `crates/solarxy-host/src/lib.rs:24` is the model: a section headed
"What this crate is not", stating that the crate does not depend on `solarxy-graph` and why
that matters.

**Today, and this is a real gap.** The two largest shell crates have no crate-level
documentation at all. `crates/solarxy-app/src/lib.rs:1` and `crates/solarxy-cli/src/lib.rs:1`
both begin directly with `#![warn(clippy::pedantic)]`, with no `//!` line anywhere, at 17,324
and 18,082 lines respectively. Meanwhile `crates/solarxy-bvh/src/lib.rs` gives 38 header lines
for a 2,788-line crate and `crates/solarxy-render/src/lib.rs` gives 27 for 4,293. Documentation
effort is inversely correlated with crate size, so the two crates a newcomer most needs
orienting in are the two with no orientation.

CI runs `cargo doc --no-deps --workspace --all-features` under `RUSTDOCFLAGS=-D warnings` and
passes on both, because nothing requires a crate root to say what the crate is for. Adding
`#![warn(missing_docs)]` to a crate root would not catch this either, since that lint governs
items rather than the crate itself. The check that would catch it is a two-line test asserting
every `crates/*/src/lib.rs` begins with `//!`.

## 2. Glossary

One word per concept, across Rust, TypeScript, UI copy and these documents. Where the codebase
currently uses two words for one thing, that is stated, because a glossary that pretends
otherwise is a wish list.

| Term | Definition | Where it is defined |
|---|---|---|
| **node** | One instance in a graph, with a stable id and a type. | `crates/solarxy-graph/src/document/mod.rs:37` |
| **node type** | A registered kind of node, with ports, parameters and a cook body. 77 are registered. | `crates/solarxy-graph/src/nodes/mod.rs` |
| **port** | A typed input or output socket on a node. Connection legality is the coercion matrix. | `crates/solarxy-graph/src/registry/` |
| **parameter** | A named authored value on a node. May be a literal or an expression. | `crates/solarxy-graph/src/params.rs` |
| **cook** | Evaluating a node to produce its outputs. Budgeted and resumable. Never "evaluate", "compute" or "execute" for this. | `crates/solarxy-graph/src/cook/` |
| **registry** | The catalogue of node types, and the snapshot of it the frontend interprets. | `crates/solarxy-graph/src/registry/` |
| **context** | One graph canvas, of kind Obj, Geo, Mat or Tex. The root canvas is a context. | `crates/solarxy-graph/src/document/mod.rs` |
| **subflow** | A child context owned by a container node. | `GraphContext::Subflow`, same file |
| **delta** | The engine-to-renderer scene contract. The only thing that crosses between them. | `crates/solarxy-core/src/scene.rs` |
| **attribute** | Named per-element data on geometry, in a lane of f32, vec2, vec3 or vec4. | `crates/solarxy-kernel/src/set.rs` |
| **domain** | Which elements an attribute binds to. Exactly two, point and primitive. See [ADR 0014](adr/0014-two-attribute-domains.md). | `AttributeDomain`, `set.rs:34` |
| **lane** | One named attribute within a domain. | `crates/solarxy-kernel/src/set.rs` |
| **placement** | A per-mesh instance transform. Geometry carrying placements is instanced. | `CookedMesh::instances`, `crates/solarxy-core/src/scene.rs` |
| **pane** | One rectangle of the viewport, with its own camera and display settings. Up to four. | `crates/solarxy-renderer/src/panes.rs` |
| **desk** | A named snapshot of the application arrangement, never of document state. | `web/src/store/desks.ts:1` |
| **arrangement** | The current shape of the app chrome; a desk is a saved one. The pair is well defined and should stay. | `web/src/store/desks.ts:129` |
| **still** | A single high-quality rendered image, produced by the tiled still job. Not "final render", not "export". | `crates/solarxy-host/src/still.rs` |
| **tile** | One region of a still, bounded by the tile pixel budget. | `TilePlan`, same file |
| **beauty** | The shaded colour image, as opposed to an auxiliary pass. | `PassKind::Beauty`, `crates/solarxy-host/src/passes.rs:63` |
| **AOV** | An auxiliary pass written beside the beauty: albedo, normal, depth. | `AovKind`, `crates/solarxy-host/src/passes.rs:29` |
| **backend** | An implementation of the render contract. Two exist, raster and traced. | `crates/solarxy-renderer/src/backend.rs` |
| **helper** | A world-scaled wireframe showing a light's real extent. | `crates/solarxy-renderer/src/helpers.rs` |
| **marker** | A screen-constant glyph saying only that a light is at a position. Distinct from a helper; the distinction is load-bearing and documented at the same file. | `crates/solarxy-renderer/src/helpers.rs` |
| **annotation** | One review note anchored in the scene, with optional replies. | `crates/solarxy-graph/src/review.rs` |
| **anchor** | Where an annotation is attached, with a world-space fallback. | same file |
| **mirror** | The frontend's read-only reflection of the Rust-owned document. | `web/src/store/mirror.ts` |
| **command** | The only way the frontend mutates the document. | `Command`, `crates/solarxy-graph/src/engine/mod.rs:69` |
| **hierarchy** | The bounding-volume structure used for ray queries. | `crates/solarxy-bvh/` |
| **arena** | The packed storage buffers the traced kernel binds. | `crates/solarxy-renderer/src/pathtrace/arena.rs` |
| **atlas** | The packed texture pages the traced kernel samples. | `crates/solarxy-renderer/src/pathtrace/atlas.rs` |
| **probe** | The harness that runs a ray corpus or a material pool through the real kernel bindings so a shader can be tested. | `crates/solarxy-renderer/src/pathtrace/probe.rs` |

### Where the codebase uses two words for one thing

These are real and should converge. None is urgent; each is a rename when the surrounding code
is next opened.

**Render engine, three names.** `RenderEngine { Raster, PathTraced }` at
`crates/solarxy-graph/src/nodes/export_nodes.rs:31`, `StillEngine { Raster, PathTraced }` at
`crates/solarxy-host/src/still.rs:199`, and `RenderEngineArg { Raster, PathTraced }` at
`crates/solarxy-cli/src/parser.rs:243`. Same two-member set, same default, three names, mapped
by hand at four sites. The duplication is structural rather than careless: `solarxy-host` is
forbidden a `solarxy-graph` dependency, so the shared still job cannot name the engine's enum.
The naming is not forced by that, though: all three could be `RenderEngine`. Note that the fan
out costs edits and not silent divergence, because every one of the maps is an exhaustive
two-arm match with no wildcard, so adding a third variant halts compilation everywhere.

**Hierarchy and BVH.** The frontend calls `buildHierarchyInWorker` at
`web/src/engine/session.ts:192`; the export it calls is `build_bvh_job` at
`crates/solarxy-web/src/app.rs:6282`. One operation, two words, one on each side of the wasm
boundary. "Hierarchy" is the better word for prose and "BVH" is the crate name; pick one for
identifiers.

**Subflow and network.** `crates/solarxy-graph/src/document/mod.rs` uses "subflow" 23 times and
"network" 16, for the same thing. The type is `GraphContext::Subflow`, so "subflow" wins.

**Pass, AOV and plane.** `PassKind` is the AOVs plus the beauty and `AovKind` is the auxiliary
passes alone, which is a real distinction correctly drawn at
`crates/solarxy-host/src/passes.rs`. "Plane" then appears as a third word for the same objects
inside the still job (`PendingPlane` at `crates/solarxy-host/src/still.rs:580`, `TilePlanes` at
:672). Three words, two concepts. The still job means "one readback of one pass", which is a
worthwhile fourth concept but should be named for what it is.

**`solarxy-render` and `solarxy-renderer`.** Two crates at different layers, one letter apart.
This is a naming defect that outlives every rename opportunity because renaming a crate is a
packaging change. It is recorded here so a reader who mistypes one for the other in a review
knows they are not confused.

## 3. File and function size

### 3.1 The caps

| Signal | Rust | TypeScript and TSX |
|---|---|---|
| Soft cap, triggers a decomposition conversation | about 400 lines | about 250 lines |
| Hard signal, presumed to be doing too much until argued otherwise | 800 lines | 500 lines |
| Function soft signal | about 60 lines | about 60 lines |
| Function hard signal | 100 lines | 100 lines |

Crossing a soft cap is not a failure. It is a prompt to say out loud, in the pull request, what
the second responsibility is and why it should stay. Sometimes the answer is that there is only
one responsibility and it is genuinely that large, which is the correct outcome for a table of
47 pipeline descriptors and the wrong one for a type with a dozen concerns.

### 3.2 The ratchet

**Rule.** No new violations. Existing violations burn down with a named owner. A file already
over a cap may be edited freely; it may not grow past its current size without the same
conversation a new violation would trigger.

**Why.** These are the measured counts today, from the shared facts:

| Measure | Count |
|---|---|
| Production Rust files at or over 400 lines | 123 |
| Production Rust files at or over 800 lines | 43 |
| Production Rust functions at or over 100 lines | 123 |
| Rust functions at or over 60 lines | 423 |
| Production TypeScript and TSX files at or over 250 lines | 31 |
| Production TypeScript and TSX files at or over 500 lines | 8 |

A cap stated as a gate would be violated 277 times on the day it lands, counting the file caps
and the hundred-line function signal. Nobody enforces a rule with 277 standing exceptions, and
a rule nobody enforces makes every other rule in this document weaker. So the caps apply to new
code and to files being substantially reworked, and the 277 are a backlog with named owners
rather than a compliance failure.

The mechanical form of this is a file-size report in CI that fails only on an increase, which
is proposed in [09-evolution-and-roadmap.md](09-evolution-and-roadmap.md) alongside the other
drift checks. Until that exists, the ratchet is a review convention, and a reviewer is entitled
to hold it.

### 3.3 Decompose by responsibility, never by line count

**Rule.** Splitting a 900-line file into two 450-line files at an arbitrary boundary makes it
worse: now the reader has to hold both open. The split is legitimate only when each half has a
name that describes what it owns.

The patterns, in the order to try them:

1. **Submodule per concept.** The file becomes a directory; each concept becomes a sibling with
   its own header saying what it owns and what it does not.
2. **Trait definition separated from implementations.** `crates/solarxy-renderer/src/backend.rs`
   declares the render contract and implements none of it; `RasterBackend` lives in
   `solarxy-host` beside the pass chain it wraps and `PathBackend` in
   `crates/solarxy-renderer/src/pathtrace/backend.rs`. That shape is what lets a third backend
   arrive without touching a shell.
3. **Pure transformation extracted from orchestration.** The transformation gets tests; the
   orchestration keeps the device handle. `crates/solarxy-renderer/src/pathtrace/scene.rs`
   contains no wgpu by design, which is why its ingestion logic carries 981 lines of in-file
   tests while the module that uploads carries 41.
4. **View, state and logic split in the frontend.** A component renders; a store holds; a
   module computes. `web/src/flow/layout.ts` is the pattern, and
   `web/src/components/StillRenderModal.tsx` at 676 lines with a single 599-line component is
   the counterexample.
5. **Tests moved out of the way.** A `mod tests` that is twice the size of the module it tests
   belongs in its own file or its own directory.

### 3.4 Concrete decomposition proposals

These are the current worst offenders, opened and read. Each proposal names modules and says
what moves. None of them is scheduled here; sequencing is
[09-evolution-and-roadmap.md](09-evolution-and-roadmap.md)'s job.

#### `crates/solarxy-web/src/app.rs`, 6,489 lines, 169 methods on one type, zero tests

The worst hotspot in the workspace, and the problem is not length. It is that at least twelve
responsibilities share one `&mut self` that owns a WebGPU device, so nothing in the file can be
constructed without one, which is exactly why the file has no tests and the whole crate has
eleven (in `camera_commit.rs` and `trace_settings.rs`, which were extracted and acquired tests
immediately). The methods are already spread across five `impl SolarxyApp` blocks at lines 913,
3940, 4208, 5882 and 5978, which is a decomposition the author started and stopped.

Proposed split, using the boundaries the file already has:

| New module | What moves | Lines today |
|---|---|---|
| `dto.rs` | The ~25 hand-written boundary structs and their `From` impls, currently in two separate runs | :288-410 and :6311-6490 |
| `still.rs` | The still-render job driver: `start_still_render`, `pump_still_render`, `take_still_tile`, `take_still_preview`, the pass and file accessors, EXR and PNG encoding | :1932-2409 |
| `capture.rs` | Screenshot and turntable: `request_screenshot`, `request_turntable_frame`, `poll_screenshot`, `render_screenshot`, `finish_capture`, `render_turntable_frame` | :2524-2900 |
| `jobs.rs` | The four worker pumps and their eight paired submit and error arms: import, validate, image, HDRI | :3357-3789 |
| `gizmo_drag.rs` | The whole drag lifecycle already isolated in its own impl block: `pane_ray`, `manipulator_at`, `update_gizmo_hover`, `begin_gizmo_drag`, `take_gizmo_drag`, `drag_state`, `update_gizmo_drag`, `commit_gizmo_drag`, `rollback_gizmo_drag` | :3940-4205 |
| `environment.rs` | Environment and IBL install: `sync_traced_environment`, `install_still_environment`, `install_traced_environment`, `apply_scene_environment`, `environment_json` | :4445-4612 |
| `attr_viz.rs` | Attribute visualisation: channel sync, label atlas rebuild, vector-line geometry, the aggregate builder | :4711-5020 |
| `player.rs` | The embedded asset-preview sub-app, already its own `impl` block plus a `PreviewState` struct: `preview_open`, `preview_orbit`, `preview_zoom`, `preview_resize`, `preview_close`, `preview_render_set`, `render_preview` | :5869-6106 |
| `workers.rs` | The four GPU-free worker exports, which are a second headless wasm instance's entire public API and have no business sharing a file with the GPU host | :6195-6300 |
| `scenefile.rs` | `.slxy` save and load plus the camera and view JSON marshalling | :3861-3942, :5759-5868 |

What is left in `app.rs` after that is the wasm-bindgen class, device and surface setup, the
frame pump, pointer routing, per-pane camera lifecycle and the pane render path: still large,
but one responsibility, the host.

The tractable wins are `player.rs`, `workers.rs`, `dto.rs` and `gizmo_drag.rs`, because each is
already a contiguous region behind a clean seam. `gizmo_drag.rs` is the one with the largest
test payoff: the drag solver it calls already has 801 lines of tests in
`crates/solarxy-host/src/gizmo.rs`, and the lifecycle wrapping it has none.

#### `crates/solarxy-graph/src/engine/mod.rs`, 4,201 lines

Genuinely the boundary vocabulary, so its size is less alarming than the wasm host's. `dispatch`
at 257 lines (`:1122`) is a match over a 35-variant enum and is fine as it stands. The engine
directory already has siblings (`attr_table.rs`, `snapshot.rs`, `scene.rs`, `scenefile.rs`,
`undo.rs`), so the pattern is established.

| New module | What moves | Lines today |
|---|---|---|
| `command.rs` | `Command` (35 variants), `CookMode`, `PortRefDto` and its `From` impl. The entire inbound vocabulary in one file the frontend's `types.ts` can be diffed against. | :52-271 |
| `event.rs` | `EngineEvent` (21 variants), `NodeReport`, `EventBatch`, `PickDetail`, `ReviewMarkerWorld`, `push_validation_events`. The outbound vocabulary and its lowering. | :272-506 |
| `transform.rs` | `GizmoTarget`, `NodeTransform`, `gizmo_frame`, `mat3_to_array`, `transform_params_for`. Manipulator semantics currently living inside the document engine, one layer from `solarxy_core::gizmo` which exists for exactly this. | :609-824 |

`Engine` itself, its 19 fields and its 109 methods, stay in `mod.rs`. That is still a large
type, and shrinking it is a design question rather than a file split, so it belongs in the
target architecture rather than here.

`crates/solarxy-graph/src/engine/tests.rs` at 9,295 lines is the largest file in the
repository and holds 205 tests. Its size is not a defect: it is why `solarxy-graph` is the
best-covered crate. But 205 tests in one flat file means locating the cross-context reference
tests versus the geometry sweep tests is a text search. Split it into
`engine/tests/{commands,cook,contexts,gizmo,review,boundary}.rs`. Purely mechanical, no
behaviour change.

#### `crates/solarxy-renderer/src/pipelines.rs`, 1,265 lines, one 983-line function

`Pipelines::new` at `pipelines.rs:233` builds all 47 pipelines in one body. Judged mildly: it is
a straight-line sequence of descriptor literals, tedious rather than tangled, and it has no
branching to get wrong.

The decomposition is already designed and simply not applied. The struct at `pipelines.rs:201`
is five sub-structs: `ScenePipelines` (14 fields), `PostProcessingPipelines` (10),
`OverlayPipelines` (16), `UvPipelines` (8), `InspectionPipelines` (2). Give each one a `new`
taking `(&Device, &SurfaceConfiguration, &BindGroupLayouts, sample_count)`, and `Pipelines::new`
becomes five calls. That turns one 983-line function into five of roughly 50 to 300 lines, each
named for what it builds, with no shared local state to thread because there is none: the
constructor's only locals are the format constants and the two vertex-buffer layout helpers at
`pipelines.rs:213` and `:224`, both already free functions.

While there, fix the stale header documented in 1.5.

#### `crates/solarxy-renderer/src/frame.rs`, 2,549 lines, zero tests

The `Renderer` impl opened at `frame.rs:504` carries 42 methods, and the file also holds seven
resource structs. This is the real one in the renderer, because unlike `pipelines.rs` it is
tangled rather than merely long: pass encoding sits beside resource ownership, an async
readback state machine, and three separate host-fed vertex-writing channels.

| New module | What moves | Anchor |
|---|---|---|
| `targets.rs` | `RenderTargets`, `PostProcessing` and its strength accessors, `resize_targets` | :102-151, :516 |
| `uv_overlap.rs` | `UvOverlapResources` and its whole async readback state machine, `request_readback` and `poll_readback`, plus the two UV passes | :181-309, :203, :244, :2253, :2293 |
| `validation_gpu.rs` | `ValidationColorResources`, `ObjectValidationGpu`, `draw_validation_overlay` | :310-337, :2162 |
| `furniture.rs` | The three host-fed channels that `clear_viewport_furniture` exists to reset together: camera helpers, light helpers, light markers, the manipulator, and the attribute label upload | :1398-1608, :1630-1683, :2504-2546 |
| `outline.rs` | The jump-flood selection outline: `render_selection_outline`, `composite_selection_outline`, `set_selection_highlight`, `draw_selection_tint` | :1684-1897 |
| `passes.rs` | The ten `render_*_pass` encoders and the `draw_*` helpers they call | :931-1397, :1898-2503 |

`clear_viewport_furniture` at `frame.rs:1588` is the argument for grouping the furniture
channels rather than scattering them: it exists because clearing three of four is the bug it
closes, and a file boundary around all four makes that invariant local.

#### `web/src/engine/session.ts`, 1,248 lines

58 top-level exports and fifteen pieces of module-level state: eleven `let` bindings (lines 64,
92, 99, 111, 402, 403, 405, 871, 872, 985, 1227) and four mutated module-level `Map`s (93, 100,
112, 1040). It has no test file, and the module-level mutable state is why: nothing can be
constructed twice in one test process.

| New module | What moves | Lines today |
|---|---|---|
| `workers.ts` | The import worker handle and the three token-and-waiter promise maps, `previewParseModel`, `buildHierarchyInWorker` | :64-400 |
| `boot.ts` | `client`, `booting`, `pendingRecovery`, `bootSession`, `getClient`, `isBooted`, `hasPendingRecovery`, `takeRecovery` | :402-520 |
| `view.ts` | Layout, split ratio, active pane, pane settings and look, display settings, camera commands, camera lock, jump and create, `refreshViewState`, `flyToIssue` | :662-780 |
| `annotations.ts` | The six review command wrappers | :782-829 |
| `persistence.ts` | `debounceTimer`, `lastSaveAt`, `autosaveDelayMs`, `buildSaveExtra`, `explicitSave`, `restoreDocument`, `openScene`, `openSampleScene` | :871-985 |
| `clipboard.ts` | `clipboard`, `copySelection`, `paste`, `duplicateSelection` | :985-1032 |
| `assets.ts` | `stagedAssetNames`, `stageFile`, `assetDisplayName`, `stagedManifestNames`, `refsWithMtlTextures`, `importDroppedFiles`, `completeModelImport` | :1040-1168 |

What remains is the frame pump and the dispatch wrappers, which is a coherent module.

The state should move into the modules that own it rather than staying module-level, and where
a piece is genuinely one-per-page (the client, the import worker) it should be behind an
accessor that a test can reset. `autosaveDelayMs` is already exported as a pure function,
which is why it is the one piece of this file's behaviour that is testable today; it is the
pattern for the rest.

## 4. Rust

### 4.1 Error handling by crate kind

**Rule.** Library crates define their own error enum with `thiserror`. Binary crates use
`anyhow` and add context with `.context(...)` where it helps a user diagnose the failure.
`anyhow` never appears as a normal dependency of a library crate.

**Today, and it holds.** Every library crate that can fail has a `thiserror` enum:
`FormatsError`, `GraphError`, `CookError`, `EngineError`, `KernelError`, `TransferError` (twice,
once in `solarxy-kernel` and once in `solarxy-bvh`), `ImagingError`, `SceneFileError`,
`RendererError`, `RenderError`, `ValidationRunError`, `ProjectConfigError`, `ReviewError`,
`ModelDocumentError`, `QuiescenceError`. Where `anyhow` appears in a library manifest it is a
dev-dependency for examples (`crates/solarxy-renderer/Cargo.toml:39`,
`crates/solarxy-host/Cargo.toml:40`) or optional behind a feature
(`crates/solarxy-core/Cargo.toml:43`, behind `serialization`). That is the rule being followed,
not an exception to it.

**Rule.** An error variant carries what a caller needs to act, not a formatted string. If the
only thing a caller can do with an error is print it, the variant is under-specified.

### 4.2 `unwrap` and `expect`

**Rule.** No `unwrap` or `expect` outside tests. Use `?`. Where a value is genuinely
infallible by construction, restructure so the compiler knows, or use `expect` with a message
that states the invariant rather than the symptom, and treat that as a reviewed exception.

**Why.** In the browser shell a panic takes the tab with no recovery path; in the desktop shell
the release profile sets `panic = "abort"`, so there is no unwinding either.

**Today.** `solarxy-web` and `solarxy-validate` have zero occurrences in `src`. The rest have
occurrences, but the great majority are inside `#[cfg(test)]` modules, which the rule permits
and which a raw grep cannot separate. The honest statement is that the rule is stated in the
working agreement, is followed in the crates where it matters most, and is not mechanically
enforced. `clippy::unwrap_used` and `clippy::expect_used` are the lints that would enforce it;
they are in neither the pedantic set nor any crate's configuration today. Turning them on is
part of the lint proposal in 4.8, and it requires the test modules to be exempted, which
`#[cfg_attr(test, allow(...))]` does at the module level.

### 4.3 Errors across the wasm boundary

**Rule.** A fallible export returns `Result<JsValue, JsError>`. The `JsError` carries a message
a user can act on, because it is the only thing that survives the crossing. A panic is never an
error-signalling mechanism at this boundary.

**Rule.** Boundary types are hand-authored on the TypeScript side and pinned on the Rust side,
which means every serde enum crossing the boundary carries both `rename_all = "camelCase"` and
`rename_all_fields = "camelCase"`.

**Why, and this is a real bug that shipped.** `#[serde(rename_all = "camelCase")]` on an enum
renames the variants and nothing else. `HostEvent` in `crates/solarxy-web/src/app.rs:212` had
only the first attribute. Every field in that enum was one word, so nothing was wrong, until
`renderProgress` gained `elapsed_ms` and `remaining_ms`. Those crossed in snake case while
`web/src/engine/types.ts` declared them camel, and the still dialog's elapsed and remaining
readouts read `undefined` for a full release. Neither side was wrong alone; only the pair was.

The guard is `the_wasm_boundary_enums_rename_their_fields_too` at
`crates/solarxy-core/tests/tokens_drift.rs:1071`, which scans the source text of both enums. It
is a source-level scan rather than a serialisation test because `HostEvent` lives behind
`cfg(target_arch = "wasm32")`, so no native test can construct one.

### 4.4 Ownership in the graph: identifiers, not references

**Rule.** The document graph is stored as identifiers into maps, never as references or
`Rc`/`Arc` links between nodes. `Graph` is `nodes: BTreeMap<NodeId, NodeData>` plus
`edges: BTreeMap<EdgeId, Edge>` at `crates/solarxy-graph/src/document/mod.rs:187`, and `NodeId`
is a newtype over `u64` at `:37`.

**Why.** Four reasons, all of which a reference graph gives up.

1. **Undo and redo work on values.** Removing a node and putting it back is inserting a
   `NodeData` under its original key. With references, restoring a node means restoring every
   inbound reference to it. The doc comment on `EdgeId` at `document/mod.rs:39` records the
   consequence that makes this concrete: variadic `port_order` lists hold edge ids, so undo
   must restore removed edges under their original ids or the port ordering silently changes.
2. **Serialisation is the same shape as memory.** A `.slxy` document is the same identifiers,
   so a load is not a pointer-fixup pass.
3. **The frontend can hold the same identity.** The mirror store keys on `NodeId`, so a
   selection survives an event batch without any object identity crossing the wasm boundary.
4. **No borrow-checker fight in the cook.** The cook driver mutates the slot map while walking
   the topology; with references that walk would borrow the graph immutably for its duration.

**Rule.** An identifier is a newtype, never a bare integer, and it is never reused after being
freed. Reusing an id is how a stale reference becomes a wrong reference rather than an absent
one.

### 4.5 Allocation discipline in the cook hot path

**Rule.** Geometry buffers are shared by `Arc` and never cloned to be passed along. A node that
does not modify a buffer passes the same `Arc`.

**Why, and this is load-bearing for the renderer as well as the cook.** The renderer decides
whether a mesh changed by comparing `Arc` pointers, not contents:
`same_geometry` at `crates/solarxy-renderer/src/scene_objects.rs:946` compares
`Arc::ptr_eq` on positions, indices, normals, texture coordinates, colours and placements, and
`Arc::ptr_eq` on every material. Its own doc comment states why it is shared with the traced
scene: "Two consumers deciding unchanged two ways would eventually disagree about whether a
re-seeded scatter needs rebuilding, and only one of them would be right."

The consequence is that a cook body which clones a buffer it did not modify does not merely
waste memory. It defeats the dedupe, so the renderer re-uploads geometry that did not change
and the traced scene rebuilds a hierarchy it already had. This is stated here so that nobody
"simplifies" an `Arc::clone` into a `Vec::clone` in the name of readability.

**Rule.** Placements are carried or baked, never dropped. A geometry port that does not declare
`carries_placements` receives baked input, resolved by `bake_uncarried_geometry` in
`crates/solarxy-graph/src/cook/driver.rs` before the cook body runs. This means a node needs no
instancing awareness at all, and the default is safe. Adding a node with a geometry input means
considering it in the registry-derived sweep tests in `crates/solarxy-graph/src/engine/tests.rs`,
which fail the build for a type that is neither swept nor exempted.

### 4.6 Unsafe

**Rule.** `unsafe` requires a `// SAFETY:` comment naming the specific invariant the caller
upholds, and it requires a reviewer who is not the author. If a safe crate provides the same
operation, use it and take the dependency conversation rather than the block.

**Today, three sites, and none meets the bar.** The workspace's only `unsafe` is at
`crates/solarxy-formats/src/export.rs:947`, `:951` and `:955`:

```rust
fn bytemuck_cast(v: &[[f32; 3]]) -> &[u8] {
    // Plain little-endian f32 triples; safe on every target we ship
    // (wasm32 and the desktop triples are little-endian).
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}
```

Two of the three carry no comment at all. The one that does discusses endianness, which is a
correctness question about the GLB payload rather than a soundness question about the cast; the
soundness question is alignment and padding, and it happens to be fine here because `[f32; 3]`
has alignment 4 and no padding, but the comment does not say so. There is no
`forbid(unsafe_code)` anywhere in the workspace.

The fix worth taking: the three helpers are named after `bytemuck`, which is a workspace
dependency of `solarxy-renderer` and `solarxy-bvh` but not of `solarxy-formats`. Adding it here
replaces all three with `bytemuck::cast_slice` and removes the `unsafe` entirely. Adding a
dependency is a gated decision under the working agreement, which is why this is a proposal and
not a change.

**Rule.** Every crate that contains no `unsafe` declares `#![forbid(unsafe_code)]`, so that
adding one is a deliberate edit to the crate root rather than an unremarked line.

### 4.7 API design

- **Module privacy is how a crate states its contract.** A `pub mod` is a promise; an internal
  module is `pub(crate)` or private, with a facade `pub use` block naming what is actually
  supported.

  **Today, this is not held.** `crates/solarxy-renderer/src/lib.rs` declares 41 public modules
  and zero private ones, giving 749 public items, the largest surface in the workspace and
  larger than the engine's 643 despite being a third smaller. `solarxy-kernel` is 19 public and
  1 private; `solarxy-graph` 17 and 1. Only `solarxy-web`, `solarxy-scenefile`, `solarxy-app`
  and `solarxy-validate` use module privacy to shape a surface. The consequence is that
  downstream crates reach into internals as a matter of course, so any refactor of an internal
  renderer module is a breaking change to four crates.

- **A constructor returning `Self` is `new`; one returning something else is named for what it
  returns.** `LoadedModel::load` at `crates/solarxy-renderer/src/scene.rs` is named that way for
  exactly this reason.

- **Free functions over borrowed parameters, not methods on a host type, for anything shared
  between shells.** `crates/solarxy-host/src/lib.rs:34` states the rule and the reason: each
  shell keeps its own state layout and builds the parameters from whatever it has.

- **Where one caller has a capability another lacks, the parameter is an `Option` whose `None`
  already means the right thing, not a flag the function branches on.** Same source. A desktop
  pane passes no selection and gets no highlight; the absent path emits the identical command
  stream it did before the code was shared, which is what makes an extraction provable rather
  than plausible.

- **Exhaustive destructuring is a deliberate compiler guard, and it is used here.** The three
  per-shell `trace_settings_for` functions each open with
  `let RenderSettings { ... } = *settings;` listing every field with no rest pattern, so adding
  a field to `RenderSettings` halts compilation in all three shells until each says what it
  does with it. That guard exists because a camera's aperture resolved correctly out of a
  document and reached no renderer for a whole release, with every test passing because they
  all used an aperture of zero. A test catches a value wired to the wrong field; only the
  compiler catches one wired nowhere.

### 4.8 Lints

**Today, honestly.** There is no `[lints]` table in the workspace `Cargo.toml` or in any member
manifest. Instead `#![warn(clippy::pedantic)]` plus a hand-maintained `#![allow(...)]` block is
copied into all 15 crate roots, in **12 distinct configurations**. Only four are identical
(`solarxy-core`, `solarxy-formats`, `solarxy-graph`, `solarxy-kernel`, 21 lints each).
`solarxy-renderer` allows 22; `solarxy-host` 20; `solarxy-app` 19; `solarxy-bvh` and
`solarxy-cli` 15 each with different contents; `solarxy-validate` 10; `solarxy-web` 9;
`solarxy-imaging` 6; the root binary 5; `solarxy-scenefile` 2; `solarxy-render` exactly one,
`module_name_repetitions`, making the newest crate the strictest in the workspace.

Five lints appear in exactly one crate each: `inline_always` and `needless_range_loop` in
`solarxy-bvh`, `doc_markdown` in `solarxy-validate`, `unused_self` in `solarxy-web`, plus four
in `solarxy-cli` only. Nothing records which of these are policy and which are accretion.

There is **no `#![deny]` and no `#![forbid]` anywhere** in the workspace.

Two consequences follow, and both have already cost something. Moving code between crates,
which this workspace does routinely, produces clippy failures unrelated to the change. And
`too_many_lines` and `too_many_arguments` are allowed 12 and 28 times respectively across the
workspace, at the site rather than centrally, which makes both a convention that is opted out
of per function rather than a signal. `render_ui` at
`crates/solarxy-app/src/gui/renderer.rs:418` is 533 lines with over twenty parameters under an
explicit allow at `:417`.

**CI is weaker than the documented local command.** `.github/workflows/ci.yml:22` runs
`cargo clippy --workspace --all-features -- -D warnings` with **no `--all-targets`**, so tests,
examples and benches are compiled by the later `cargo test` and never linted. That includes
14 example targets totalling about 5,350 lines, among them
`crates/solarxy-host/examples/golden.rs`, which is the golden-capture harness itself, and
`crates/solarxy-graph/examples/gen_samples.rs` at 1,673 lines. `crates/solarxy-cli/src/bin/solarxy-cli.rs`
is a separate crate root of 741 lines and carries no lint attributes at all, so `lib.rs`'s
pedantic warn does not reach it. `CLAUDE.md` documents the developer command as
`cargo clippy --all-targets`, so the local gate and the CI gate disagree in the direction that
matters.

**Proposed fix.** Three changes, in order of cost.

1. Add `--all-targets` to the CI clippy invocation. One word. Expect a burst of findings in the
   examples and test files that have never been linted; fix or allow them once.
2. Move the shared allow list into a workspace `[lints]` table in the root `Cargo.toml` and
   have every member declare `lints.workspace = true`. The table becomes the one place the
   policy is written, and a crate that genuinely needs a divergence declares it in its own
   manifest with a comment saying why. `solarxy-bvh`'s `inline_always` and `needless_range_loop`
   are the likely genuine cases, because its traversal is a line-for-line twin of a WGSL kernel
   and must stay term-for-term.
3. Decide what is `deny` rather than `warn`. The candidates, and the reason for each:

   | Lint | Level | Reason |
   |---|---|---|
   | `unsafe_code` | `forbid` per crate, except `solarxy-formats` | Three sites exist; everywhere else the answer is no. |
   | `clippy::unwrap_used`, `clippy::expect_used` | `deny`, with `#[cfg_attr(test, allow(...))]` | The working agreement already says this; nothing enforces it. |
   | `clippy::todo`, `clippy::unimplemented` | `deny` | Neither should reach a merged branch. |
   | `clippy::dbg_macro`, `clippy::print_stdout` in library crates | `deny` | A library writing to standard output breaks the CLI's `--json` mode. |
   | `clippy::pedantic` | `warn` | Keep as the broad net; the allow list is what tunes it. |

   `too_many_lines` and `too_many_arguments` stay `warn` and stay in the allow list, because the
   ratchet in section 3 is the mechanism for those and a per-site allow is exactly the escape
   hatch the ratchet needs.

**Also today.** `clippy.toml` is one line, `msrv = "1.92"`. There is no `rust-toolchain.toml`
anywhere, so the MSRV lives in four unlinked places: `Cargo.toml:24`, `clippy.toml:1`, the
`dtolnay/rust-toolchain@1.92` pins in `ci.yml` and `web-release.yml`, and, by omission, the
unpinned `rustup default stable` in `native-bundle.yml:63` that builds the artefacts users
install. Adding a `rust-toolchain.toml` makes local builds, CI and release agree by
construction. That belongs to [07-build-release-and-platforms.md](07-build-release-and-platforms.md)
rather than here, and is named because it is the reason a lint or codegen difference can reach a
user before it reaches a gate.

### 4.9 Feature-flag hygiene

**Rule.** Every declared feature is referenced by the crate that declares it. A feature that
gates nothing is a promise the crate does not keep.

**Today, one violation.** `crates/solarxy-render/Cargo.toml:15` declares `clap = ["dep:clap"]`
with an optional `clap` dependency, and a recursive search for the string `clap` across
`crates/solarxy-render/src/` returns nothing: no `cfg(feature = "clap")`, no `cfg_attr`, no
`use clap`, no derive. `crates/solarxy-cli/Cargo.toml:40` nonetheless enables it. An optional
dependency is compiled into the shipped CLI's graph for no effect. Programmatic comparison of
declared against referenced features across all 15 members shows this is the workspace's only
dead feature. Contrast `solarxy-validate`, whose identically named feature has six real gate
sites.

**Rule.** A feature is either used by a shipped configuration or removed. If it is kept for a
future caller, the manifest comment says which caller and the claim is checked, because
`crates/solarxy-cli/Cargo.toml` already contains a manifest comment about a feature's
distribution status and the mechanism that makes it true lives in a different block of the same
file.

**Rule.** A crate that must compile for `wasm32` takes its intra-workspace dependencies with
`default-features = false`. This is currently held by `solarxy-graph`, `solarxy-renderer`,
`solarxy-host` and `solarxy-web` on both `solarxy-core` and `solarxy-formats`, and a single
forgotten one would pull `dirs`, `toml` and the path-based loaders into the browser build. The
only thing that exercises the reduced set is the separate
`cargo clippy -p solarxy-web --target wasm32-unknown-unknown` step; a workspace-wide
`--all-features` build unifies features under the version-2 resolver and would not catch it.

## 5. GPU and WGSL

### 5.1 Shader module organisation and naming

**Rule.** A shader file is named for the pass it implements, and a pass is named for what it
produces. `shadow.wgsl`, `gbuffer.wgsl`, `bloom.wgsl`, `composite.wgsl`, `uv_overlap.wgsl`. A
file with no entry point is a fragment and lives in the directory of the kernel that composes
it.

**Today.** 45 WGSL files, about 8,500 lines. WGSL has no include mechanism, so the path tracer
composes its kernels from fragments with `concat!` in
`crates/solarxy-renderer/src/pathtrace/mod.rs:106` and `:129` and in
`crates/solarxy-renderer/src/pathtrace/probe.rs`. That is what lets one traversal text be
shared by every kernel that walks the scene and by the test that pins it to its CPU twin.

**Rule.** Every fragment is named by a composition recipe, and a fragment that no recipe names
fails the build. Enforced by `crates/solarxy-renderer/tests/pathtrace_shader_source.rs`, which
also enforces the uniformity invariant on `traverse.wgsl` textually: level-sampled reads only,
no derivative-dependent call, no barrier. Textually rather than by comment because the browser
rejects the alternative at pipeline creation with a message that reads like a type error.

### 5.2 Bind group and layout conventions

**Rule.** `crates/solarxy-renderer/src/bind_groups.rs` is the single source of truth for every
bind group layout. A pipeline never builds its own. All uniform entries use
`min_binding_size: None`, so growing a uniform is layout-invisible.

**Rule on group index, and here is the honest state.** The path tracer has a convention and
holds it: group 0 is the scene (traversal storage buffers), group 1 the target and per-dispatch
parameters, group 2 the texture atlas, group 3 the environment and camera. Verified across
`crates/solarxy-renderer/src/shaders/pathtrace/`: `traverse.wgsl` and `atlas_probe.wgsl` use
group 0, `trace.wgsl` / `path.wgsl` / `depth.wgsl` group 1, `atlas.wgsl` group 2,
`camera.wgsl` and `environment.wgsl` group 3.

The raster path has no such convention. Group 0 is the material texture set in `shader.wgsl:146`,
the camera in `grid.wgsl`, `gizmo.wgsl` and `normals.wgsl`, the gradient colours in
`background.wgsl`, the scene and bloom textures in `composite.wgsl`, and the depth, normal and
noise inputs in `ssao.wgsl`. The camera is group 1 in `shader.wgsl:18` and group 0 in
`grid.wgsl`. Each pipeline assigns groups locally and correctly, and nothing is broken, but a
reader cannot carry an expectation from one shader to the next and a shared bind group cannot
be bound at a fixed index across passes.

**Proposed convention, for new passes and for any pass being reworked**: group 0 is per-frame
and per-view data that does not change within a pane (camera, lights, environment); group 1 is
per-pass parameters; group 2 is per-material or per-draw resources; group 3 is anything
pass-specific beyond those. Converging the existing raster shaders on it is a mechanical but
wide change and is not proposed here; the rule is that a new pass follows it and a reworked one
moves toward it.

**Rule on the storage-buffer budget, and it is spent.** The path tracer's four layouts are
declared in `bind_groups.rs` like every other layout but as their own `PathtraceLayouts`, built
only when a tracer exists. The scene group binds six compute-stage storage buffers, which core
WebGPU allows at eight per stage and `Limits::downlevel_defaults()` allows at four. Folding
them into `BindGroupLayouts::new` would impose the tracer's limit floor on every consumer of
the registry and break the headless smoke suite. The binding numbers are a budget rather than a
convention: seven in the scene group plus the transparent matte's coverage count at target
binding 4 spends all eight. A ninth logical array now means moving coverage into a
primary-only replay kernel with its own read-write texture, which is the recorded fallback.

### 5.3 Uniform layout

**Rule.** CPU uniform structs are `#[repr(C)]` with explicit padding fields chosen to hit
WGSL's 16-byte struct alignment, and each carries a
`const _: () = assert!(std::mem::size_of::<T>() == N);`. When extending a uniform, repack the
padding or update the assert in lockstep with the shader.

**Rule.** A WGSL struct may declare a **prefix** of the CPU struct and omit trailing fields it
does not read, because wgpu enforces size at the binding rather than shape. A field missing from
the **middle** is a defect. This carve-out is deliberate: a field can be added to
`CameraUniform` and only `shader.wgsl` updated, because the other shaders that read only
`material_override` keep working.

**Rule.** A uniform a shader declares whole belongs in the table in
`crates/solarxy-renderer/tests/uniform_layout.rs`, which computes the naga span of a named WGSL
struct and compares it to `size_of` on the Rust side. That comparison is the one nothing else
in the build makes, and the failure it catches is silent: WGSL aligns `vec3<f32>` to 16 bytes
in the uniform address space while Rust aligns `[f32; 3]` to 4, so a mispaired colour leaves the
Rust size assert passing, the shader compiling, and the viewport black at draw time. A prefix
declaration is legitimately smaller and does not belong in the table.

### 5.4 Resource lifetime, ownership and reuse

**Rule.** Long-lived GPU resources are created once and reused. **Nothing that outlives a frame
is allocated per frame.** Pipelines are built at startup; bind group layouts once; render
targets on resize; buffers grow with headroom rather than being reallocated to fit.

**Why.** Two mechanisms in the codebase depend on this and both break quietly if it is
violated. The `Arc`-identity dedupe in `scene_objects.rs` (see 4.5) decides "unchanged" by
pointer; a per-frame reallocation makes every object look changed. And the traced scene's
hierarchy cache keys on the `Arc::as_ptr` pair that `same_geometry` compares, holding **strong
clones of both buffers**, because an address only identifies an allocation while that
allocation is alive. Its sweep therefore runs once at the end of a batch and never inside the
remove arm, or an address freed at one operation is recyclable by the next operation in the
same list.

**Rule.** Growable buffers follow one headroom policy. `scene_objects` and the tracer's six
storage buffers both use 1.5x growth, and the tracer rebuilds its bind group only when a buffer
was actually reallocated.

**Rule.** A test-only harness does not live in the shipped library without a feature gate.
**Today this is violated**: `crates/solarxy-renderer/src/pathtrace/probe.rs` is 1,391 lines with
no `cfg(test)` gate and no tests of its own. Its purpose is sound, since running a ray corpus
through the real kernel bindings is the only way a shader gets unit tested, but the placement
means the apparatus compiles into every desktop binary and into the wasm the browser downloads,
and nothing measures what it costs. The `pt-probe` feature on `solarxy-web` exists for exactly
this kind of gating.

### 5.5 Shader variants and the permutation ceiling

**Rule.** Variation is a uniform branch first, a pipeline permutation second, and a
preprocessor never.

**Today.** This is what the codebase does. `shader.wgsl` branches on
`camera.material_override` and `material.shading_model` inside `fs_main` rather than compiling
five clay and chrome variants. The 47 pipelines in `Pipelines` are mostly distinct passes, not
permutations; the real permutation axes are narrow and each has a stated reason: `main` versus
`main_colored` (a vertex buffer at slot 2 when a colour lane is present), `alpha_blend` versus
`alpha_blend_colored`, `line` versus `line_colored`, `attr_labels` versus
`attr_labels_occluded` (depth test only), `manipulator_lines` versus `manipulator_tris`
(topology only). Nothing multiplies.

**Rule, the ceiling.** A new pipeline field is a decision, not a default. If a variant axis
would multiply against an existing one, the answer is a uniform branch. The reference number to
argue against is 47.

**Rule.** A pipeline that only some sessions need is built lazily.
`PostProcessingPipelines::float_composite` at `crates/solarxy-renderer/src/pipelines.rs:1226` is
the pattern: the layout and shader module are held (both refcounted handles, so holding them is
free) and the pipeline is built the first time a float still asks for it, so a session that
never renders one never pays.

### 5.6 Capability and platform differences

**Rule.** Optional GPU features are not required. Every device request in the workspace passes
`wgpu::Features::empty()`, verified at `crates/solarxy-app/src/state/init.rs:46`,
`crates/solarxy-render/src/lib.rs:1043`, `crates/solarxy-web/src/app.rs:949` and three sites in
`crates/solarxy-web/src/pathtrace_probe.rs`. That is the posture, and it is what makes one
renderer run on a browser and on a desktop adapter without a capability matrix.

**Rule.** Limits are raised off the adapter, never lowered from the baseline, and the raise
names the fields it touches. `solarxy_renderer::limits::required_limits` raises exactly two,
`max_buffer_size` and `max_storage_buffer_binding_size`, and its header states why it does not
use `Limits::or_better_values_from`: that helper raises the count limits too, and the tracer's
scene bind group already spends core WebGPU's eight-storage-buffer budget, so raising a count
limit would let a later change quietly exceed what the target platform guarantees and fail only
on the machines that guarantee least.

**Today, one inconsistency.** `crates/solarxy-cli/src/render_watch/mod.rs:212` and `:1094` use
`wgpu::Limits::default()` rather than the helper. That surface therefore cannot allocate a
buffer the other three shells can, and the difference is undocumented.

**Rule.** A capability difference is declared and has a defined fallback, never assumed away.
`BackendCaps` in `crates/solarxy-renderer/src/backend.rs` states capability and never identity,
which is the rule that lets a third backend slot in without touching a shell, and
`PassSelector` in `crates/solarxy-host/src/passes.rs` is keyed on `writes_aovs` rather than on
which backend drew the image, for the same reason.

### 5.7 Rendering correctness standards

These are rules, and a change that breaks one is a defect rather than a difference of opinion.
Their contracts live in [06b-rendering-and-shading.md](06b-rendering-and-shading.md); what is
here is the reviewable form.

1. **Colour space is stated, never inferred.** Every texture, target and conversion says which
   space it is in at the point it is created or sampled. A function that takes "a colour"
   without saying whether it is scene-referred or display-referred is under-specified. The known
   live instance: the tracer's atlas is `Rgba8Unorm` and filters in encoded sRGB before decoding
   the blended result, while the raster path uses sRGB texture views so the hardware decodes
   each texel before the bilinear blend.

2. **An AOV derives from the beauty evaluation. A second code path producing one is a defect.**
   An albedo that is computed by a different expression than the one the beauty used is not an
   albedo of that image.

3. **A shading model parameter has one definition, shared by the node graph, the UI, the
   serialised form and both shaders.** Per
   [ADR 0013](adr/0013-path-tracer-is-the-shading-ground-truth.md) the path tracer is the ground
   truth and a raster divergence is either a listed and justified real-time approximation or a
   defect filed against the rasterizer. `thickness` currently has two incompatible physical
   meanings, a Beer-Lambert path length in `shader.wgsl:911` and a thin-film boolean in
   `bsdf.wgsl:665`, from one parameter with one help string. That is the shape of the defect
   this rule prevents.

4. **Pass ordering constraints are documented at the pass, and the documentation distinguishes
   correctness from preference.** "The floor pass must follow the main pass because it reads the
   depth buffer" is a correctness constraint. "Grid before normals so normals draw on top" is a
   preference. A reader reordering passes needs to know which they are looking at.

5. **A visual change carries a golden-image comparison, or a stated reason none applies.** The
   golden job captures at the merge base and at the branch head on one runner and compares at
   zero tolerance, so hardware cancels out. An intentional rebaseline is declared by
   `[golden-accept]` in the pull request title, and the pull request body justifies the diff. A
   change with no pixel effect says so.

6. **Capability use is declared with a defined fallback.** See 5.6.

7. **Per-pass GPU timing is instrumented where the platform supports it, and measured before
   optimisation.** **Today this does not exist.** Every `RenderPassDescriptor` in the workspace
   passes `timestamp_writes: None`, and there is no `QuerySet` anywhere. Since every device
   request passes `Features::empty()`, `TIMESTAMP_QUERY` is not requested and could not be used
   without changing that posture. So no pass in this renderer has ever been measured on the GPU,
   and every performance claim about the render path is inferred from wall-clock frame time. The
   rule stands as the target; the honest current state is that a timing harness has to be built
   before it can be met, and the capability posture has to gain a "requested if available" arm
   to build it.

## 6. TypeScript and React

### 6.1 Strictness and the `any` policy

**Rule.** `strict` stays on, with `noUnusedLocals`, `noUnusedParameters`,
`noFallthroughCasesInSwitch` and `isolatedModules`. No `any` in hand-written code. No
`@ts-ignore` and no `@ts-expect-error`. Non-null assertions only where the invariant is
genuinely outside the type system, with a comment naming it.

**Today, and this is a standard to hold rather than a problem to fix.** Measured across
`web/src`: 90 occurrences of `any`, of which 64 are in the generated
`wasm/pkg/solarxy_web.d.ts`, which is build output and gitignored. That leaves about **26
hand-written**, in 34,000 lines. **Zero** `@ts-ignore` or `@ts-expect-error`. **Ten** non-null
assertions. For a codebase this size that is unusually disciplined, and the rule is to keep it
there rather than to drive it to zero, because the last few `any` are typically at a genuine
boundary where a narrower type would be a lie.

`tsconfig.json` is confirmed to carry every flag above.

### 6.2 Discriminated unions for anything crossing the wasm boundary

**Rule.** A type that crosses the boundary is a discriminated union on a `type` field, matching
the Rust serde tag, with every field in camelCase.

**Today, held.** `web/src/engine/types.ts:320` declares `Command` as a union tagged on `type`
with tags `"addNode"`, `"removeNodes"`, `"connect"`, and so on, matching
`crates/solarxy-graph/src/engine/mod.rs:69`. `ParamValue` at `types.ts:19` is tagged the same
way. `GraphContext` is `"root" | { subflow: NodeId }`, matching the Rust enum's serde shape.

### 6.3 Keeping boundary types in sync with Rust

**Today.** `web/src/engine/types.ts` is 931 lines and hand-authored. Its own header says so:

```ts
// Hand-authored TypeScript mirror of the frozen Rust serde boundary shapes
// (solarxy-graph). These are the wasm boundary contract; they are pinned on
// the Rust side by `command_boundary_json_shape_is_camelcase` and exercised
// live. A generated .d.ts via tsify is a documented follow-up; until then
// keep this file in lockstep with the Rust `Command`/`EngineEvent`/snapshot
// definitions (all camelCase).
```

**What that pin actually covers.** Three tests serialise a boundary shape and assert on it:
`command_boundary_json_shape_is_camelcase` (`crates/solarxy-graph/src/engine/tests.rs:383`),
`review_command_boundary_shape_is_camelcase` (`:1897`) and
`gizmo_command_boundary_json_shape_is_camelcase` (`:4210`). Between them they exercise **six**
`Command` variants of 35: `AddNode`, `ResetParams`, `AddAnnotation`, `ReanchorAnnotation`,
`CancelTransaction`, `EnsureTransformTarget`. Plus one event shape, `nodeAdded`, of 21.

**Rule, stated plainly.** Hand-authoring should not continue at this coverage. Six of 35 pinned
means a variant added on either side and forgotten on the other is caught by nothing until a
user hits it, which is exactly what happened with the still dialog's readouts. Two acceptable
resolutions, and the choice is a real decision:

- **Generate.** `tsify` produces the `.d.ts` from the Rust types, and the hand-written file
  becomes a thin re-export of the generated one plus any frontend-only types. This is what the
  header already names as the follow-up. It costs a build-order dependency the web build
  already has, since `web/src/wasm/pkg/` must exist before `tsc` runs.
- **Pin exhaustively.** Keep hand-authoring but assert every variant, by driving the pin from a
  list the compiler forces to be complete rather than from hand-written cases.

Doing neither is the current state and is the position this rule exists to close.

### 6.4 Component boundaries and composition

**Rule.** A component renders one thing. It reads from stores and dispatches commands; it does
not compute derived state that another component also needs, which belongs in a store selector
or a pure module.

**Rule.** Registry-driven components stay pure interpreters of the registry snapshot. The
palette, the typed handles and the parameter panel interpret; they do not special-case a node
type. A node added in Rust requires zero frontend changes, and
`web/src/registry/extensibility.test.ts` guards it. The one deliberate exception is the note
node, keyed on `typeId === "note"`, and it is the exception precisely because it is named as
one.

**Today.** Eight production TypeScript and TSX files are at or over 500 lines, and the largest
components are the ones with no tests: `ParameterPanel.tsx` at 797,
`preferences/PreferencesModal.tsx` at 748, `PaneToolbar.tsx` at 708, `StillRenderModal.tsx` at
676 as a single 599-line component. The ratchet in section 3 applies.

### 6.5 State ownership and colocation

**Rule.** Rust owns document state; the frontend mirrors it. The frontend owns only view and
chrome state. A piece of state lives in the narrowest scope that needs it: component state
first, a store only when two components need it.

**Rule.** Nine stores exist and nine is the number. Adding a tenth is a design conversation, not
a convenience. Only `mirror` reflects Rust document state; `viewState` reflects host-owned view
state; the other seven (`ui`, `prefs`, `desks`, `review`, `toasts`, `radial`, `renderJob`) are
frontend-owned.

**Rule.** Desync is detected and recovered, never assumed away. `web/src/store/mirror.ts:259`
treats a revision gap of more than one as desync and re-snapshots. Any new mirrored surface
carries the same discipline.

### 6.6 Memoisation follows measurement

**Rule.** `useMemo`, `useCallback` and `memo` are added when a measurement shows they are
needed, not pre-emptively. Each one is a correctness risk (a stale dependency array) traded for
a performance gain that may be zero.

**Today, and it is held.** 13 files under `components/` and `flow/` use any memoisation at all,
and the heaviest user has four `useMemo` calls. That restraint is the standard.

**The one place per-frame work is deliberately kept out of React** is marker positioning:
`web/src/engine/markers.ts` is an imperative registry that positions DOM pins in pane-relative
CSS pixels each frame with no React re-render. That is the correct shape for per-frame work and
the pattern to follow rather than memoising a component into submission.

### 6.7 Side-effect discipline

**Rule.** An effect has a stated cleanup or it does not need one, and the difference is
visible. Subscriptions, timers, workers and object URLs are released.

**Rule.** Worker communication goes through a token-and-waiter map, never through a bare
`onmessage` handler shared by two job kinds. `web/src/engine/session.ts` has three such maps for
three job kinds and gives each its own token space (starting at -1, -1,000,000 and -2,000,000),
which is what keeps them from colliding. Note the related hazard on the Rust side: the two
binary transfer codecs that cross the same worker boundary,
`crates/solarxy-kernel/src/transfer.rs` and `crates/solarxy-bvh/src/transfer.rs`, both export
`pack` and `unpack` over a little-endian blob, and only the BVH one carries a magic word and a
version. A blob handed to the wrong `unpack` is silently misinterpreted rather than rejected.
**Rule: a hand-written binary framing carries a magic word and a version.**

### 6.8 Accessibility baseline

**Rule.** Every interactive element is reachable and operable by keyboard. Focus is visible.
Any element with a role has the state that role implies. Colour is never the only carrier of
meaning; the review category glyphs at
`crates/solarxy-app/src/gui/review_visuals.rs` are the pattern, where each category has a letter
as well as a colour.

**Rule.** Motion respects `prefers-reduced-motion`. `web/src/styles/tokens.css:46` has the
media block; new animation goes through the motion tokens rather than around them.

**Today.** About 130 `aria-*` attributes and `role=` usages across `components/`, `flow/` and
`dock/`. That is a foundation rather than a completed baseline, and no automated accessibility
check runs anywhere.

### 6.9 Responsive behaviour

**Rule.** The public pages (landing, roadmap, references) work at phone, tablet and desktop
widths. The editor targets desktop and tablet; it is not expected to be usable on a phone, and
that expectation should be stated on the page rather than discovered.

**Today, one inconsistency worth fixing.** Breakpoint units differ by stylesheet:
`web/src/styles.css` uses `rem` (44rem, 60rem), `landing/landing.css` uses pixels (640, 720),
and `roadmap/roadmap.css` uses four pixel breakpoints (460, 760, 820, 860). Three files, two
unit systems, six distinct thresholds. Pick one unit and a small named set of thresholds, and
put them in the token file.

### 6.10 Styling

**Rule.** Colour comes from tokens. Tokens come from
`crates/solarxy-core/src/theme.rs`, which also feeds the desktop GUI and the terminal UI.

**Rule, and it is stated in the file itself.** `web/src/styles/tokens.generated.css` is
generated and **must not be hand-edited**:

```css
/* GENERATED by `cargo run -p solarxy-core --example gen_tokens`. Do not edit.
 *
 * The source of truth is `crates/solarxy-core/src/theme.rs`, which also
 * feeds the egui desktop GUI and the analyze TUI. Edit the palette there
 * and regenerate; `tests/tokens_drift.rs` fails CI otherwise.
 */
```

Changing a colour means editing the palette in `solarxy-core` and regenerating. The drift test
in `crates/solarxy-core/tests/tokens_drift.rs` fails CI on an out-of-date generated file, which
is the enforcement.

`web/src/styles/tokens.css` is the hand-authored half (fonts, motion, spacing) and imports the
generated one. New tokens go there or in the palette, never inline.

**Today.** `web/src/styles.css` is 5,556 lines, which is the largest file under `web/src`
including data files, and it is not in the TypeScript counts in section 3 because it is CSS. It
is a candidate for a per-surface split on the same ratchet terms.

**Today, no linter.** `web/` has no ESLint or Prettier configuration and no lint dependencies:
`package.json` devDependencies are the type packages, the React plugin, TypeScript, Vite and
Vitest. 34,000 lines of TypeScript are gated by `tsc --noEmit` alone. There is also no
import-boundary lint, which is why the one frontend layering rule that exists,
`the_player_does_not_import_the_editors_ui_graph`, is implemented as a Rust test in
`solarxy-core` reaching into the TypeScript tree, and a frontend developer running `npm test`
sees none of it.

## 7. Testing

### 7.1 What each layer owes

| Layer | Owes |
|---|---|
| Pure core logic (`solarxy-core`, `solarxy-kernel`, `solarxy-bvh`, `solarxy-imaging`) | Unit tests in-file. No device, no filesystem, no fixture that can be absent. |
| Engine (`solarxy-graph`) | Integration tests per behaviour: command, cook, undo, context, reference. Registry-derived sweeps so a new node type cannot skip the question. |
| Persistence (`solarxy-scenefile`, and the engine's mapping onto it) | Round-trip tests, and a migration test per schema version the reader still accepts. |
| Cook results | Deterministic assertions on point and primitive counts, attribute lanes and topology. |
| Renderer | Golden-image comparison for the raster path. Analytic and parity tests for the tracer: the WGSL kernel against its CPU twin, the estimator against a furnace. |
| Boundary types | A serialised-shape assertion per variant, on both sides. See 6.3 for the current shortfall. |
| Frontend logic | Pure-module tests: stores, layout, keymap, redaction, transport. |
| Frontend components | Nothing today, and that is a gap, not a policy. See 7.3. |
| Cook and frame timing | Benchmarks. None exist. See 8. |

### 7.2 What must be true before a change is mergeable

- `cargo fmt --all --check` clean.
- `cargo clippy --workspace --all-features` clean at `-D warnings`, and, once 4.8's first change
  lands, `--all-targets` too.
- `cargo test --workspace --all-features` green. Note that a bare `cargo test` at the repository
  root runs the root binary alone and reports nothing useful; the workspace form is the real
  suite.
- On a renderer change: the golden job green, or `[golden-accept]` in the pull request title
  with the diff justified in the body.
- On a change under `web/`: `npm run typecheck` and `npm test` green.

### 7.3 What is deliberately not tested, and what is a gap

**Deliberately not tested.** Wgpu resource construction is exercised indirectly by the golden
gate and the headless smoke suite rather than by unit tests, because constructing a device per
test is slower than the coverage is worth. Windowed event loops are not driven headlessly. The
manual QA checklists at `docs/qa/desktop-checklist.md` and `docs/qa/render-checklist.md` are
the declared substitute for the half the automated suite cannot see, and they are a real gate
rather than an aspiration.

**Gaps, named.**

- **`crates/solarxy-web/src/app.rs` has zero tests**, and the crate around it has eleven, all in
  `camera_commit.rs` and `trace_settings.rs`. Untested by consequence: the still-render pump and
  its image encoding, the gizmo drag lifecycle including rollback, all four worker pumps and
  their eight submit and error arms, the `.slxy` save and load path, the view-state round trip,
  and screenshot capture. The cause is structural, and section 3.4's split is the fix: the two
  modules that were extracted acquired tests immediately.
- **`crates/solarxy-app` has 92 tests and none in its core.** `state/input/mod.rs` (1,312 lines,
  including a 351-line `handle_key`), `state/render.rs` and `gui/renderer.rs` (1,090 lines)
  have none.
- **No component-render tests in the frontend.** There is no jsdom and no testing library in
  `web/package.json`, and no `vitest.config.*` anywhere, so Vitest runs on defaults in a node
  environment. That is the mechanical reason a component test cannot exist, and it is a smaller
  fix than it looks: a `vitest.config.ts` with `environment: "jsdom"` plus one dependency.
- **No property-based testing and no fuzzing**, anywhere. `solarxy-formats` parses OBJ, STL, PLY
  and glTF from attacker-supplied bytes in the browser import worker and is covered only by
  fixture-driven examples. `parse_ply` alone is 251 lines of hand-rolled parsing.
- **The golden gate covers one shell and one backend.** Six pane modes times two models, raster
  only, desktop only. The path tracer, the tiled still job and every pixel the web shell
  produces have no image regression gate; the stated substitute is a manual checklist.
- **A fixture-dependent performance test lives in library source.**
  `crates/solarxy-core/src/raycast.rs:861` calls `solarxy_formats::obj::load_obj` inside a
  `#[cfg(test)]` block, which is the sole reason for the `solarxy-core` to `solarxy-formats`
  dev-dependency and its package-level cycle. It returns early with a print if the model is
  absent, so on any checkout without the fixture it reports success while measuring nothing.
  It belongs in `crates/solarxy-core/tests/`, or outside the default test set entirely.

**Rule.** A test that can silently no-op is not a gate. Either it fails when its input is
missing, or it is marked `#[ignore]` and named as a measurement.

### 7.4 Reading a red graphics job

The `GPU tests (macOS)` job runs against a virtual machine with no dedicated graphics device, and
between 2026-08 and 2026-09-07 it failed 13 of 25 runs while every other job in those runs passed.
The failing tests were different every time, and the same files run thirty times on a machine with
a real adapter produced zero failures. So a red result from that job has historically been more
likely to be the runner than the change, and it was read as a real failure by nobody, which is the
worse outcome: a gate that is red half the time teaches a reader to merge through red.

Its tests are serialised as of 2026-09-07 on the hypothesis that concurrent graphics work on one
device is the cause. Until the rate over subsequent runs says otherwise:

- **Re-run once.** A failure that does not recur on the same tests is the runner.
- **A failure that recurs on the same tests is real**, and is investigated as a regression rather
  than re-run again.
- **Never widen a tolerance to make this job green.** The tests are deterministic on real
  hardware, which is the evidence that a wide tolerance would be hiding a runner problem behind a
  weaker assertion.
- **The golden job is separate and is not affected by any of this.** It compares the same two
  models on the same runner at the merge base and at the change, so hardware cancels out. A red
  golden job means the render moved.
- On a boundary change: the shape assertion updated on both sides in the same change.
- On a schema change: the migration path and its test, in the same change.

## 8. Performance

### 8.1 The numbers that exist

**Enforced in CI, at tag time only.** From `.github/workflows/web-release.yml`: wasm gzipped
2,621,440 bytes (2.5 MiB, line 39); editor boot JavaScript gzipped 471,040 bytes (460 KiB, line
57); player JavaScript gzipped 51,200 bytes (50 KiB, line 46). These run in `web-release.yml`,
which is invoked from the release workflow's post-announce jobs, so **they gate nothing on a
pull request**. `ci.yml`'s web job builds the bundle and asserts nothing about its size. Moving
the three checks into that job costs one step, since the bundle is already built there.

**Enforced in code, as constants rather than against a clock.**

| Budget | Value | Where |
|---|---|---|
| Cook budget, browser | 6 ms | `crates/solarxy-web/src/app.rs:74` |
| Cook budget, desktop | 8 ms | `crates/solarxy-app/src/state/update.rs:21` |
| Screenshot and capture | 4.0 megapixels | `crates/solarxy-web/src/app.rs` |
| Still tile | 4,194,304 pixels | `crates/solarxy-host/src/still.rs` |
| Reference chain depth | 32 | `crates/solarxy-graph/src/refs.rs:31` |

The two cook budgets are the same concept with two numbers and nothing recording why they
differ. Either the difference is measured per-shell headroom, in which case the reason belongs
beside both constants, or it is drift.

### 8.2 What does not exist

No frame-time budget. No target frame rate. No cook latency objective. No scene-scale limit. No
memory ceiling. No startup-time budget. No historical record of any performance number over
time. No benchmark framework of any kind: no `criterion`, no `divan`, no `#[bench]`, no
`benches/` directory in any crate. The three files that read as benchmarks
(`crates/solarxy-bvh/tests/build_perf.rs`,
`crates/solarxy-renderer/tests/pathtrace_perf.rs`,
`crates/solarxy-renderer/tests/pathtrace_denoise.rs`) are all `#[ignore]`d and describe
themselves as measurements rather than regression gates. And, per 5.7, no GPU timing
instrumentation at all.

So: **no performance number in this repository is defended by anything.** That is the honest
statement, and it is the reason the rule below is written the way it is.

### 8.3 The rules

**Rule.** Optimisation follows measurement. A pull request claiming a performance improvement
carries the before and after numbers and says on what hardware. In the absence of a benchmark
harness, that means a recorded measurement in the pull request body, which is weaker than a gate
and better than an assertion.

**Rule.** A budget is stated as a number in one place, with a comment saying what it protects
and what happens when it is exceeded. A budget duplicated across two shells with two values is a
defect until one of them carries the reason.

**Rule.** Where a platform has a hard failure mode, the ceiling is a refusal with a message, not
a crash. `crates/solarxy-renderer/src/limits.rs` states the pattern: `required_limits` raises
what it can, and `buffer_ceiling` refuses what still will not fit, so a mesh that is too large
produces a message rather than a dead viewport. The browser's separate float-still pixel ceiling
exists because wasm is a 32-bit address space where an allocation failure takes the tab, and it
deliberately does not apply to the desktop, which renders such a still comfortably.

### 8.4 Invariants that exist to protect performance

These read as things a tidy-up would remove. They must not be removed.

1. **The `Arc`-identity dedupe in scene ingestion.** `same_geometry` at
   `crates/solarxy-renderer/src/scene_objects.rs:946` compares pointers rather than contents,
   because the engine's cook cache shares those `Arc`s across frames so pointer equality means
   content equality. Replacing it with a content comparison would be correct and would cost a
   full buffer walk per object per frame. Replacing it with a coarser check would re-upload
   unchanged geometry.

2. **Its placement test.** The same function compares `instances` by `Arc` identity with a
   comment explaining why: a re-seeded scatter has the same prototype buffers, so every other
   test passes while the copies sit somewhere else. Removing that one line makes a re-seed
   invisible.

3. **The hierarchy cache's strong buffer clones.** `crates/solarxy-renderer/src/pathtrace/scene.rs`
   keys its cache on `Arc::as_ptr` and holds strong clones of both buffers, because an address
   only identifies an allocation while that allocation is alive. Its sweep runs once at the end
   of a batch, never inside the remove arm.

4. **The accumulator ping-pong ordering.** `crates/solarxy-renderer/src/pathtrace/backend.rs:729`
   swaps **before** a dispatch and never after one, with a first dispatch that does not swap.
   Every reader reads the write slot, so a swap on the far side leaves a converged pane
   re-resolving the slot from the dispatch before it. `crates/solarxy-host/tests/traced_backend.rs`
   is the only thing that catches an inversion.

5. **The mid-tile preview's own readback slot.** The still job's preview does not borrow the
   tile's readback slot, because that one gates sampling and a preview sharing it would halve
   the sample rate to show progress.

6. **`StillCtx` takes a clock rather than reading one.** `now_ms` is supplied by the caller at
   every step, from five sites, because `solarxy-host` compiles for the browser and has no
   `Instant`. It is a field on the bundle rather than an argument precisely so that a caller who
   forgets it does not compile. The preview throttle is the only reader, and a zero interval
   means never, which is what keeps a headless render from paying for a composite and a readback
   four times a second with nobody watching.

7. **`BindGroupLayouts` does not contain the path tracer's layouts.** See 5.2. Folding them in
   would impose the tracer's limit floor on every consumer.

8. **Lazily built pipelines stay lazy.** See 5.5.

**Rule.** Any of the above being edited requires the comment at the site to be read first. Each
of them already carries one, and each of those comments is load-bearing under the standard in
1.2.

## Open questions

Recorded rather than asserted, because they could not be resolved from the code.

- Are the two cook budgets (6 ms browser, 8 ms desktop) a measured difference in per-frame
  headroom, or drift? Neither constant carries a reason.
- Is the absence of a workspace `[lints]` table deliberate per-crate strictness, or accretion?
  Specifically: are `inline_always` and `needless_range_loop` allowed in `solarxy-bvh` because
  the traversal must stay term-for-term with its WGSL twin, and is `solarxy-render`'s single
  allow a deliberate new standard the older crates should migrate toward?
- Was CI's clippy left without `--all-targets` to keep the job fast, or is it an oversight? The
  documented local command includes it.
- Is `solarxy-render`'s declared-but-unreferenced `clap` feature a leftover from a design where
  the crate carried the command-line value enums, or was it added by analogy and never
  populated?
- What does `crates/solarxy-renderer/src/pathtrace/probe.rs` cost the shipped wasm?
  `build-wasm.sh` has a feature pass-through for measuring exactly this, and no figure has been
  recorded.
- Should the boundary types be generated or exhaustively pinned? Both close the gap in 6.3; they
  have different costs and the choice has not been made.
- Is the frontend's player import ban left non-recursive deliberately (the directory holds one
  file today) or is it an oversight that stops protecting the player when a second file lands
  under a subdirectory?
- Is there a reason the three payload budgets could not run in the pull-request web job, which
  already produces the bundle?
- Does `crates/solarxy-core/src/raycast.rs`'s `dragon_perf_budget` ever measure anything in CI,
  or is the fixture absent on the runners?
- What would a per-pass GPU timing harness cost, given that enabling `TIMESTAMP_QUERY` means
  changing the workspace's `Features::empty()` posture to a requested-if-available arm?
