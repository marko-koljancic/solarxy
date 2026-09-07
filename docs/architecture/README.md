# Solarxy solution architecture

This directory is the architecture of record for Solarxy. It describes the system as it is,
defines the system as it should be, records the decisions that got us from one to the other,
and states the engineering standards that govern how code is written here.

It is written for a competent contributor who has never seen this codebase.

## Why this exists

Before this set, Solarxy's architecture lived in three places that were never meant to hold
it: prose in `CLAUDE.md`, essay-style module doc comments, and the briefing files under
`.claude/`. Six named architecture invariants were written down in a skill file and enforced
by no code at all. The single most load-bearing boundary in the workspace, the rule that the
shared render host must not depend on the graph engine, was recorded in a `Cargo.toml`
comment.

That gap was measurable. A shading parameter ended up with two incompatible physical
meanings in two renderers. A settings helper was hand-copied into three crates in a form
that defeated the compiler check its own sibling was built around. A function was documented
as shared while being implemented twice.

An architecture that is written down can be argued with. One that is not is just whatever
the code happened to do.

## How to read this

Read in order the first time. After that, jump to what you need.

| Document | Read it when |
|---|---|
| [01-context-and-drivers.md](01-context-and-drivers.md) | You want to know what Solarxy is, who uses it, and what forces shaped it |
| [02-principles-and-constraints.md](02-principles-and-constraints.md) | You are about to make a design choice and need the rules and the hard limits |
| [03-current-architecture.md](03-current-architecture.md) | You need the honest as-is, including where it is wrong |
| [04-target-architecture.md](04-target-architecture.md) | You need to know what a crate or module is allowed to own and depend on |
| [05-boundaries-and-contracts.md](05-boundaries-and-contracts.md) | You are changing anything that crosses a boundary |
| [06-cross-cutting-concerns.md](06-cross-cutting-concerns.md) | You are touching errors, threading, caching, determinism, or accessibility |
| [06b-rendering-and-shading.md](06b-rendering-and-shading.md) | You are touching a pass, a shader, a material parameter, an AOV, or colour |
| [07-build-release-and-platforms.md](07-build-release-and-platforms.md) | You are changing the build, the CI, or a shipped artefact |
| [08-engineering-standards.md](08-engineering-standards.md) | You are writing code or reviewing someone else's |
| [09-evolution-and-roadmap.md](09-evolution-and-roadmap.md) | You want to know what to build next and in what order |
| [10-risks-and-open-questions.md](10-risks-and-open-questions.md) | You want the known unknowns and the decisions still owed |
| [adr/](adr/) | You want to know why a decision was made, and what it cost |

Two documents carry most of the weight. [04](04-target-architecture.md) holds a
responsibility card for every crate and every frontend module, and the card's `Does not own`
and `Must not depend on` fields are the ones that actually prevent drift.
[08](08-engineering-standards.md) is the one a reviewer points at.

## Current versus target

These are kept strictly apart, and it matters.

[03](03-current-architecture.md) is descriptive. Everything in it is true today and cites a
path. Where the code is wrong, it says so rather than describing the intent.

[04](04-target-architecture.md) is prescriptive. Nothing in it is a description of today
unless it says so explicitly. Where a responsibility card describes a boundary that does not
yet hold, the card says which [09](09-evolution-and-roadmap.md) step establishes it.

If you find aspiration stated as fact in 03, or a target claim written in the present tense
in 04, that is a defect in the document, not a nuance.

## Evidence rule

Every structural claim in this set cites the path it came from. A claim without a citation
is a defect. If something could not be verified from the code it appears in
[10](10-risks-and-open-questions.md) as an open question rather than being smoothed over.

Line numbers drift. Paths drift more slowly. Where a citation names a line, treat the line as
a hint and the path as the claim.

## Decisions

Every non-obvious decision is an [ADR](adr/): context, options considered, decision,
consequences, status. Many of the ADRs here are retroactive, written to capture a decision
that was already embodied in the code but never articulated. Those are marked accepted with
the release they were embodied in, because the decision was real even though the record was
not.

An ADR is never edited to change its decision. It is superseded by a new one, and the old
one's status becomes `superseded by NNNN`. The trail is the point.

## When you must update this set

These are obligations, not suggestions. Each of them invalidates something written here.

- Adding, removing, renaming or re-scoping a crate. Update 04's card and 05's allow-matrix.
- Changing a boundary contract, including any type that crosses the wasm boundary. Update 05.
- Adding a platform target or a shipped artefact. Update 07.
- Changing the persistence format or its migration behaviour. Update 05 and the relevant ADR.
- Adding a render pass, changing pass order, or changing a colour-space conversion. Update 06b.
- Adding or changing a material parameter, or an AOV. Update 06b's material and AOV contracts.
- Introducing a cross-cutting concern that did not exist. Update 06.
- Reversing or superseding a decision. Write the new ADR, do not edit the old one.

Architecture changes are proposed here and approved before implementation, not discovered
afterwards. A pull request that changes a boundary without changing this set is incomplete.

## Keeping it honest

Documentation drifts silently unless something makes it fail loudly.
[09](09-evolution-and-roadmap.md) proposes the mechanical checks that would make drift in
this set noisy: a dependency allow-list assertion derived from 05's matrix, a file-size
report, cycle detection, and for rendering the golden-image comparison, an AOV-versus-beauty
consistency assertion, and per-pass GPU timing against a budget.

The `solarxy-audit` skill under `.claude/skills/` is the existing code-quality rubric. It
already works by deferring current-state facts to a source of truth and auditing the code
against it, which is exactly the right shape. It should be **extended, never duplicated**, so
there is one rubric rather than two that disagree. Three changes, proposed rather than made:

1. **Repoint its current-state deferral.** Today it defers to `CLAUDE.md` alone. That file
   stays authoritative for current-state facts such as crate roles, feature flags, enum
   variants and pass order. This set becomes authoritative for boundaries, responsibilities,
   decisions and standards, and the skill should audit against both.
2. **Add an architecture-conformance category.** Its checks come from
   [05-boundaries-and-contracts.md](05-boundaries-and-contracts.md) and the cards in
   [04-target-architecture.md](04-target-architecture.md): a dependency edge outside the
   allow-matrix, a module importing something its card denies, a responsibility landing in a
   crate whose card says it does not own it, a decision contradicted without a superseding
   ADR, and a boundary type changed without its contract updated.
3. **Add a rendering-pipeline quality category.** Its checks come from
   [06b-rendering-and-shading.md](06b-rendering-and-shading.md): a texture read or write whose
   colour space is inferred rather than stated, an auxiliary output produced by a code path
   other than the one that produces beauty, a material parameter interpreted differently in
   two places, a pass whose ordering constraint is undocumented, a shader or pipeline change
   with no golden comparison and no stated reason none applies, and an optional GPU capability
   used without a declared fallback.

The mechanical checks are the other half, and they belong in the build rather than in a
rubric a person runs. [09-evolution-and-roadmap.md](09-evolution-and-roadmap.md) proposes them
as its first group: a dependency allow-list assertion derived from the matrix, the same check
for frontend module imports, a file and function size ratchet against a committed baseline,
and a boundary exhaustiveness test. For rendering, the golden-image comparison already exists
with a single shared definition of image difference; what does not exist is an
auxiliary-output-versus-beauty consistency assertion, a rasterizer-versus-tracer agreement
test, and per-pass GPU timing against a budget.

## Conventions in this set

Written for a public repository. No internal shorthand, no milestone planning codes, no
unexplained jargon. Version references are used where they are load-bearing, because
"persisted before 0.8.1" is often the reason a code path exists.

Diagrams are Mermaid, inline in the document that needs them, and each one is followed by a
paragraph saying what to notice. A diagram without a reading guide is decoration, and it is
the first thing to rot.
