//! Shared host orchestration for the Solarxy shells.
//!
//! Both shells drive the same renderer through the same sequence: build a
//! per-pane camera, write the pane's uniforms, run the pass chain, composite
//! into the pane's rect. That sequence was written twice, once in
//! `solarxy-app`'s `state/render.rs` plus `state/update.rs` and once in
//! `solarxy-web`'s `app.rs`, whose own header said so. This crate is the one
//! copy.
//!
//! # What this crate is not
//!
//! **There is no renderer trait here, and no dynamic dispatch.** This is
//! deduplication validated by two callers of one implementation, not an
//! abstraction over two backends. A trait would have to be designed against a
//! backend that does not exist yet, then changed when the real one arrives,
//! refactoring every host twice. When a second *implementation* exists, it
//! shapes the trait.
//!
//! **It does not depend on `solarxy-graph`.** A host crate sits under both
//! shells, and the desktop shell has no engine yet, so an engine dependency
//! here would arrive in `solarxy-app` a release early through the back door.
//! Where the orchestration needs something the engine owns, the shell passes
//! it in as plain data — see [`gizmo::GizmoPose`], which is the drag solver's
//! whole view of its target.
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
pub mod lighting;
pub mod pane;
pub mod view;

pub use cameras::{depth_bounds, ensure_pane_cameras};
pub use lighting::{active_ibl, rebuild_light_bind_group};
pub use pane::{
    PaneComposite, PaneScene, PaneUniforms, composite_and_submit, render_3d_passes,
    render_overdraw_pane, setup_pane_lighting, write_inspection_block, write_pane_uniforms,
    write_wireframe_params,
};
pub use view::HostViewState;
