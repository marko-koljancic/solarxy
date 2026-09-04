//! The Solarxy web shell: the `wasm-bindgen` boundary and WebGPU host.
//!
//! This crate is the only one permitted binary-style error looseness at the
//! boundary (`JsError`). It owns the canvas surface, the wgpu device/queue,
//! the `solarxy_renderer` renderer and scene objects, and the
//! `solarxy_graph` engine, and exposes the `SolarxyApp` class the React
//! frontend drives: dispatch a `Command`, get an `EventBatch`; call `frame`
//! to cook under a budget, apply the scene delta, and render. Cooked
//! geometry never crosses into JavaScript; only commands, events, snapshots,
//! and asset bytes do.
//!
//! Everything of substance is `cfg(target_arch = "wasm32")`: on native the
//! crate is an (almost) empty library so `cargo build --workspace` stays
//! green without a wasm toolchain, while the real host is exercised by the
//! wasm build and `wasm-bindgen-test`.

#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::unused_self
)]

#[cfg(target_arch = "wasm32")]
mod app;

// The gizmo drag solver, the attribute-visualization state, the label packing
// and the display-defaults parsing used to live here, outside the wasm cfg, so
// that native CI would run their tests rather than leaving them to a wasm-only
// build. They now live in `solarxy-host` for the same reason and one more: the
// desktop shell needs them too, and a module in a crate named for the web was
// never going to be where it reached for them.

// The no-movement guard on a locked look-through camera commit. Both targets,
// unlike the host that calls it, so native CI runs its tests; it cannot move
// to `solarxy-host`, which has no `solarxy-graph` dependency by design.
mod camera_commit;

// What a still asks the tracer for, from what the render node says. Both
// targets for the same reason as above, and it cannot move to `solarxy-host`
// for the same reason either.
mod trace_settings;

// The traversal parity probe: the browser half of the check that the WGSL
// traversal agrees with its CPU twin. Feature-gated, because the shipped
// artifact has no reason to carry a diagnostic.
#[cfg(all(target_arch = "wasm32", feature = "pt-probe"))]
mod pathtrace_probe;

#[cfg(target_arch = "wasm32")]
pub use app::SolarxyApp;

#[cfg(all(target_arch = "wasm32", feature = "pt-probe"))]
pub use pathtrace_probe::{BsdfProbeCheck, PathtraceProbe};

/// Installs the panic hook that routes Rust panics to `console.error`, so a
/// boundary panic is legible in the browser devtools. Called once by JS
/// before constructing a `SolarxyApp`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}
