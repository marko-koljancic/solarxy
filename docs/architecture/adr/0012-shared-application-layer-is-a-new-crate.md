# 0012. The shared headless application layer is a new crate above the engine and the render host

- **Status**: accepted as direction; the crate itself is not yet created
- **Date**: 2026-09-07

## Context

Solarxy ships two graphical shells that share no view layer. `solarxy-app` is winit plus
egui; the browser shell is React and TypeScript over a WebAssembly build of the same Rust.
Parity between them therefore cannot mean a shared UI. It can only mean a shared headless
application layer in Rust that both shells render.

That layer does not exist, and the current dependency graph forbids it from appearing where
you would first look for it.

`solarxy-host` is the only crate both graphical shells share. Its normal dependencies are
`solarxy-core`, `solarxy-kernel` and `solarxy-renderer`, and it deliberately has no edge to
`solarxy-graph` in any dependency kind. That non-dependency is what keeps the engine and the
renderer genuinely separate, since they otherwise meet only at
`solarxy_core::scene::SceneDelta`. So the shared crate is structurally a rendering host and
cannot hold anything that references a document, a node, or a command.

The result is measurable. `solarxy_graph::Command` has 35 variants. The browser shell drives
effectively all of them. `solarxy-app` production code dispatches two: a `SetSelection` at
`crates/solarxy-app/src/state/intents.rs:95` and a `SetParam` at the same file's line 864.
Everything above the engine, meaning menus, keymap, dock and workspace arrangement, modals,
toasts, parameter widget selection, parameter visibility, preferences, autosave, save and
open, copy and paste, and the export flows, is written once per shell.

The duplication this produces is not hypothetical. `render_pane` is implemented separately in
`crates/solarxy-app/src/state/render.rs:160` and `crates/solarxy-web/src/app.rs:5458` while
`crates/solarxy-app/src/state/render.rs:2` claims it delegates to a `solarxy_host::render_pane`
that does not exist. The still-render pump loop is written three times. `denoise_settings_for`
is byte-identical in three crates.

## Options considered

### Option A: a new crate above both the engine and the render host

A crate depending on `solarxy-graph` and `solarxy-host`, owning the application session:
document lifecycle, selection, tool and gizmo mode, pane and dock model, the menu and command
model, keymap, and the application-level undo grouping. Both graphical shells and the terminal
surfaces depend on it and render it.

Costs a new workspace crate, which is a gated addition here, and a migration that moves
behaviour out of two shells at once.

### Option B: widen `solarxy-render` into the layer

`solarxy-render` already depends on `solarxy-core`, `solarxy-formats`, `solarxy-graph`,
`solarxy-host` and `solarxy-renderer`, so it is the only existing crate positioned correctly.
Widening its charter costs no new crate.

It costs clarity instead. The crate is named and documented as a headless rendering library,
1,605 of its 2,081 lines are one `lib.rs`, and its name already collides confusingly with
`solarxy-renderer`, one letter apart at a different layer. A crate that is both the render
command and the application layer would be harder to reason about than the problem it solves.

### Option C: let `solarxy-host` depend on the engine

Fewest crates, and the application layer lands where the shared code already is.

It dissolves the one boundary Cargo currently enforces end to end. The engine and renderer
separation is not merely tidy: it is what lets the engine compile without wgpu, what lets the
import worker run a GPU-free WebAssembly instance, and what keeps the cook testable without a
device. Trading it for crate count is a bad trade.

## Decision

The shared headless application layer is a new crate, working name `solarxy-studio`,
depending on `solarxy-graph` and `solarxy-host`. Both graphical shells become renderers of it.
`solarxy-host` keeps its render-only charter and its refusal to depend on `solarxy-graph`.

A command issued from React and the same command issued from egui take the identical path:
the shell translates a gesture into an application intent, hands it to `solarxy-studio`, and
renders whatever state comes back. Neither shell interprets the document.

## Consequences

Parity stops being a porting exercise and becomes a migration. Each behaviour moved into the
new crate is available to both shells at once, which is the only way a single maintainer
reaches parity across two view layers.

It makes some things harder. The new crate must stay free of view concerns or it becomes a
third shell, and there is no compiler check for that. It must stay wasm-clean, since one of
its consumers is a WebAssembly build. And it inherits the hardest part of the current design,
which is that view state is host-owned today and document state is engine-owned, so every
moved behaviour has to decide which side of that line it sits on.

[adr/0016](0016-interface-derivation-splits-between-registry-and-application-layer.md) settles
what the crate holds first, and it is not session state: the shared interface derivation's
presentation half lands here before any of the behaviours listed above, because 0.10.0 has a
second consumer waiting on it. The semantics half goes to the registry instead. That refines this
decision rather than reversing it: the crate, its name and its position are unchanged.

The crate does not exist yet. Creating a workspace crate is a gated addition under this
repository's working agreement, so this ADR records the direction and
[09-evolution-and-roadmap.md](../09-evolution-and-roadmap.md) sizes the migration. Until that
approval, no code moves.

Enforcement: the allow-matrix in [05-boundaries-and-contracts.md](../05-boundaries-and-contracts.md)
names `solarxy-graph` as forbidden to `solarxy-host` and permitted to `solarxy-studio`. Nothing
enforces it mechanically today. The dependency allow-list assertion proposed in
[09](../09-evolution-and-roadmap.md) is what would.
