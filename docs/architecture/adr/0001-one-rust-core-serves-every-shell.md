# 0001. One Rust core serves every shell

- **Status**: accepted
- **Date**: 2026-09-07

## Context

Solarxy delivers through surfaces that share no view layer: a browser application
(React and TypeScript over a WebAssembly build, rendering through WebGPU), a native desktop
window (winit, egui, wgpu), and terminal surfaces (the analyze view and the render dashboard).

Several things have to mean exactly one thing across all of them: what a document is, what a
cook produces, what a material parameter means, and what a pixel should be. If each surface
answered those questions for itself, the answers would diverge, and the divergence would be
discovered by a user comparing two pictures rather than by a compiler.

The workspace is 15 Cargo members. The browser entry point is `solarxy-web`, declared
`crate-type = ["cdylib", "rlib"]`, and the entire WebAssembly-only host sits behind one
conditional at `crates/solarxy-web/src/lib.rs:30`. Below that gate, nothing is
platform-conditional: `crates/solarxy-renderer` and `crates/solarxy-host` together carry
zero `target_arch` conditionals, and the whole workspace carries 18 platform conditionals in
`crates/**/src` and `src/`. Portability is bought with Cargo feature negation, chiefly
`default-features = false` dropping `std-fs` and `serialization`, rather than with `cfg`.

## Options considered

### Option A: one Rust core compiled for every target

The document engine, the geometry kernel, the scene file, the renderer and the render host
are one codebase. Each shell owns its view layer, its input handling and its platform I/O,
and nothing else.

### Option B: a per-platform engine

A TypeScript engine for the browser, where the ecosystem and the debugging story are
strongest, and the Rust one for the desktop and the terminal.

This costs two definitions of a cook. Every node type, every coercion rule, every migration
and every numeric edge would exist twice, and the two would be held together by test
discipline rather than by the compiler. The failures that discipline misses are exactly the
ones that matter here: a document that opens differently in two places.

### Option C: browser only, with the desktop as a wrapper around the web build

One implementation, one shell, and the desktop reduced to a container.

It gives up the things the desktop exists for: a native GPU adapter without the browser's
capability floor, a real filesystem, and a headless render command that a build system can
call. It also makes the browser's 32-bit address space the ceiling for every user.

## Decision

There is one Rust core, and it compiles for both `wasm32-unknown-unknown` and native. A shell
owns its view layer, its input and its platform I/O. It does not own document semantics,
geometry, shading, or the scene format.

## Consequences

Every crate on the browser path must stay free of filesystem, threads and wall-clock reads.
`solarxy-kernel`, `solarxy-bvh`, `solarxy-imaging` and `solarxy-graph` contain none of those.
`solarxy-core` gates every filesystem site behind its `fs` or `serialization` features
(`crates/solarxy-core/src/lib.rs`), and `solarxy-scenefile` reads and writes only in-memory
buffers (`crates/solarxy-scenefile/src/archive.rs`).

The same rule reaches function signatures. `solarxy-host` compiles for the browser and has no
`Instant`, so the tiled still job takes the current time as a parameter at every step rather
than reading a clock, and each shell supplies its own source.

Wasm compatibility is a property every consumer has to remember rather than one the build
enforces: `solarxy-graph`, `solarxy-renderer`, `solarxy-host` and `solarxy-web` all take
`solarxy-core` and `solarxy-formats` with `default-features = false`, and one forgotten edge
would pull `dirs` and the path-based loaders into the browser build.

It makes the browser's constraints visible in shared code. The browser caps a float still at
16,000,000 pixels because an allocation failure there takes the tab; the desktop has no such
ceiling, and the cap deliberately lives in the web shell rather than in `solarxy-host`.

What this decision does not buy, and is often assumed to: shared application behaviour.
`solarxy_graph::Command` has 35 variants and the desktop shell dispatches two of them. A
shared core made shared semantics possible and did not by itself produce a shared application
layer. That is [0012](0012-shared-application-layer-is-a-new-crate.md).

Enforcement: Cargo, for the compilation half. The reduced feature set that the browser
actually ships is exercised by exactly one step,
`cargo clippy -p solarxy-web --target wasm32-unknown-unknown` at
`.github/workflows/ci.yml:200`, plus the wasm build at `ci.yml:204`. Every other build and
test invocation in CI is `--all-features`, where Cargo's feature unification turns the gated
modules back on, so that one step is the whole guard.

## Notes

The measured shape of the split: about 159k lines of Rust under `crates/*/src` against about
34k lines of TypeScript and TSX under `web/src`, of which roughly 3.7k is the public roadmap
page rather than the application.
