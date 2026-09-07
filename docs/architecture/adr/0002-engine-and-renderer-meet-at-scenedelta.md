# 0002. The engine and the renderer never depend on each other; they meet at a scene delta

- **Status**: accepted
- **Date**: 2026-09-07

## Context

A node-graph application has an obvious shape that is also a trap: the engine holds the
document, the renderer draws it, and the engine calls the renderer. Once that call exists the
engine has a GPU dependency, the cook cannot be tested without a device, and any code that
wants to evaluate a graph has to bring a windowing stack with it.

Solarxy needs the opposite property in three concrete places. The cook is tested in a plain
`cargo test` with no adapter. The browser's import worker is a second, headless WebAssembly
instance of the same binary with no `wgpu` device at all, and it parses models and builds
acceleration structures inside it. And the headless render command has to bring a device up
with no surface.

So the two halves were separated before either was finished, and the separation was written
into a type: `crates/solarxy-core/src/scene.rs`, whose header states the rule directly, that
"the engine and the renderer never depend on each other; they communicate exclusively through
`SceneDelta` values built from these types" and that everything in the module is plain data
with "no wgpu, no filesystem, no windowing".

## Options considered

### Option A: no edge in either direction, one plain-data contract between them

`solarxy-graph` produces a `SceneDelta`; `solarxy-renderer` consumes one. Neither names the
other. The contract lives in `solarxy-core`, which both already depend on.

### Option B: the engine depends on the renderer

The natural reading of "the engine draws the scene". It is one fewer type and one fewer
translation step.

It costs the three properties above outright. `solarxy-graph` would link `wgpu`, so the cook
tests would need a device, the GPU-free import worker would carry the whole renderer into a
second WebAssembly instance, and every consumer of the graph would inherit a graphics stack
whether it drew anything or not.

### Option C: the renderer depends on the engine

The renderer reads the document directly and decides for itself what changed, which removes
the delta and its diffing entirely.

It makes the renderer a consumer of node semantics: cooked geometry, parameter resolution,
containers and contexts would all have to be understood by the drawing layer, and a document
model change would be a renderer change. It also forecloses a second producer, which is
exactly what the file loader on the desktop and the headless command are.

## Decision

`solarxy-graph` and `solarxy-renderer` have no dependency on each other in any dependency
kind. They meet only at `solarxy_core::scene::SceneDelta`, which is plain data.

`solarxy-host`, the orchestration both graphical shells drive, sits on the renderer's side of
that line and likewise takes no dependency on `solarxy-graph`. Where it needs something the
engine owns, the shell passes it in as plain data.

## Consequences

The no-edge claim holds today in every dependency kind, and that was checked rather than
assumed. `crates/solarxy-graph/Cargo.toml` declares `solarxy-core`, `solarxy-formats`,
`solarxy-imaging`, `solarxy-kernel` and `solarxy-scenefile`, with no `[dev-dependencies]` and
no `[target.*]` table at all. `crates/solarxy-renderer/Cargo.toml` declares `solarxy-bvh`,
`solarxy-core` and `solarxy-formats`, with `pollster`, `anyhow` and `ab_glyph` as
dev-dependencies. `solarxy-host` takes `solarxy-core`, `solarxy-kernel` and `solarxy-renderer`
normally and `solarxy-bvh` and `solarxy-formats` for tests, and its manifest carries the rule
as a comment where the missing dependency would be
(`crates/solarxy-host/Cargo.toml:53`, "Deliberately ABSENT: `solarxy-graph`").

What it buys is specific. The engine compiles without `wgpu`. The cook runs in a test with no
adapter. The import worker is a headless instance of the same WebAssembly binary that has no
device, and it runs the model parse and the hierarchy build inside its own heap. On the web
both halves compile into one instance, so a cooked buffer moves from engine to renderer as an
`Arc` pointer handoff and cooked geometry never crosses into JavaScript.

What it costs is a shared home for anything that is genuinely about both. The engine declares
a two-valued render-engine choice and the host declares its own copy of the same two values,
because the host may not name the engine's type; three shells then map between them by hand,
and `solarxy-cli` carries a third copy for its argument parsing. Those matches are exhaustive
with no wildcard arm, so a third engine halts compilation rather than diverging silently, but
it is four edits. `solarxy-core` already hosts `gizmo` for exactly this reason and was not
used here.

The separation also constrains where the shared application layer can live, which is the
whole argument of [0012](0012-shared-application-layer-is-a-new-crate.md).

Enforcement: Cargo, and it is the one architecture invariant in the workspace that the build
system enforces end to end rather than prose. The three crates that legitimately hold both
sides are the shells: `solarxy-app`, `solarxy-web` and `solarxy-render`.

## Notes

The contract's own incrementality is worth knowing when reading it: the engine re-lowers the
whole scene on every call, and the renderer's diff is `Arc` pointer identity
(`crates/solarxy-renderer/src/scene_objects.rs`). Pointer equality is sound here only because
cooked buffers are immutable and shared across frames, which is a property of the delta types
rather than of the renderer.
