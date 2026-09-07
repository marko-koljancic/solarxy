# Evolution and roadmap

The ordered path from [03-current-architecture.md](03-current-architecture.md) to
[04-target-architecture.md](04-target-architecture.md).

Every step here is independently shippable. None of them requires the next one to be useful,
and none of them leaves the tree in a state that has to be finished before a release. That
constraint is what makes the list executable by one maintainer across many milestones rather
than a rewrite that has to land whole.

Each step states what it is, why it matters, a rough size, what it unblocks, and what stays
broken if it is skipped. Sizes are relative and assume the maintainer's own familiarity with
the code:

- **Small**: a sitting. One file or a few, and a test.
- **Medium**: a few sittings. Touches several files, needs its own thought about edge cases.
- **Large**: a milestone's worth. Touches many files or crosses a boundary.
- **Extra large**: several milestones. Only one step here is this size, and it is broken into
  shippable pieces below rather than left as one item.

The groups below are ordered by leverage, not by appetite. Group A is first because every
later group is easier to land safely once the rules are checkable, and Group B is second
because those defects are cheap, silent, and currently shipping.

## Group A: make the rules checkable

The whole of [05-boundaries-and-contracts.md](05-boundaries-and-contracts.md) is a
specification that nothing enforces. Four small checks turn it into a gate. This group is
first because it is the cheapest work in the document and it protects everything after it.

**Adopted into the 0.10.0 milestone on 2026-09-07**, as its item 1G and as a board epic
sequenced before the host extraction. A1 and A4 are scheduled there. Two things drove it: that
release moves behaviour between crates, which is the case A1's "Unblocks" note names, and the
continuous integration gate it would be judged against was found to be failing 13 of its last 25
runs, always the GPU job and always different tests, while thirty local runs of the same files
passed. So the gate could not have discharged the release's own acceptance criteria. The triage
of that rides with this group because it is the same question: whether a rule the project states
is actually checked.

### A1. Dependency allow-list assertion

**What.** A test that reads the allow-matrix in
[05](05-boundaries-and-contracts.md), reads `cargo metadata`, and fails on any workspace edge
not in the list, in any dependency kind including `dev-dependencies` and target-conditional
ones.

**Why.** Every layering finding in [03](03-current-architecture.md) is an edge or a near-edge.
A machine reading a fifteen-line table catches all of them for the cost of one test. It also
makes the three load-bearing non-edges real: the engine-to-renderer pair, and
`solarxy-host` to `solarxy-graph`, which today live in comments.

**Size.** Small. Tests are not a gated addition, and `cargo metadata` already exposes exactly
the required data.

**Unblocks.** Group E. Moving behaviour between crates is safe to do incrementally only if a
wrong edge fails loudly.

**If skipped.** The most important boundary in the workspace stays enforced by the fact that
nobody has typed the wrong `use` yet.

### A2. Frontend module import rule

**What.** The same check for `web/src`, reading the per-module allow-lists in
[04](04-target-architecture.md).

**Why.** Four cycles exist today and each is state reaching sideways into the view. Each one
makes a module untestable without a DOM, and untestable-without-a-DOM is exactly the property
that stops logic moving into shared Rust later.

**Size.** Small.

**Unblocks.** Group E's frontend half.

**If skipped.** The cycles multiply, and the modules that most need to move are the ones most
entangled.

### A3. File and function size ratchet

**What.** A report, not a gate at first: current counts per file and per function, compared
against a committed baseline, failing only when a file crosses a threshold it was previously
under or a new file lands over it.

**Why.** [08-engineering-standards.md](08-engineering-standards.md) sets soft caps that 123
Rust files and 31 TypeScript files exceed today. A cap violated on the day it ships is
decoration. A ratchet with a committed baseline is the same rule with teeth and no burn-down
prerequisite.

**Size.** Small.

**If skipped.** The size standard stays advisory, which is how the current distribution arose.

### A4. Boundary exhaustiveness test

**What.** Emit the variant and field names of every enum crossing the WebAssembly boundary
from Rust, and assert the TypeScript mirror matches that list exactly.

**Why.** The mirror covers roughly 80 types by hand; the only guards today are three JSON
spot-checks covering six variants and one source grep. Nothing checks that the 35 `Command`
variants and 21 `EngineEvent` variants are all present with matching field names. The field
naming trap in [05](05-boundaries-and-contracts.md) has already cost one release, and the
existing guard against it is narrower than the failure it was written for.

**Size.** Small to medium, depending on whether the Rust side emits the list from a test or
from a small example binary.

**Unblocks.** Confidence in every later change to the boundary, which Group E makes many of.

**If skipped.** A variant added in Rust and forgotten in TypeScript compiles cleanly on both
sides and fails at runtime.

**Settled 2026-09-07.** This item is now the decision rather than one of two options.
[adr/0015](adr/0015-the-boundary-mirror-is-checked-not-generated.md) rules that the mirror stays
hand-written and is checked, closing the open question [05](05-boundaries-and-contracts.md) left
standing. The 0.10.0 milestone had specified generation and was reversed to match, so this item
is scheduled in that release rather than unscheduled.

## Group B: close the silent correctness defects

These are cheap, they are shipping, and none of them announces itself. Each is small on its
own; together they are the highest ratio of user-visible correctness to effort in the
document. Each is stated with its evidence in [03](03-current-architecture.md) and
[06-cross-cutting-concerns.md](06-cross-cutting-concerns.md).

### B1. Clear the colour-grading cache on document load

**What.** `CookEngine::reset` clears nine maps and not the grading-table cache, while the
per-node forget path does remove it. Node ids restart from the loaded document, so a colliding
id inherits the previous document's grading.

**Size.** Small. The fix is one line; the test that pins it is the real work.

**If skipped.** Opening a second scene in one session can grade it with the first scene's look,
silently.

### B2. Clear the environment and grading side channels on bypass

**What.** The bypass path commits validation but not the environment or grading caches, both of
which the scene lowering reads directly rather than through node outputs. Bypassing an
environment or camera node therefore leaves its contribution live.

**Size.** Small.

**Note.** [06](06-cross-cutting-concerns.md) records this as ambiguous intent rather than an
obvious defect, since bypass clearing validation but not these two could be deliberate. It
needs a ruling before it needs a patch.

### B3. Primitive attribute lanes across a primitive-count change

**What.** Subdivision emits four triangles per input triangle and then clones the
primitive-attribute map verbatim, leaving a lane of length N on a mesh with 4N primitives.
Deletion has the same shape.

**Why.** [ADR 0014](adr/0014-two-attribute-domains.md) accepts two domains as the ceiling and
commits the effort to making them correct instead. This is that effort. Nothing declares the
invariant that a lane's length equals its domain's element count, and nothing checks it.

**Size.** Medium. Two operators need a real answer for what happens to a lane when a primitive
splits, and the invariant needs a home and a check at cook commit.

**If skipped.** Any downstream reader of a primitive lane after a subdivide reads misaligned
data, with no error anywhere.

### B4. Wrangle programs create dependency edges

**What.** The expression dependency index scans parameters whose source is an expression. A
wrangle program is stored as literal text, and it can call the cross-node read. So an upstream
parameter change never re-cooks the wrangle that reads it.

**Size.** Medium. The index has to learn to scan snippet parameters, which means parsing them
at index time.

**Precondition.** A ruling on whether cross-node reads inside a wrangle program are a supported
feature. The node's own shipped help advertises it and the cook wires it through, which argues
yes, but that should be stated rather than inferred.

**If skipped.** A stale wrangle result that only a manual recook corrects, which is the exact
failure mode [ADR 0005](adr/0005-expressions-read-document-state.md) exists to prevent.

### B5. Parameter reset ordering, and rename on reset

**What.** Two related defects in the same command. Resetting parameters removes them before
marking dirty, so the expression referrers of a reset parameter are derived from a node that no
longer has the parameter and are never re-cooked. And resetting a node's name re-mints a
different name without rewriting the cross-node reads that pointed at the old one, where the
set-parameter path does rewrite them precisely so a rename carries its referrers.

**Size.** Small. The dirty marking moves before the removal; the reset path calls the same
rewrite the set path already calls.

**If skipped.** Reset silently breaks references that rename correctly preserves, which is the
worse kind of inconsistency because the working case teaches the wrong expectation.

### B6. The scene lowering's stopped clock

**What.** The lowering builds its evaluation contexts with a default scene time at six sites,
so time-driven lights, cameras and transforms evaluate at frame zero regardless of playback.

**Size.** Small to change, medium to land, because it changes what every existing golden
capture lowers if any golden scene uses time.

**Precondition.** A ruling. Frame-zero lowering may have been chosen for reproducibility. If it
was, it should say so at the call sites; if it was not, this is a straightforward defect.

**If skipped.** Animation does not reach lights, cameras or transforms, and nothing says why.

### B7. Step the scene migration in a loop

**What.** The schema migration entry point is called once, while its own documentation
describes stepping one version at a time. Correct today with one step defined; wrong on the day
a second is added, and wrong silently.

**Size.** Small.

**If skipped.** The second schema version ships a migration that is never reached from the
first, and the failure appears as a malformed document rather than as a version error.

### B8. Preserve parameters on an unknown node type

**What.** A placeholder node for an unknown or too-new type constructs itself with an empty
parameter map, making save-after-open lossy, while three separate doc comments state that
parameters are preserved.

**Size.** Medium. Preserving them means carrying raw parameter values the registry cannot type,
which is a real design question rather than a patch.

**If skipped.** Opening a file from a newer build and saving it destroys data, and the
documentation says it does not.

### B9. Commit a real old scene file as a migration fixture

**What.** Every migration test constructs its input with the current writer. That proves the
migration is self-consistent, not that it reads a file an earlier release actually wrote.

**Size.** Small, and it is work only a person can do: produce a file with a real old build and
commit it.

**If skipped.** The compatibility promise in [ADR 0006](adr/0006-slxy-scene-file-format.md) is
tested against itself.

## Group C: make the material model one model

[ADR 0013](adr/0013-path-tracer-is-the-shading-ground-truth.md) settles that the path tracer is
ground truth. This group is what that decision costs, and it is worth paying because the
alternative is a viewport that lies about the render.

### C1. Give `thickness` one meaning

**What.** It is an optical path length in the rasterizer and a thin-or-solid flag in the
tracer. One authored parameter, one help string, two incompatible physical meanings.

**Size.** Medium, and it needs a ruling first: which meaning is the parameter's, given the
tracer is authoritative on shading but the rasterizer's Beer-Lambert reading is the more
conventional one for the name.

**If skipped.** Every transmissive material is authored against whichever renderer the user
happened to be looking at.

### C2. Read `emissive_strength` in the raster path

**What.** It is uploaded, declared in the shader struct, and read by no raster shader. The
tracer honours it, as do the importer, the exporter and the node parameter.

**Size.** Small.

**If skipped.** Viewport and render differ by the full value of the multiplier, which for an
authored emissive is often a large factor.

### C3. Decide the traced atlas filtering and mip policy

**What.** The atlas filters in encoded sRGB with no mip chain; the raster path decodes per
texel and filters a full chain. Under ADR 0013 this is either a listed approximation with a
reason or a defect.

**Size.** Medium to large. A mip chain in the atlas is real work, and the correct long answer
involves ray differentials picking a level.

**If skipped.** A traced texture aliases where the rasterized one does not, and the difference
is unexplained.

### C4. Move the alpha test into the shared traversal

**What.** The depth auxiliary pass shoots through alpha-masked surfaces the beauty ray stops
at, because the alpha test lives in the path kernel loop rather than in the traversal both
kernels share.

**Why.** This is exactly the second-code-path defect that
[06b-rendering-and-shading.md](06b-rendering-and-shading.md) states as a rule: an auxiliary
pass derives from the same evaluation as beauty, and one produced by a separate path is a
defect.

**Size.** Medium.

**If skipped.** A depth pass that disagrees with the image it accompanies, which is worse than
no depth pass because a compositor trusts it.

### C5. A rasterizer-versus-tracer agreement test

**What.** One scene, both engines, an image comparison with a stated tolerance and a list of
the differences that are permitted and why.

**Why.** ADR 0013 makes this the test that gives the decision teeth. Today the tracer is
tested against analytic expectations and the rasterizer against its own past, and nothing
compares them to each other.

**Size.** Medium. The image comparison already exists as one shared definition; the work is
choosing a scene that exercises the disputed parameters and agreeing the tolerance.

**Depends on.** C1 and C2, or it fails on day one for known reasons.

## Group D: build and release integrity

Two real gaps, both stated in
[07-build-release-and-platforms.md](07-build-release-and-platforms.md), plus the lint
situation from [08](08-engineering-standards.md).

### D1. Pin the toolchain that builds shipped binaries

**What.** Every continuous-integration job pins the toolchain; the workflow that builds the
bundles uses the unpinned stable channel, and the release tool installs its own. The binaries a
user installs are not built by the toolchain that verified them.

**Size.** Small.

**Precondition.** A ruling on whether the unpinned channel was deliberate, to pick up newer
code generation for shipped artefacts. If it was, the fix is to document it and to build one
verification job on the same channel, rather than to pin.

**If skipped.** A code generation difference between the verifying and shipping toolchains
reaches users first.

### D2. Move the payload budgets to the pull request

**What.** The three budgets are real, enforced, and numbered, and they live in the release
workflow. The pull request job already produces the bundle and asserts nothing about its size.

**Size.** Small. The bundle is already built in the earlier job; the budget check is a
comparison.

**If skipped.** A bundle regression is discovered after the release is announced, which is when
it is most expensive to fix.

### D3. Workspace lints, and lint the targets that are not linted

**What.** There is no workspace lint table; allow-lists are hand-copied across crates in
several distinct configurations. Continuous integration's lint step omits the flag that
includes tests, examples and benches, so a substantial body of code is never linted, and the
locally documented command does include it, so the two disagree.

**Size.** Medium. Adding the flag will surface findings in code that has never been linted.

**If skipped.** Lint policy stays per-crate folklore, and moving code between crates changes
which lints apply to it.

## Group E: the shared application layer

The parity work. This is the extra-large item, and it is broken here into pieces that each ship
on their own. [ADR 0012](adr/0012-shared-application-layer-is-a-new-crate.md) settles the
shape; this is the sequence.

The measure of progress is concrete: today the desktop shell dispatches two of the engine's 35
commands. Each step below moves a behaviour into the shared layer, and both shells gain it at
once.

### E0. Create the crate

**What.** An empty `solarxy-studio` depending on `solarxy-graph` and `solarxy-host`, with its
allow-list entry in [05](05-boundaries-and-contracts.md) and the A1 assertion covering it.

**Size.** Small in code and gated in process: a new workspace crate is an addition the working
agreement requires approval for. Nothing moves until that approval exists.

**If skipped.** Everything else in this group is blocked, which is the reason it is listed
separately rather than folded into E1.

### E1. Move the keymap

**What.** The shortcut table. The frontend already treats its table as the single source and
generates its shortcuts modal from it; the desktop has a hand-maintained modal.

**Why first.** It is the smallest thing that is genuinely duplicated, it has no dependency on
document state, and it proves the seam works before anything harder crosses it.

**Size.** Medium.

**Unblocks.** A generated shortcuts reference on both shells, and a drift test holding them to
one table.

### E2. Move view state and display settings

**What.** Per-pane display settings, the global display settings, and the resolution of a
pane's effective look. The types already live in the foundation crate; what is duplicated is
the behaviour around them.

**Size.** Medium.

**If skipped.** The desktop and browser keep drifting on what a display mode means, which is
already visible in the per-pane look a browser-authored scene carries that the desktop drops.

### E3. Move the pane and layout model

**What.** The split-viewport model, the active-pane rule, and the layout presets. The geometry
math is already shared in the renderer; the model around it is not.

**Size.** Large. Both shells have their own arrangement systems above this, and only the model
moves, not the arrangement.

### E4. Collapse the still-render driver

**What.** The still pump loop is written three times, once per surface, and its own
documentation admits the duplication. The settings resolver beside it is byte-identical in
three crates in a form that does not get the compiler's exhaustiveness help, unlike its sibling
which does.

**Why here.** It is the largest existing duplication that is already almost shared: the job
itself lives in the render host, and only the loop driving it is triplicated.

**Size.** Large.

**If skipped.** A sixth denoise steering value added to the settings compiles in all three
places and is honoured in none, which is the specific failure the current shape permits.

### E5. Move selection and tool state

**What.** Selection is already an engine command; what is duplicated is everything around it,
including the tool mode, the gizmo target resolution, and what a selection means to each
surface.

**Size.** Large.

**Unblocks.** The desktop reaching genuine editing parity, since a tool that cannot express its
target cannot write through it.

### E6. Move the command and menu model

**What.** The list of things a user can invoke, their enablement rules, and their grouping, as
data rather than as two hand-built menu trees.

**Size.** Large.

**Unblocks.** The desktop dispatching the rest of the 35 commands, which is the headline parity
number.

### E7. Retire the shells' direct engine edges

**What.** With E1 through E6 landed, `solarxy-app` and `solarxy-web` no longer need to depend on
`solarxy-graph` directly. Remove the edges and let A1 keep them removed.

**Size.** Small once the preceding steps land, and it is what makes the allow-matrix in
[05](05-boundaries-and-contracts.md) true rather than aspirational.

## Group F: structural cleanup

Not parity, not correctness. These are the changes that make the next decade of work cheaper,
and none of them is urgent.

### F1. Split the foundation crate

**What.** The foundation crate is a foundation by dependency count and a grab-bag by content,
carrying geometry, the engine-renderer contract, a user-interface palette, desktop preferences
and install-channel detection. Several are feature-gated, so the cost is smaller than it looks,
but the incoherence is real.

**Size.** Large.

**Note.** The adversarial pass corrected the original finding here: preferences and
install-channel detection are feature-gated and two crates take the foundation with default
features off, so this is a coherence problem rather than a compile-cost problem. That changes
its priority, not its diagnosis.

### F2. Decompose the wasm host

**What.** 6,489 lines, 169 methods on one type, zero tests in the file, and 11 in the whole
crate. It is the largest single-file concentration in the workspace and the least tested.

**Size.** Large.

**Why it matters more than its size suggests.** Group E moves behaviour out of exactly this
file. Splitting it first makes each of those moves a smaller, reviewable change.

### F3. Decompose the other named hotspots

**What.** The engine facade at 4,201 lines, the pipeline constructor whose single function is
983 lines, the frame module at 2,549, and the frontend session controller at 1,248 with six
module-level mutable globals and 58 exported functions. Concrete splits for each are proposed
in [08-engineering-standards.md](08-engineering-standards.md).

**Size.** Large, and naturally incremental: each file is its own step.

### F4. Unify the two review models

**What.** Review is modelled twice in two crates, a sidecar-file model used only by the desktop
and an engine model used only by the browser, with no adapter between them.

**Size.** Large.

**Precondition.** A ruling on which model wins. This is in
[10-risks-and-open-questions.md](10-risks-and-open-questions.md) and cannot be answered from
the code, because both are actively maintained.

### F5. Retire the desktop's second scene representation

**What.** The desktop carries two mutually exclusive scene representations and every downstream
consumer branches on the pair. The still-render path already routes a file load through the
document path, which is the proof the second representation is removable.

**Size.** Large.

## Group G: the render graph and the quality bar

### G1. Make pass ordering explicit

**What.** There is no render-graph abstraction; ordering is implicit in call sequence.
[ADR 0011](adr/0011-pass-ordering-is-implicit.md) records that this was a decision by default
rather than a considered one. The step is to declare each pass with its inputs, its outputs,
and whether its position is required for correctness or for performance.

**Size.** Large.

**If skipped.** Pass order stays a property of a function body, and the distinction between an
ordering that is required and one that is merely current stays in people's heads.

### G2. Per-pass GPU timing

**What.** Timestamp queries where the platform supports them, measured against a budget.

**Precondition.** A decision, because the workspace requires no optional GPU features anywhere,
and timestamp queries are an optional feature. Requesting one if-available is a change to the
posture in [ADR 0008](adr/0008-no-optional-gpu-features.md), not just a new measurement.

**Size.** Medium, plus the decision.

**If skipped.** Optimisation continues without measurement, which
[08](08-engineering-standards.md) forbids in principle and nothing prevents in practice.

### G3. Colour pipeline round-trip assertions

**What.** Assertions that pin the space and precision at each stage of the chain that
[06b](06b-rendering-and-shading.md) documents, so a future change that moves a conversion fails
rather than shifts every image slightly.

**Size.** Medium.

## Reading the order

If only one group is done, do A. It is the cheapest and it protects everything else.

If two, do A and B. Group B is small, entirely defensive, and every item in it is currently
shipping.

Group E is the one the product roadmap cares about, and it is deliberately last among the
substantial groups, because moving behaviour between crates without A1 in place is how a
boundary quietly stops holding.

Nothing in this document is a ticket. The grouping is intended to survive being fanned out onto
the project board, and several groups map naturally onto a point release, but the mapping is a
decision for the milestone planning rather than something this document should presume.
