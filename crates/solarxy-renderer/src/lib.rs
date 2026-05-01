//! All wgpu state for Solarxy: render pipelines, bind groups, shaders, IBL,
//! SSAO, bloom, shadow, composite, camera, per-frame draw orchestration
//! ([`frame`]), and per-model GPU scene state ([`scene`]).
//!
//! This crate has **no winit, no egui** — input and UI live in `solarxy-app`.
//! The app drives a [`frame::Renderer`] each frame; everything below that is
//! implementation detail.
//!
//! # Render pass order (per pane in split mode)
//!
//! 1. Shadow → 2. `GBuffer` (if SSAO) → 3. Background → 4. Main PBR →
//! 5. Floor → 6. Wireframe overlays → 7. Grid + normals + gizmo →
//! 8. Validation overlay → 9. SSAO + Bloom → 10. Composite (tone-map) →
//! 11. UV map passes (UV-mode panes) → 12. egui (in `solarxy-app`).
//!
//! # GPU uniform buffer convention
//!
//! `*Uniform` structs are `#[repr(C)]` with explicit `_pad` fields sized to
//! hit WGSL's 16-byte struct-size alignment. Several have
//! `const _: () = assert!(std::mem::size_of::<T>() == N);` size guards —
//! when extending a uniform, repack padding and update the assert in lockstep
//! with the matching shader.
//!
//! # Bind group layouts
//!
//! [`bind_groups::BindGroupLayouts`] is the single source of truth for every
//! layout. All entries use `min_binding_size: None` so growing a uniform is
//! layout-invisible.
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
    clippy::pub_underscore_fields,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unreadable_literal,
    clippy::used_underscore_binding,
    clippy::wildcard_imports
)]

pub mod bind_groups;
pub mod bloom;
pub mod camera;
pub mod camera_state;
pub mod composite;
pub mod frame;
pub mod geometry;
pub mod ibl;
pub mod light;
pub mod material;
pub mod model;
pub mod pipeline_builder;
pub mod pipelines;
pub mod resources;
pub mod scene;
pub mod shadow;
pub mod ssao;
pub mod texture;
pub mod uv_camera;
pub mod validation;
pub mod visualization;
