//! Bounding volume hierarchy for Solarxy's ray queries.
//!
//! Pure CPU: no wgpu, no filesystem, no threads, no graph awareness. The crate
//! exists as its own workspace member for one reason, and it is a boundary
//! reason rather than a tidiness one. The builder has to run inside the import
//! Web Worker, which hosts a headless wasm instance with no GPU at all. Living
//! in `solarxy-renderer` would drag wgpu into that worker; living in
//! `solarxy-kernel` would put a rendering acceleration structure inside the
//! geometry kernel. Depending on `solarxy-core` alone keeps both clean.
//!
//! Contents:
//!
//! - [`BvhNode`] — the 32-byte node the traversal kernel reads, with the
//!   left-child-is-next and leaf-flag packing that carries the topology in the
//!   eight bytes that are not bounds.
//! - [`Bvh`] — a binned-SAH BVH2 over triangles ([`Bvh::build_triangles`]) or
//!   over instance boxes ([`Bvh::build_tlas`]), plus [`Bvh::to_gpu_arrays`]
//!   for upload.
//! - [`Bvh::intersect_triangles`] / [`Bvh::occluded_triangles`] — the CPU
//!   traversal the WGSL kernel is a twin of, and the reference a parity corpus
//!   holds it to.
//! - [`Bvh::intersect_instances`] / [`Bvh::occluded_instances`] — the same two
//!   queries over the two-level structure, transforming the ray into each
//!   instance's object space.
//! - [`transfer`] — [`transfer::pack`] and [`transfer::unpack`], because on web
//!   the build runs in the import worker's own wasm heap and a finished
//!   hierarchy has to cross as bytes.
//! - [`corpus`] — the deterministic meshes and ray set every comparison of
//!   those implementations draws from, here rather than in a test so the
//!   comparison that lives in another crate draws from the same one.
//!
//! Two invariants hold across the whole crate and the shader that mirrors it:
//! a node's left child is always the next node, and tree depth stays under
//! [`build::MAX_DEPTH`] so a fixed [`traverse::TRAVERSAL_STACK_SIZE`] stack
//! cannot overflow. Both are enforced by the builder rather than assumed.
//!
//! Everything is written on `[f32; 3]` rather than a math crate's vector type,
//! because the traversal has a WGSL twin that must match it term for term.

#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::inline_always,
    clippy::many_single_char_names,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_range_loop,
    clippy::return_self_not_must_use,
    clippy::similar_names,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args
)]

pub mod bounds;
pub mod build;
pub mod corpus;
pub mod node;
pub mod transfer;
pub mod traverse;

pub use bounds::Bounds;
pub use build::{Bvh, BvhStats, GpuArrays, MAX_DEPTH, MAX_LEAF_SIZE, TARGET_LEAF_SIZE};
pub use corpus::CorpusRay;
pub use node::{BvhNode, LEAF_FLAG};
pub use traverse::{InstanceHit, Instanced, TRAVERSAL_STACK_SIZE, TriangleHit};
