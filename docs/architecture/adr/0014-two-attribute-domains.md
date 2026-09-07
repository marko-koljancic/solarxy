# 0014. Geometry carries two attribute domains, point and primitive, and that is the ceiling

- **Status**: accepted
- **Date**: 2026-09-07

## Context

Node-based procedural modelling systems in this family conventionally bind attributes at four
levels: point, vertex (meaning per-corner, per-face-vertex), primitive, and detail (meaning
one value for the whole geometry). The four-level model is what lets a UV seam or a hard edge
be expressed as data rather than as duplicated indices, and it gives per-set constants a home.

Solarxy has two. `crates/solarxy-kernel/src/set.rs:34` declares
`AttributeDomain { Point, Primitive }`, and `KernelMesh` carries exactly two maps,
`attributes` and `primitive_attributes`, each a `BTreeMap<String, AttributeData>` over four
lane types: f32, vec2, vec3 and vec4. The frontend mirrors it as
`type AttrDomain = "point" | "primitive"` at `web/src/engine/types.ts:128`.

Position, normal, UV and indices are not attributes at all. They are fixed fields on
`KernelMesh`. Normals additionally exist as a reserved attribute lane named `N`, which
`set.rs:47` describes as "the attribute-lane twin of `KernelMesh::normals`", so one concept is
modelled in two places with no rule stating which wins when both are present.

The reserved names carry a stated type contract. `set.rs:39` says a lane under a reserved name
"must carry the documented type or consumers refuse it", and that refusal is implemented once
per consumer rather than checked centrally, which makes it a convention rather than a contract.

A decision was needed because the gap between two domains and four is not a gap that closes
by itself, and leaving it open meant every operator author had to guess.

## Options considered

### Option A: two domains is the ceiling

Keep the model, state the constraint and its consequences, and spend the effort on the real
defects in what exists.

### Option B: grow to four domains

Add vertex and detail. It is the conventional model and it removes two real expressive limits.

It also touches nearly everything: every operator in `solarxy-kernel`, the transfer codec that
carries geometry to the import worker, the persisted form, the attribute table UI, and the
renderer's ingestion. It is a multi-release migration whose benefit is expressiveness that
nothing in the product currently asks for.

### Option C: add the detail domain only

Detail is the cheap half. It is one value per set rather than a parallel array, so it costs no
per-element storage and no per-operator length bookkeeping, and per-set constants have no home
today.

Still a persisted-form change and a transfer-codec change, for a capability with no current
caller.

## Decision

Two attribute domains, point and primitive, is the deliberate ceiling. Position, normal, UV
and topology remain fixed fields rather than attributes.

Effort goes into making the existing model correct rather than into widening it.

## Consequences

Two expressive limits are accepted and must be documented rather than discovered. Per-corner
data cannot be expressed as an attribute, so a UV seam or a hard edge is represented by
duplicating indices, which is what the fixed `tex_coords` and `normals` fields already assume.
Per-set constants have no home, so anything genuinely global to a geometry is carried on a
parameter or recomputed.

Three defects in the existing model become the actual work, and they are real. Primitive-domain
lanes are copied verbatim by operations that change the primitive count:
`crates/solarxy-kernel/src/subdivide.rs:117` clones `primitive_attributes` after
`subdivide.rs:103` has emitted four triangles per input triangle, leaving a lane of length N on
a mesh with 4N primitives, and `crates/solarxy-kernel/src/delete.rs` has the same shape. Nothing
declares the invariant that a lane's length equals its domain's element count, and nothing
checks it. And the normals duplication between the fixed field and the reserved `N` lane needs
a stated precedence rule.

The reserved-name type contract stays a convention enforced per consumer. Making it a checked
contract at cook commit is a candidate in
[09-evolution-and-roadmap.md](../09-evolution-and-roadmap.md), and is a smaller piece of work
than adding a domain.

This ADR is reversible. If a future capability genuinely needs per-corner or per-set data, this
is superseded rather than worked around, because working around it means encoding a domain in
lane names, which is the worst of both models.
