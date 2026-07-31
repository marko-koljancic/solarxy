//! Edge extraction (the `edges_to_geo` node's kernel): the unique edges of
//! each input mesh as Lines-topology segments, discharging the Tier-2
//! catalog row that waited on line-primitive support.
//!
//! Near-zero-copy: the output shares the input's position buffer (and its
//! point-domain attribute lanes) by refcount; only the segment index list
//! is new. Edges dedup undirected through a `BTreeSet`, so iteration order
//! and therefore output are deterministic.

use std::collections::BTreeSet;
use std::sync::Arc;

use solarxy_core::geometry::MeshTopology;

use crate::set::{AttributeMap, GeometrySet, KernelMesh};

/// Extracts each mesh's unique edges as a Lines mesh. Triangle meshes
/// contribute their triangle edges, line meshes their deduplicated
/// segments; point meshes have no edges and are dropped. Materials are
/// dropped: wires draw unlit.
#[must_use]
pub fn edges_to_geo(set: &GeometrySet) -> GeometrySet {
    let meshes = set
        .meshes
        .iter()
        .filter(|mesh| mesh.topology != MeshTopology::Points)
        .map(edges_of)
        .collect();
    GeometrySet::from_parts(meshes, Vec::new())
}

fn edges_of(mesh: &KernelMesh) -> KernelMesh {
    let mut edges: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut insert = |a: u32, b: u32| {
        if a != b {
            edges.insert((a.min(b), a.max(b)));
        }
    };
    match mesh.topology {
        MeshTopology::Triangles => {
            for tri in mesh.indices.chunks_exact(3) {
                insert(tri[0], tri[1]);
                insert(tri[1], tri[2]);
                insert(tri[2], tri[0]);
            }
        }
        MeshTopology::Lines => {
            for pair in mesh.indices.chunks_exact(2) {
                insert(pair[0], pair[1]);
            }
        }
        MeshTopology::Points => unreachable!("filtered by the caller"),
    }

    let mut indices = Vec::with_capacity(edges.len() * 2);
    for (a, b) in edges {
        indices.push(a);
        indices.push(b);
    }
    KernelMesh {
        name: mesh.name.clone(),
        positions: Arc::clone(&mesh.positions),
        normals: None,
        tex_coords: None,
        indices: Arc::new(indices),
        material_index: None,
        topology: MeshTopology::Lines,
        attributes: mesh.attributes.clone(),
        primitive_attributes: AttributeMap::new(),
        instances: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::generate_box;
    use crate::set::KernelMesh;

    #[test]
    fn a_box_yields_its_unique_edges_sharing_positions() {
        let mesh = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        let set = GeometrySet::from_mesh(mesh.clone());
        let out = edges_to_geo(&set);
        let wire = &out.meshes[0];
        assert_eq!(wire.topology, MeshTopology::Lines);
        // The box splits each of its 6 faces into 2 triangles over 24
        // unshared corner vertices: per face 4 outline edges + 1 diagonal,
        // nothing dedups across faces (no shared indices), so 30 edges.
        assert_eq!(wire.primitive_count(), 30);
        assert!(
            Arc::ptr_eq(&wire.positions, &mesh.positions),
            "positions ride by refcount"
        );
        assert!(wire.is_renderable());
    }

    #[test]
    fn shared_triangle_edges_dedup_once() {
        // Two triangles sharing the 1-2 edge over 4 shared vertices:
        // 5 unique edges, not 6.
        let mesh = KernelMesh::new(
            "quad",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
            vec![0, 1, 2, 1, 3, 2],
        );
        let out = edges_to_geo(&GeometrySet::from_mesh(mesh));
        assert_eq!(out.meshes[0].primitive_count(), 5);
    }

    #[test]
    fn line_segments_dedup_and_point_clouds_drop() {
        let line =
            KernelMesh::polyline("l", vec![[0.0; 3], [1.0, 0.0, 0.0]], vec![0, 1, 1, 0, 0, 1]);
        let cloud = KernelMesh::points("p", vec![[0.0; 3]; 5]);
        let set = GeometrySet::from_parts(vec![line, cloud], Vec::new());
        let out = edges_to_geo(&set);
        assert_eq!(out.mesh_count(), 1, "the point cloud contributes nothing");
        assert_eq!(
            out.meshes[0].primitive_count(),
            1,
            "duplicate segments fold"
        );

        let only_points = GeometrySet::from_mesh(KernelMesh::points("p", vec![[0.0; 3]; 3]));
        assert!(edges_to_geo(&only_points).is_renderable_empty());
    }
}
