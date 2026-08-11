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
pub mod display_defaults;
pub mod gizmo;
pub mod headless;
pub mod lighting;
pub mod pane;
pub mod raster;
pub mod still;
pub mod view;

pub use cameras::{depth_bounds, ensure_pane_cameras};
pub use lighting::{active_ibl, rebuild_light_bind_group};
pub use pane::{
    EncodedPane, PaneComposite, PaneScene, PaneUniforms, apply_viewer_rig, composite_and_submit,
    encode_pane_passes, render_3d_passes, render_overdraw_pane, setup_pane_lighting,
    write_inspection_block, write_pane_uniforms, write_wireframe_params,
};
pub use raster::RasterBackend;
pub use still::{StillCtx, StillRenderJob, StillSpec, StillStep};
pub use view::HostViewState;
