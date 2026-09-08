//! Shared host orchestration for the Solarxy shells.
//!
//! Both shells drive the same renderer through the same sequence: build a
//! per-pane camera, write the pane's uniforms, run the pass chain, composite
//! into the pane's rect. That sequence was written twice, once in
//! `solarxy-app`'s `state/render.rs` plus `state/update.rs` and once in
//! `solarxy-web`'s `app.rs`, whose own header said so. This crate is the one
//! copy.
//!
//! # Where the backend trait sits
//!
//! Through 0.8.2 this crate deliberately had no renderer trait: one
//! implementation with two callers is deduplication, and a trait designed
//! against a backend that does not exist yet gets redesigned when the real one
//! arrives, refactoring every host twice. 0.9.0 is when the second
//! implementation shows up, so the trait was written then, from three call
//! sites rather than one guess.
//!
//! **It is declared in `solarxy_renderer::backend`, not here**, so a backend
//! living in the renderer can implement it without depending on this crate.
//! What lives here is [`RasterBackend`], the implementation that wraps
//! [`pane::encode_pane_passes`], because that is where the pass chain is.
//!
//! # What this crate is not
//!
//! **It does not depend on `solarxy-graph`.** The engine and the renderer
//! meet only at `solarxy_core::scene::SceneDelta`, and this crate sits on
//! the renderer's side of that line. Both shells hold an engine now; what
//! keeps the boundary real is that neither hands it to this crate. Where the
//! orchestration needs something the engine owns, the shell passes it in as
//! plain data — see [`gizmo::GizmoPose`], which is the drag solver's whole
//! view of its target.
//!
//! # What belongs here
//!
//! The section below says how a shared item is written. It did not say what
//! qualifies as one, and that omission is why this extraction happened twice:
//! 0.8.2 moved the pass chain, and the orchestration one layer above it was
//! written twice again afterwards, by people following a shape rule that had
//! nothing to say about membership.
//!
//! **The test.** A behaviour belongs here when all four hold.
//!
//! 1. **Both graphical shells need it**, or one needs it and the other is
//!    about to. A behaviour with one consumer and no second one coming is not
//!    shared, it is misplaced.
//! 2. **It can be expressed without naming a document.** If the signature
//!    wants a node, a parameter, a command or a graph, it belongs above this
//!    crate rather than in it. Passing the same information as plain data is
//!    the usual fix and is what [`gizmo::GizmoPose`] is.
//! 3. **It can be expressed without naming a widget.** No egui, no DOM, no
//!    `web_sys`, no winit. A function that takes a rect and returns a rect is
//!    fine; one that takes a `Ui` is a shell's.
//! 4. **The two shells want the same answer.** Where they want different
//!    answers from the same arithmetic, share the arithmetic and leave the
//!    policy at the call site, which is the rule the shape section already
//!    states.
//!
//! **What does not belong**, stated so a reader stops rather than tries: a
//! clock, because this crate compiles for the browser and has no `Instant`; a
//! device or an adapter, because three callers want three request policies; a
//! surface, because acquiring one is a shell's job; logging; anything that
//! decides what a menu item does.
//!
//! **Single-consumer code here is legitimate and is not an oversight.**
//! `gizmo.rs`, `attr_viz.rs` and `attr_labels.rs` are 2,574 lines read only by
//! the browser today. They were placed here for 0.10.0, which lights them up
//! from the desktop, and rule 1's second clause is written for exactly them. A
//! tidy-up that moves them back would be undoing the work rather than
//! finishing it.
//!
//! # The exceptions, and why each one resists the rule
//!
//! Named as exceptions so a later reader does not conclude the rule is wrong
//! and collapse them.
//!
//! **The render-settings translation exists three times and cannot be shared.**
//! `trace_settings_for` is written once per shell, in `solarxy-app`,
//! `solarxy-web` and `solarxy-render`. It reads a document to produce renderer
//! settings, so it needs both sides of a boundary that refuses it in both
//! directions: this crate must not see the engine, and the engine must not see
//! the renderer. The mitigation is that each copy destructures `RenderSettings`
//! exhaustively, so a field added to the settings stops all three compiling
//! until each says what it does with it. That is a compiler tripwire standing
//! in for a shared home, and it exists because a camera's aperture once
//! resolved correctly out of a document and reached no renderer for a whole
//! release.
//!
//! **The still job takes its clock rather than reading one.** `StillCtx`
//! carries `now_ms`, supplied by the caller at every `advance` from five sites.
//! It is a field rather than an argument precisely so a caller that forgets it
//! does not compile.
//!
//! **The browser's float-still ceiling stays in the browser.**
//! `MAX_FLOAT_STILL_PIXELS` exists because wasm is a 32-bit address space where
//! an allocation failure takes the tab. It is a platform limit its shell
//! imposes on itself, not a property of a still, and applying it here would
//! refuse a render the desktop makes comfortably.
//!
//! # Deciding where a new host behaviour goes
//!
//! - Does it name a node, a parameter or a command? Then it is the application
//!   layer's, not this crate's.
//! - Does it name a widget, a window or an event loop? Then it is a shell's.
//! - Do both shells want it, or will the second want it this release? If
//!   neither, leave it where it is and write down why.
//! - Do they want the same answer, or the same arithmetic and different
//!   answers? The second means the body moves and the guard stays.
//! - Can it be written as a free function over borrowed values? If it needs a
//!   host type to hang off, the state it wants is probably the shell's.
//!
//! # How the shared functions are shaped
//!
//! Free functions over explicit borrowed parameters, never methods on a host
//! type. Each shell keeps its own state layout — both hold `SceneObjects` and
//! `SceneEnvironment` as siblings, and the desktop additionally keeps its
//! file-loaded model in an `Option<ModelScene>` beside them — and builds the
//! parameters from whatever it has.
//!
//! Where one shell has a capability the other does not, the parameter is an
//! `Option` whose `None` **already means** what the shell without it needs,
//! rather than a flag the function branches on. A desktop pane passes no
//! selection and gets no highlight and no outline; it passes no grid plane and
//! the grid-plane offset is left unwritten. The point is that the absent path
//! emits the identical GPU command stream it did before this crate existed,
//! which is what makes the extraction provable rather than merely plausible.
//!
//! Where the two shells disagree on *policy* rather than arithmetic — which
//! panes get a synthesized light rig, when a shadow pass runs — the guard
//! stays at the call site and only the body is shared.
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::default_trait_access,
    clippy::fn_params_excessive_bools,
    clippy::many_single_char_names,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unreadable_literal,
    clippy::wildcard_imports
)]

pub mod attr_labels;
pub mod attr_viz;
pub mod cameras;
pub mod compare;
pub mod display_defaults;
pub mod gizmo;
pub mod headless;
pub mod lighting;
pub mod pane;
pub mod passes;
pub mod raster;
pub mod still;
pub mod view;
pub mod visualization;

pub use cameras::{depth_bounds, ensure_pane_cameras};
pub use compare::{ImageDifference, compare_rgba8};
pub use lighting::{
    EnvironmentApplied, active_ibl, apply_scene_environment, build_bounds_env,
    rebuild_light_bind_group,
};
pub use pane::{
    EncodedPane, PaneComposite, PaneScene, PaneUniforms, apply_viewer_rig, composite_and_submit,
    encode_pane_passes, render_3d_passes, render_overdraw_pane, setup_pane_lighting,
    write_inspection_block, write_pane_uniforms, write_wireframe_params,
};
pub use passes::{AovKind, PassKind, PassSelector};
pub use raster::RasterBackend;
pub use still::{StillCtx, StillRenderJob, StillSpec, StillStep};
pub use view::HostViewState;
