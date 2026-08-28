//! Geometry-to-points conversion (the `points_from_geo` node's kernel):
//! collapses each input mesh to a Points-topology cloud, either its
//! vertices verbatim or one point per primitive at the primitive's center.
//!
//! Vertices mode is near-zero-copy: positions and every point-domain lane
//! ride by refcount, and the mesh's normal and UV buffers lift into the
//! reserved `N` and `uv` lanes (when not already claimed) so downstream
//! consumers like `copy_to_points` see them uniformly. Centers mode
//! averages every point-domain lane over each primitive's corners, and
//! primitive-domain lanes cross over to point-domain verbatim, since one
//! primitive becomes exactly one point.

use std::sync::Arc;

use solarxy_core::geometry::MeshTopology;

use crate::set::{AttributeData, AttributeMap, GeometrySet, KernelMesh, reserved};

/// What each output point corresponds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointsFrom {
    /// One point per input vertex, attributes carried verbatim.
    #[default]
    Vertices,
    /// One point per primitive (triangle centroid, segment midpoint, or
    /// the point itself), attributes averaged over the corners.
    PrimitiveCenters,
}

/// Converts every mesh of `set` to a Points-topology cloud. Materials are
/// dropped: point clouds draw unlit by color.
#[must_use]
pub fn points_from_geo(set: &GeometrySet, mode: PointsFrom) -> GeometrySet {
    let meshes = set
        .meshes
        .iter()
        .map(|mesh| match mode {
            PointsFrom::Vertices => vertices_cloud(mesh),
            PointsFrom::PrimitiveCenters => centers_cloud(mesh),
        })
        .collect();
    GeometrySet::from_parts(meshes, Vec::new())
}

/// The vertices themselves as points: buffers ride by refcount, the fixed
/// normal/UV buffers lift into their reserved lanes.
fn vertices_cloud(mesh: &KernelMesh) -> KernelMesh {
    let mut attributes = mesh.attributes.clone();
    if let Some(normals) = mesh
        .normals
        .as_ref()
        .filter(|buf| buf.len() == mesh.positions.len())
        && !attributes.contains_key(reserved::NORMAL)
    {
        attributes.insert(
            reserved::NORMAL.to_string(),
            AttributeData::Vec3(Arc::clone(normals)),
        );
    }
    if let Some(uvs) = mesh
        .tex_coords
        .as_ref()
        .filter(|buf| buf.len() == mesh.positions.len())
        && !attributes.contains_key(reserved::UV)
    {
        attributes.insert(
            reserved::UV.to_string(),
            AttributeData::Vec2(Arc::clone(uvs)),
        );
    }
    KernelMesh {
        name: mesh.name.clone(),
        positions: Arc::clone(&mesh.positions),
        normals: None,
        tex_coords: None,
        indices: Arc::new(Vec::new()),
        material_index: None,
        topology: MeshTopology::Points,
        attributes,
        primitive_attributes: AttributeMap::new(),
        instances: None,
    }
}

/// One point per primitive at its center, every per-point channel averaged
/// over the primitive's corners and every primitive-domain lane crossing
/// to point-domain verbatim.
fn centers_cloud(mesh: &KernelMesh) -> KernelMesh {
    let corner_sets = primitive_corners(mesh);

    let positions: Vec<[f32; 3]> = corner_sets
        .iter()
        .map(|corners| average_of(&mesh.positions, corners))
        .collect();

    let mut attributes = AttributeMap::new();
    for (name, data) in &mesh.attributes {
        if data.len() != mesh.positions.len() {
            continue;
        }
        let averaged = match data {
            AttributeData::Float(v) => AttributeData::Float(Arc::new(
                corner_sets
                    .iter()
                    .map(|corners| {
                        corners.iter().map(|&i| v[i as usize]).sum::<f32>() / corners.len() as f32
                    })
                    .collect(),
            )),
            AttributeData::Vec2(v) => AttributeData::Vec2(Arc::new(
                corner_sets.iter().map(|c| average_of(v, c)).collect(),
            )),
            AttributeData::Vec3(v) => AttributeData::Vec3(Arc::new(
                corner_sets.iter().map(|c| average_of(v, c)).collect(),
            )),
            AttributeData::Vec4(v) => AttributeData::Vec4(Arc::new(
                corner_sets.iter().map(|c| average_of(v, c)).collect(),
            )),
        };
        attributes.insert(name.clone(), averaged);
    }
    if let Some(normals) = mesh
        .normals
        .as_ref()
        .filter(|buf| buf.len() == mesh.positions.len())
        && !attributes.contains_key(reserved::NORMAL)
    {
        attributes.insert(
            reserved::NORMAL.to_string(),
            AttributeData::Vec3(Arc::new(
                corner_sets
                    .iter()
                    .map(|c| normalize_or(average_of(normals, c)))
                    .collect(),
            )),
        );
    }
    if let Some(uvs) = mesh
        .tex_coords
        .as_ref()
        .filter(|buf| buf.len() == mesh.positions.len())
        && !attributes.contains_key(reserved::UV)
    {
        attributes.insert(
            reserved::UV.to_string(),
            AttributeData::Vec2(Arc::new(
                corner_sets.iter().map(|c| average_of(uvs, c)).collect(),
            )),
        );
    }
    // One primitive is exactly one point, so primitive lanes cross over
    // verbatim, winning any name collision with an averaged point lane.
    for (name, data) in &mesh.primitive_attributes {
        if data.len() == corner_sets.len() {
            attributes.insert(name.clone(), data.clone());
        }
    }

    let mut out = KernelMesh::points(mesh.name.clone(), positions);
    out.attributes = attributes;
    out
}

/// The corner indices of each primitive under the mesh's topology.
fn primitive_corners(mesh: &KernelMesh) -> Vec<Vec<u32>> {
    match mesh.topology {
        MeshTopology::Triangles => mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| c.to_vec())
            .collect(),
        MeshTopology::Lines => mesh
            .indices
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| c.to_vec())
            .collect(),
        MeshTopology::Points => (0..mesh.positions.len() as u32).map(|i| vec![i]).collect(),
    }
}

fn average_of<const N: usize>(buffer: &[[f32; N]], corners: &[u32]) -> [f32; N] {
    let mut sum = [0.0f32; N];
    for &corner in corners {
        for (lane, value) in sum.iter_mut().zip(buffer[corner as usize]) {
            *lane += value;
        }
    }
    sum.map(|v| v / corners.len() as f32)
}

fn normalize_or(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-12 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::primitives::{generate_box, generate_plane};

    #[test]
    fn vertices_mode_shares_buffers_and_lifts_normals_and_uvs() {
        let plane = generate_plane(2.0, 2.0, 1, 1);
        let set = GeometrySet::from_mesh(plane.clone());
        let out = points_from_geo(&set, PointsFrom::Vertices);
        let cloud = &out.meshes[0];
        assert_eq!(cloud.topology, MeshTopology::Points);
        assert!(
            Arc::ptr_eq(&cloud.positions, &plane.positions),
            "positions ride by refcount"
        );
        let Some(AttributeData::Vec3(n)) = cloud.attributes.get(reserved::NORMAL) else {
            panic!("normals lifted into the N lane");
        };
        assert_eq!(n.len(), 4);
        let Some(AttributeData::Vec2(uv)) = cloud.attributes.get(reserved::UV) else {
            panic!("UVs lifted into the uv lane");
        };
        assert_eq!(uv.len(), 4);
        assert!(cloud.normals.is_none() && cloud.tex_coords.is_none());
    }

    #[test]
    fn centers_mode_averages_positions_and_lanes() {
        // One triangle with a color lane: the centroid and the averaged
        // color are exact thirds.
        let mut mesh = KernelMesh::new(
            "t",
            vec![[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
            vec![0, 1, 2],
        );
        mesh.attributes.insert(
            reserved::COLOR.to_string(),
            AttributeData::Vec4(Arc::new(vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ])),
        );
        let out = points_from_geo(&GeometrySet::from_mesh(mesh), PointsFrom::PrimitiveCenters);
        let cloud = &out.meshes[0];
        assert_eq!(cloud.vertex_count(), 1);
        assert_eq!(cloud.positions[0], [1.0, 1.0, 0.0]);
        let Some(AttributeData::Vec4(colors)) = cloud.attributes.get(reserved::COLOR) else {
            panic!("color lane averaged");
        };
        let third = 1.0 / 3.0;
        for (got, want) in colors[0].iter().zip([third, third, third, 1.0]) {
            assert!((got - want).abs() < 1e-6, "{:?}", colors[0]);
        }
    }

    #[test]
    fn centers_mode_counts_follow_the_topology() {
        let box_set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let tri_centers = points_from_geo(&box_set, PointsFrom::PrimitiveCenters);
        assert_eq!(tri_centers.meshes[0].vertex_count(), 12, "one per triangle");

        let line = GeometrySet::from_mesh(KernelMesh::polyline(
            "l",
            vec![[0.0; 3], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0]],
            vec![0, 1, 1, 2],
        ));
        let midpoints = points_from_geo(&line, PointsFrom::PrimitiveCenters);
        assert_eq!(midpoints.meshes[0].vertex_count(), 2, "one per segment");
        assert_eq!(midpoints.meshes[0].positions[0], [1.0, 0.0, 0.0]);
        assert_eq!(midpoints.meshes[0].positions[1], [2.0, 1.0, 0.0]);

        let cloud = GeometrySet::from_mesh(KernelMesh::points("p", vec![[5.0, 0.0, 0.0]]));
        let identity = points_from_geo(&cloud, PointsFrom::PrimitiveCenters);
        assert_eq!(identity.meshes[0].positions[0], [5.0, 0.0, 0.0]);
    }

    #[test]
    fn primitive_lanes_cross_to_point_domain_in_centers_mode() {
        let mut mesh = KernelMesh::new(
            "t",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
            vec![0, 1, 2, 1, 3, 2],
        );
        mesh.primitive_attributes.insert(
            "id".to_string(),
            AttributeData::Float(Arc::new(vec![7.0, 9.0])),
        );
        let out = points_from_geo(&GeometrySet::from_mesh(mesh), PointsFrom::PrimitiveCenters);
        let Some(AttributeData::Float(ids)) = out.meshes[0].attributes.get("id") else {
            panic!("primitive lane crossed to point domain");
        };
        assert_eq!(**ids, vec![7.0, 9.0]);
        assert!(out.meshes[0].primitive_attributes.is_empty());
    }
}
