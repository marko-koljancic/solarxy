# 0011. Render pass order is implicit in call sequence, and there is no render graph

- **Status**: accepted, with consequences
- **Date**: 2026-09-07

## Context

This decision is different from the others in this set, and the difference should be stated
plainly: the code shows it was made by default rather than deliberately.

There is no render-graph abstraction in the workspace. No type declares what a pass reads or
writes, there is no pass registry, no dependency edge, and no barrier scheduling. Order is
Rust statement order. The sequencer is `encode_pane_passes`
(`crates/solarxy-host/src/pane.rs:410`), which matches on what a pane is showing and calls one
of three hardcoded chains: the six-step raster chain at `pane.rs:259`, the overdraw pair at
`pane.rs:304`, or the UV chain at `pane.rs:605`. The composite is deliberately not in that
function; it is a separate free function at `pane.rs:345` that the caller invokes after the
backend's encode returns, so the full per-pane order lives in two functions with a trait call
between them and each shell writes the stitching itself.

Nothing in the code weighs a graph against a call sequence. There is no comment arguing the
trade, no rejected sketch, and no note saying a graph was considered and declined. The chain
grew one pass at a time, and the shape is the residue of that growth. That is not a criticism
of the result, which works and is legible in the small, but it does mean the property everyone
assumes was chosen was not.

## Options considered

Retrospectively. The code records no comparison, so this section states what the alternatives
would have cost so that the decision can be argued with from here rather than inherited.

### Option A: implicit ordering in call sequence

What exists. Each chain is a short function whose passes execute in the order they are written.
A reader with the file open can see the whole frame.

### Option B: a declared render graph

Each pass declares its reads and its writes; a scheduler derives the order, allocates and
aliases transient targets, and inserts barriers.

It buys three things this design cannot have: an ordering that is checked rather than trusted,
transient resource aliasing that is safe by construction rather than by submission discipline,
and a natural place to hang per-pass instrumentation. It costs a scheduler, an allocator, and
an indirection between a pass and the thing it draws into, in a renderer with roughly 48 render
pipelines and three chains. A graph pays off when the number of chains grows or when passes are
composed dynamically; neither is true today.

### Option C: declared reads and writes without a scheduler

Each pass names its inputs and outputs as data, checked in debug builds against the order the
chain actually runs, with the order still written by hand.

Most of Option B's checking at a fraction of its cost, and it does not touch how a frame is
written. It buys nothing at runtime, which is why it is easy to skip and easy to let rot.

## Decision

Pass order is the statement order of the three chains in `solarxy-host`'s pane module. There is
no render-graph abstraction, and introducing one is a deliberate architectural step rather than
a refactor.

## Consequences

What it costs is specific, and each item is a real property of the code today.

The dependency structure is not readable from any one place. Deriving whether a pass may move
means tracing bind groups across three files. The occlusion chain is the worked example: it
reads the depth and normal buffers written two passes earlier and writes a buffer read only by
the composite, so it could sit anywhere between the two, and its placement after the main pass
is incidental. Nothing says so. Someone reordering it above the geometry pass would get a valid
bind group holding last frame's contents, with no error.

Nothing checks a reorder. The only mechanism that would notice is the golden-capture job, which
renders at the current commit and at the base commit and compares, so a pass-order change
arrives as a pixel difference to be judged by a human rather than as a failure that names what
broke.

Transient targets are aliased across panes with no type expressing the invariant. There is one
instance each of the occlusion, bloom, outline, overdraw and UV-overlap targets, shared by all
four panes of a split layout. What makes that safe is that each pane's write and read of a
shared target are contiguous in recording order, and the shells happen to submit each pane's
encoder before the next pane encodes. The composite already carries a field recording which
backend drew a pane specifically because this aliasing bit once, when a traced pane composited
against its raster neighbour's occlusion buffer. That fix addresses a symptom, not the sharing.

The cost cannot be measured. There is no GPU timing instrumentation anywhere in the workspace:
every render pass descriptor sets its timestamp writes to none, and no timestamp query, query
set or timestamp feature appears in any crate. The only render-path performance figure the
repository can produce is a hand-run, ignored-by-default ray throughput test for the tracer. So
any claim that the current shape is or is not costing frame time is unsupported in both
directions.

What it buys is worth keeping in view. One function read top to bottom is the whole frame, and
a scheduler's derived order is not readable that way. For a renderer with three chains and one
consumer per chain, that legibility is a real asset.

The explicit alternative, and the order in which it would be worth taking, belongs in
[09-evolution-and-roadmap.md](../09-evolution-and-roadmap.md). Option C is the cheap half and
should be considered before Option B.

Enforcement: the golden-capture comparison, and nothing else. It is a pixel gate, not a
structural one.

## Notes

A naming trap for anyone going looking: `crates/solarxy-host/src/passes.rs` contains no render
passes. It is the auxiliary-output display selector and the float-to-display mappings. The pass
chain is in `pane.rs`.
