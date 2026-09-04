//! The Solarxy studio core: the headless node-graph engine.
//!
//! One Rust core, two shells: this crate owns the document (nodes, edges,
//! params, subflows), the topology (cycle rejection, memoized topological
//! sorts), the cook engine (dirty tracking, keep-last-good, budgeted
//! resumable cooking, async generation guards), the node registry (typed
//! ports with the coercion matrix, declarative param schemas, bypass,
//! versioning), the 23 MVP node types, and the transactional undo stack.
//!
//! It never sees wgpu: cooked geometry leaves through
//! `solarxy_core::scene::SceneDelta`, the sole contract with the renderer.
//! Engine semantics follow the Minimystix executable specification (cook
//! orchestration, keep-last-good, generation guards) with the deliberate
//! catalog deltas: enforced typed ports, Int/Float split, per-node
//! versioning and migration, absolute matrix-bake transforms, and undo.
//!
//! The node-system contract (typed ports, coercion, declarative param
//! schemas, per-node versioning) is documented on the types themselves.

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
pub mod cook;
pub mod document;
pub mod engine;
mod error;
pub mod expr;
pub mod migration;
pub mod model_document;
pub mod naming;
pub mod nodes;
pub mod params;
pub mod previews;
pub mod reference;
pub mod refs;
pub mod registry;
pub mod review;
pub mod runtime;
pub mod topology;

pub use engine::{Command, Engine, EngineError, EngineEvent, EventBatch};
pub use error::GraphError;
pub use nodes::{builtin_descriptors, builtin_registry};
