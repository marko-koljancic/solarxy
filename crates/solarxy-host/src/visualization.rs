//! The per-mesh visualization overlays, built once for both shells.
//!
//! Two overlays read this: the normal arrows and the per-mesh bounds boxes.
//! Both are drawn from buffers baked into [`VisualizationState`], flattened in
//! the renderer's own draw order, because the renderer zips its segments
//! against the flattened scene meshes.
//!
//! # Why it reads the cooked geometry rather than the GPU meshes
//!
//! A GPU mesh carries no per-vertex normals a reader can get at, and the
//! CPU mirror beside it carries positions and indices only. The cooked
//! geometry has both, is GPU-free, and lives in the core crate, which is what
//! lets this be shared: the browser built the same aggregate off the engine's
//! `display_geometries`, and this crate must never see the engine.
//!
//! # Two orderings that have to agree
//!
//! Hidden objects are not drawn, and a cooked mesh with nothing in it gets no
//! GPU mesh, so walking every cooked mesh of every object would emit segments
//! the draw list has no entry for and silently shift every overlay after the
//! first empty one onto the wrong mesh. The visible filter and the
//! `raw_to_gpu` skip below are what keep the two lists in step.

use cgmath::{InnerSpace, Matrix, Matrix3, Matrix4, Point3, SquareMatrix, Transform, Vector3};

use solarxy_core::AABB;
use solarxy_core::geometry::compute_bounds;
use solarxy_core::preferences::{NormalsMode, PaneMode};
use solarxy_core::view_config::{BoundsMode, PaneDisplaySettings};
use solarxy_renderer::geometry::build_normals_geometry;
use solarxy_renderer::model::NormalsGeometry;
use solarxy_renderer::scene_objects::SceneObjects;

/// Whether any pane of the active layout shows an overlay this aggregate
/// feeds.
///
/// The aggregate costs a pass over every vertex in the scene, so it is built
/// only when something would draw it.
#[must_use]
pub fn overlays_wanted(pane_settings: &[PaneDisplaySettings], pane_count: usize) -> bool {
    pane_settings[..pane_count.min(pane_settings.len())]
        .iter()
        .any(|pds| {
            pds.pane_mode != PaneMode::UvMap
                && (pds.normals_mode != NormalsMode::Off || pds.bounds_mode == BoundsMode::PerMesh)
        })
}

/// The world-space visualization aggregate over every drawn mesh: per-mesh
/// world bounds, and the normal arrow lines with their per-mesh segments.
///
/// Positions go through the object matrix and directions through its
/// inverse-transpose, because a geo transform is allowed a nonuniform scale
/// and a direction carried by the plain matrix would come out skewed.
#[must_use]
pub fn build_aggregate(scene: &SceneObjects) -> (Vec<AABB>, NormalsGeometry) {
    let mut mesh_bounds: Vec<AABB> = Vec::new();
    let mut agg = NormalsGeometry {
        vertex_lines: Vec::new(),
        face_lines: Vec::new(),
        vertex_segments: Vec::new(),
        face_segments: Vec::new(),
    };

    for (_, object) in scene.iter().filter(|(_, o)| o.visible) {
        let matrix: Matrix4<f32> = object.transform;
        let normal_matrix = Matrix3::from_cols(
            matrix.x.truncate(),
            matrix.y.truncate(),
            matrix.z.truncate(),
        )
        .invert()
        .map(|inv| Matrix::transpose(&inv));

        let raw_to_gpu = object.raw_to_gpu();
        for (raw, mesh) in object.geometry().meshes.iter().enumerate() {
            // No GPU mesh, no overlay slot: an empty cooked mesh is skipped
            // when the meshes are built, and emitting a segment for it here
            // would shift every later mesh's arrows onto its neighbour.
            if raw_to_gpu.get(raw).copied().flatten().is_none() {
                continue;
            }
            let world: Vec<[f32; 3]> = mesh
                .positions
                .iter()
                .map(|p| {
                    let tp = matrix.transform_point(Point3::from(*p));
                    [tp.x, tp.y, tp.z]
                })
                .collect();
            let bounds = compute_bounds(&world);
            let world_normals: Vec<[f32; 3]> = match (&mesh.normals, normal_matrix) {
                (Some(ns), Some(nm)) => ns
                    .iter()
                    .map(|n| {
                        let v = nm * Vector3::from(*n);
                        let v = if v.magnitude2() > 1e-12 {
                            v.normalize()
                        } else {
                            v
                        };
                        [v.x, v.y, v.z]
                    })
                    .collect(),
                // A singular matrix (a zero scale) has no usable normal
                // transform; fall back to the object-space directions.
                (Some(ns), None) => ns.to_vec(),
                // No stored normals: the vertex arrows are empty, and the face
                // arrows still derive from the world positions.
                (None, _) => Vec::new(),
            };
            let (v_lines, f_lines) =
                build_normals_geometry(&world, &world_normals, &mesh.indices, &bounds);
            let v_start = u32::try_from(agg.vertex_lines.len()).unwrap_or(u32::MAX);
            agg.vertex_lines.extend(v_lines);
            agg.vertex_segments
                .push(v_start..u32::try_from(agg.vertex_lines.len()).unwrap_or(u32::MAX));
            let f_start = u32::try_from(agg.face_lines.len()).unwrap_or(u32::MAX);
            agg.face_lines.extend(f_lines);
            agg.face_segments
                .push(f_start..u32::try_from(agg.face_lines.len()).unwrap_or(u32::MAX));
            mesh_bounds.push(bounds);
        }
    }

    (mesh_bounds, agg)
}
