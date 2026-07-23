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

// Deliberately NOT wasm-gated: the gizmo's hit-testing and drag solving are pure
// math with no browser dependency, so keeping them native-visible means native
// CI runs their tests instead of leaving them to a wasm-only build.
pub mod gizmo;

// Same convention: the attribute-viz state and its ramp/clamp math are
// pure data, tested natively.
pub mod attr_viz;

#[cfg(target_arch = "wasm32")]
pub use app::SolarxyApp;

/// Installs the panic hook that routes Rust panics to `console.error`, so a
/// boundary panic is legible in the browser devtools. Called once by JS
/// before constructing a `SolarxyApp`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}
