//! Shared test fixtures for `validation` submodule tests. Compiled only
//! under `#[cfg(test)]` — no impact on the release surface.

#![cfg(test)]

use crate::geometry::{MeshTopology, RawMeshData, RawModelData};

/// Single clean unit triangle with normals + UVs, no material. Used as
/// the canonical "valid" starting point that each test mutates to
/// exercise a specific failure mode.
pub(super) fn single_triangle_raw() -> RawModelData {
    RawModelData {
        meshes: vec![RawMeshData {
            name: "test".to_string(),
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
            normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
            tex_coords: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]),
            material_index: None,
            topology: MeshTopology::Triangles,
        }],
        materials: vec![],
        polygon_count: 1,
    }
}
