//! Parametric geometry kernel for the Solarxy node engine.
//!
//! Pure CPU: no wgpu, no filesystem, no graph awareness. Every public
//! function operates on plain slices and values so it is trivially testable
//! and later parallelizable. The node engine (`solarxy-graph`) calls into
//! this crate from its cook bodies; nothing here knows what a node is.
//!
//! Contents:
//!
//! - [`GeometrySet`] / [`KernelMesh`] — the payload type behind the graph's
//!   `Geometry` wires, plus conversions to the renderer contract
//!   (`solarxy_core::scene::CookedGeometry`) and the loader/validation type
//!   (`solarxy_core::geometry::RawModelData`).
//! - [`primitives`] — the seven parametric generators (box, sphere,
//!   cylinder, cone, plane, torus, torus knot) emitting positions, normals,
//!   and UVs.
//! - [`transform`] — Euler/TRS matrix composition and matrix baking into
//!   point positions with inverse-transpose normal handling.
//! - [`merge`] — `GeometrySet` concatenation with content-hash material
//!   deduplication.
//! - [`scatter`] / [`copy`] — the procedural pair: seeded area-weighted
//!   surface sampling into a point cloud, and template instancing onto
//!   points ([`rng`] holds the shared avalanche-hash draws).
//!
//! Winding + orientation invariant (frozen for the whole engine): triangles
//! are counter-clockwise front-facing in a Y-up right-handed coordinate
//! system, and normals point out of the enclosed volume. `compute_normals`,
//! the validate node's flipped-normals check, and transform's
//! inverse-transpose all assume it.

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

pub mod array;
pub mod attribute_ops;
pub mod bounds_geo;
pub mod copy;
pub mod deform_ops;
pub mod delete;
pub mod edges_to_geo;
mod error;
pub mod merge;
pub mod mirror;
pub mod points_from_geo;
pub mod primitives;
pub mod rng;
pub mod scatter;
pub mod set;
pub mod subdivide;
pub mod transfer;
pub mod transform;
pub mod uv_project;

pub use error::KernelError;
pub use set::{AttributeData, AttributeDomain, AttributeMap, GeometrySet, KernelMesh, reserved};
