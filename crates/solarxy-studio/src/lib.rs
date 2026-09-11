//! The shared interface derivation: the rules that decide how a node and
//! its parameters are **presented**, written once and read by both shells.
//!
//! Which parameter tabs a node has, whether a group still earns one, how a
//! node summarises itself, where the palette opens, how a wire type reads,
//! how the scene tree folds, how an attribute cell formats. These lived in
//! the browser as a set of modules deliberately written without a browser
//! dependency so they could be unit tested, and the desktop needed every
//! one of them. Porting would have produced a second implementation of
//! behaviour that must agree forever; this crate is the other answer.
//!
//! # What belongs here, and what does not
//!
//! **Presentation, not semantics.** A fact about what a node *is* belongs
//! on the registry resolution path in `solarxy-graph`, where every
//! consumer already reaches and where a documentation generator can use it
//! without linking a surface. Conditional parameter visibility is the
//! worked example: the `show_if` evaluator is a fact about a node type and
//! lives in `solarxy_graph::registry::visibility`, while the tab strip
//! that hides an emptied group is presentation and lives here.
//!
//! **Presentation, not drawing.** A rule may return which silhouette, or
//! which palette role. It may not return a rectangle, a font, or a colour
//! it authored itself, because either would have picked a toolkit. The two
//! shells draw the same answer with different primitives, and that is the
//! division.
//!
//! # Constraints this crate inherits
//!
//! One of its consumers is a WebAssembly build, so it takes no clock, no
//! filesystem on any required path, no threads, and no windowing or
//! rendering toolkit. It also takes no `wasm-bindgen`: the browser reaches
//! these rules through `solarxy-web`, which re-exports them across the
//! boundary it already has, in the shape of the read-only queries it
//! already serves.

#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
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
    clippy::pub_underscore_fields,
    clippy::return_self_not_must_use,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unreadable_literal,
    clippy::used_underscore_binding,
    clippy::wildcard_imports
)]

pub mod assets;
pub mod attributes;
pub mod expression;
pub mod node;
pub mod palette;
pub mod params;
pub mod tree;
pub mod types;
