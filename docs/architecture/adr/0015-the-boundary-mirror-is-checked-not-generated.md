# 0015. The TypeScript boundary mirror stays hand-written and is checked for exhaustiveness

- **Status**: accepted
- **Date**: 2026-09-07

## Context

`web/src/engine/types.ts` is 931 hand-written lines mirroring roughly eighty Rust types that
cross the WebAssembly boundary: `Command`, `EngineEvent`, `HostEvent`, the registry snapshot,
and the boundary record types in `crates/solarxy-web/src/app.rs`. Rust owns the schema; the
TypeScript is a copy of it maintained by hand.

Three Rust tests pin a sample of variants, and one source-level scan pins the serde attributes.
Nothing asserts that the two sides carry the same set of variants and fields. A `Command`
variant added in Rust and forgotten in TypeScript compiles, links, ships, and fails only at
runtime.

That is not hypothetical. `crates/solarxy-web/src/app.rs:198-205` records the failure it already
caused: `HostEvent` carried `rename_all` without `rename_all_fields`, so a multi-word field
crossed the boundary in snake case while the TypeScript declared it camel, and the still-render
dialog's elapsed and remaining readouts were blank for a whole release. Neither side was wrong
on its own; only the pair was.

[05-boundaries-and-contracts.md](../05-boundaries-and-contracts.md) named this the most
contract-like boundary in the system and the least mechanically protected, proposed an
exhaustiveness check, and left the choice open. The 0.10.0 milestone independently specified
generation. The two had to be reconciled before that release began, because 0.10.0 takes the
desktop from four dispatched commands to substantially all thirty-five and widens the snapshot
the parameter panel reads, so the mirror moves under more pressure than it has ever been under.

## Options considered

### Option A: generate the TypeScript from the Rust definitions

A build step emits the mirror, and the generated file is committed with a drift gate asserting
it matches what the generator produces. This is what `web/src/engine/types.ts:4-5` already names
as the intended follow-up.

It removes the class of error entirely rather than reporting it: a missing variant cannot exist,
because nobody writes the file.

It costs a build step in a frontend that has one already, a generated file in review, and a
generator to maintain against serde's attribute surface. It also replaces types a reader can
read, and currently annotate, with output.

### Option B: keep the mirror hand-written and assert it is exhaustive

A test emits the variant and field names of every enum crossing the boundary from the Rust
definitions and asserts the TypeScript carries exactly that set, no more and no fewer.

It is a test rather than a build step, it leaves the hand-written types in place, and it fails
loudly and by name on the exact defect that shipped. It does not prevent the defect, it catches
it before merge, which for a repository with one maintainer and a green-before-merge habit is
the same outcome at lower cost.

It costs a parser: the check reads Rust source rather than serializing a value, because
`HostEvent` lives behind `cfg(target_arch = "wasm32")` and no native test can construct one.
`crates/solarxy-core/tests/tokens_drift.rs` already contains a source-level scan for exactly
that reason, so the technique is established here rather than novel.

### Option C: generation, and drop the sample assertions

Rejected on its own terms. A generator proves the shapes agree; the three camelCase assertions
prove the serde attributes are right. Those are different failures, and the second is the one
that has already cost a release.

## Decision

**Option B.** The mirror stays hand-written and gains an exhaustiveness check. The three
camelCase assertions stay beside it, because the check proves the shapes agree and the
assertions prove the serde attributes are right.

Hand-mirroring is a legitimate choice rather than a defect. What it does not do is stay correct
by itself, and this supplies that without a code generator in the build.

## Consequences

The failure mode this exists to stop becomes a red test naming the missing variant rather than a
blank readout discovered in production.

The mirror stays readable and annotatable, which matters because it is the one place a frontend
reader can see the whole boundary at once.

The check has to be kept honest as the boundary grows: an enum added in Rust and not added to
the check's list is invisible to it. The list is therefore derived from the source rather than
enumerated by hand, which is the same discipline the registry drift tests already follow.

This reverses the 0.10.0 milestone's decision 20, which had specified generation. That document
carries a dated amendment saying so, its sections 3.6 and 5.4 are rewritten, its exit criterion 7
now reads checked, and the board epic for the work is rescoped from a generator to a test.

If the boundary later grows past what a source scan can read reliably, generation is still
available and this ADR is superseded rather than edited.
