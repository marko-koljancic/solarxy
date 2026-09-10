# 0016. The interface derivation splits between the registry and the application layer

- **Status**: accepted
- **Date**: 2026-09-07

## Context

Eleven TypeScript modules under `web/src`, 1,329 source lines and 1,216 test lines, encode rules
that both shells need: which parameter is visible under which condition, how a
node's report reads, what a wire type looks like and which coercions are legal, how the scene
tree folds, how an attribute table formats, when a dragged parameter commits, and what the
expression lane may hold.

The desktop has none of it. The 0.10.0 milestone brings the desktop to a parametric studio, so
that logic has to exist in Rust. Porting it produces a second implementation of rules that
already exist, on the release whose purpose is to stop having two of things.

Three documents pointed in three directions. [adr/0012](0012-shared-application-layer-is-a-new-crate.md)
decided a new crate, working name `solarxy-studio`, above the engine and the render host. The
0.10.0 milestone's decision 18 named a different new crate for the derivation alone, depending on
`solarxy-core` and `solarxy-graph`'s snapshot types, carrying no wgpu so a documentation
generator or a schema exporter could use it. And
[04-target-architecture.md](../04-target-architecture.md) Position 6 says of parameter
visibility, without reference to either: "Visibility conditions are declared on the parameter
specification and validated by the registry, and no Rust code evaluates one. The only evaluator
in the repository is thirty lines of TypeScript... One evaluator, in Rust, exposed on the same
resolution path the parameter panel already pulls."

That resolution path is in `solarxy-graph`. So one document put visibility evaluation in the
engine while another explicitly rejected the engine as a home for the derivation.

Reading the modules dissolves the disagreement. They are not one kind of thing.
`web/src/components/paramVisibility.ts` carries the `showIf` evaluator, which is a fact about a
node type, in the same file as `tabLabel`, which capitalizes a group name for a tab strip. The
split runs inside files, not between them, which is why a module-by-module argument could not
settle it.

## Options considered

### Option A: split by what a rule is

Semantics into the registry resolution path in `solarxy-graph`; presentation into the new crate.

A rule is semantics if it answers a question about a node type that any consumer would ask:
whether a parameter is visible given the current parameter values, what a node's report says,
whether a connection between two types is legal and what it costs. A rule is presentation if it
answers how a surface should draw or arrange something: which glyph, which role silhouette, where
in the palette, what the info line reads, how the tree folds, how an attribute table formats,
when a dragged value commits.

### Option B: one crate holding everything, including visibility evaluation

Simplest, and closest to ADR 0012 read literally.

It costs the property the milestone wanted: the crate depends on `solarxy-host`, which depends on
`solarxy-renderer`, which is wgpu. A documentation generator, a schema exporter, or any future
headless consumer of the derivation would link a GPU stack to ask whether a parameter is visible.
It also leaves Position 6's ruling unimplemented, since the evaluator would not be on the
registry's resolution path.

### Option C: two crates

A wgpu-free derivation crate beneath the session crate. Honours both documents literally and
layers cleanly.

It costs two gated crate additions in the largest release the project has attempted, and the
lower crate's charter is hard to state without reference to the upper one.

### Option D: no new crate, all of it on the registry

Zero gated additions now.

It puts glyph mapping, palette placement and tab labels in the engine, which is presentation in
the one crate every consumer links, and it is the thing the milestone's decision 18 rejected in
terms.

## Decision

**Option A.** The derivation splits by what a rule is.

**Semantics go on the registry resolution path in `solarxy-graph`**: parameter visibility
evaluation, the node report, and the data-type coercion verdicts. They are declared on the
parameter specification already and validated by the registry, so this is where Position 6 puts
them, and it makes them free to every consumer including one that never draws anything.

**Presentation goes into the new crate**, which is `solarxy-studio` as ADR 0012 named it rather
than a second crate under a second name. It is born with the presentation charter and widens as
the shared-layer migration moves session behaviour into it.

## Consequences

The release adds one workspace crate rather than two, and ADR 0012 stands rather than being
superseded in its first week.

`solarxy-graph` gains presentation-adjacent surface it did not have. The line is the one stated
above and it will be argued at the margin; the test is whether a consumer that draws nothing
would still ask the question.

The four crate tasks on the 0.10.0 board were written assuming a module-by-module move and have
boundaries drawn by module. They are redrawn by rule as part of starting that work, which is a
change of shape rather than of scope.

`web/src` loses the rules and reads the results across the boundary it already has. The 1,216
lines of tests come across and become the Rust tests, which is what makes the move verifiable
rather than hopeful, and the browser suite staying green without them is the proof that behaviour
did not change.

It loses the rules rather than the modules, and the difference was found by doing it. Six of the
eleven carry a residue that is host-bound rather than shared: an absolute date needs a locale and
a timezone, a virtualization window is a fact about a scrolling container, parked expression text
is per-session interface memory the frozen scene schema pushed out of the document, glyph art and
the set of drawable silhouettes are what one shell can draw, a draft-commit hook is React, and an
index lookup over a snapshot the caller is already holding is not worth a crossing. The measure of
this decision is therefore that no rule has two implementations, not that eleven files are gone;
each residue names what left it and why what stayed had to.

`solarxy-studio`'s first content is therefore presentation rather than session state, which is
the opposite order from the migration sequence in
[09-evolution-and-roadmap.md](../09-evolution-and-roadmap.md). That is deliberate: the
presentation rules have a second consumer waiting on them in this release, and the session
behaviours do not.
