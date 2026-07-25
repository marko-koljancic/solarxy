//! UV projection (the `uv_project` node's kernel): writes a
//! fresh `tex_coords` buffer from one of four projections, normalized
//! against the whole set's AABB so multiple meshes share one consistent
//! mapping. `uv = normalized * scale + offset`.
//!
//! Planar, cylindrical, and spherical are per-vertex and preserve topology
//! (their wrap seams smear across the seam-crossing triangles; splitting
//! the seam is a fidelity note for a later pass). Box mode assigns each
//! triangle its dominant-normal-axis planar mapping, which is a per-corner
//! property, so it rebuilds the mesh non-indexed (three vertices per
//! triangle, positions/normals/attributes re-indexed) to be crack-free and
//! smear-free at axis boundaries.

use std::f32::consts::PI;
use std::sync::Arc;

use crate::set::{AttributeData, GeometrySet, KernelMesh};

/// The projection shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvProjection {
    Planar,
    Box,
    Cylindrical,
    Spherical,
}

/// The projection axis: planar projects along it, cylindrical wraps around
/// it, spherical uses it as the pole axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvAxis {
    X,
    Y,
    Z,
}

impl UvAxis {
    /// The axis index plus its two cross-axis indices in (u, v) order.
    fn indices(self) -> (usize, usize, usize) {
        match self {
            UvAxis::X => (0, 2, 1), // project along X: u from Z, v from Y
            UvAxis::Y => (1, 0, 2), // project along Y: u from X, v from Z
            UvAxis::Z => (2, 0, 1), // project along Z: u from X, v from Y
        }
    }
}

/// Projects UVs onto every mesh of `set`. Bounds and materials are
/// untouched; every mesh gets a fresh `tex_coords` Arc (box mode rebuilds
/// the meshes non-indexed, see the module doc).
#[must_use]
pub fn uv_project(
    set: &GeometrySet,
    mode: UvProjection,
    axis: UvAxis,
    scale: [f32; 2],
    offset: [f32; 2],
) -> GeometrySet {
    let bounds = set.bounds;
    let min = [bounds.min.x, bounds.min.y, bounds.min.z];
    let extent = [
        (bounds.max.x - bounds.min.x).max(1e-6),
        (bounds.max.y - bounds.min.y).max(1e-6),
        (bounds.max.z - bounds.min.z).max(1e-6),
    ];
    let center = [
        (bounds.min.x + bounds.max.x) * 0.5,
        (bounds.min.y + bounds.max.y) * 0.5,
        (bounds.min.z + bounds.max.z) * 0.5,
    ];

    let mut out = set.clone();
    for mesh in &mut out.meshes {
        // UV projection is a surface operation: line and point meshes pass
        // through untouched (the node warns), and box mode's per-triangle
        // rebuild would destroy them outright.
        if mesh.topology != solarxy_core::geometry::MeshTopology::Triangles {
            continue;
        }
        if mode == UvProjection::Box {
            *mesh = box_project_mesh(mesh, min, extent, axis, scale, offset);
        } else {
            let uvs: Vec<[f32; 2]> = mesh
                .positions
                .iter()
                .map(|pos| {
                    let norm = match mode {
                        UvProjection::Planar => planar_uv(*pos, min, extent, axis),
                        UvProjection::Cylindrical => {
                            cylindrical_uv(*pos, min, extent, center, axis)
                        }
                        UvProjection::Spherical => spherical_uv(*pos, center, axis),
                        UvProjection::Box => unreachable!("handled above"),
                    };
                    apply_scale_offset(norm, scale, offset)
                })
                .collect();
            mesh.tex_coords = Some(Arc::new(uvs));
        }
    }
    out
}

fn apply_scale_offset(uv: [f32; 2], scale: [f32; 2], offset: [f32; 2]) -> [f32; 2] {
    [uv[0] * scale[0] + offset[0], uv[1] * scale[1] + offset[1]]
}

/// Planar: drop the projection axis, normalize the two cross axes over the
/// set bounds.
fn planar_uv(pos: [f32; 3], min: [f32; 3], extent: [f32; 3], axis: UvAxis) -> [f32; 2] {
    let (_, u_axis, v_axis) = axis.indices();
    [
        (pos[u_axis] - min[u_axis]) / extent[u_axis],
        (pos[v_axis] - min[v_axis]) / extent[v_axis],
    ]
}

/// Cylindrical: angle around the axis (u, seam at -pi) and normalized
/// height along it (v).
fn cylindrical_uv(
    pos: [f32; 3],
    min: [f32; 3],
    extent: [f32; 3],
    center: [f32; 3],
    axis: UvAxis,
) -> [f32; 2] {
    let (h_axis, u_axis, v_axis) = axis.indices();
    let a = pos[u_axis] - center[u_axis];
    let b = pos[v_axis] - center[v_axis];
    let theta = b.atan2(a);
    [
        theta / (2.0 * PI) + 0.5,
        (pos[h_axis] - min[h_axis]) / extent[h_axis],
    ]
}

/// Spherical: longitude around the pole axis (u) and latitude from it (v,
/// 1 at the +axis pole).
fn spherical_uv(pos: [f32; 3], center: [f32; 3], axis: UvAxis) -> [f32; 2] {
    let (pole, u_axis, v_axis) = axis.indices();
    let d = [pos[0] - center[0], pos[1] - center[1], pos[2] - center[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-9 {
        return [0.5, 0.5];
    }
    let theta = d[v_axis].atan2(d[u_axis]);
    let phi = (d[pole] / len).clamp(-1.0, 1.0).acos();
    [theta / (2.0 * PI) + 0.5, 1.0 - phi / PI]
}

/// Box: per-triangle dominant face-normal axis selects one of the three
/// planar mappings. Rebuilds the mesh non-indexed so corners shared by
/// triangles of different dominant axes carry different UVs.
fn box_project_mesh(
    mesh: &KernelMesh,
    min: [f32; 3],
    extent: [f32; 3],
    axis_hint: UvAxis,
    scale: [f32; 2],
    offset: [f32; 2],
) -> KernelMesh {
    let _ = axis_hint; // box uses all three axes; the param is inert here
    let tri_count = mesh.indices.len() / 3;
    let mut positions = Vec::with_capacity(tri_count * 3);
    let mut normals = mesh
        .normals
        .as_ref()
        .map(|_| Vec::with_capacity(tri_count * 3));
    let mut uvs = Vec::with_capacity(tri_count * 3);
    let mut corner_sources = Vec::with_capacity(tri_count * 3);

    for tri in mesh.indices.chunks_exact(3) {
        let [i0, i1, i2] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        let (p0, p1, p2) = (mesh.positions[i0], mesh.positions[i1], mesh.positions[i2]);
        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let dominant = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() {
            UvAxis::X
        } else if n[1].abs() >= n[2].abs() {
            UvAxis::Y
        } else {
            UvAxis::Z
        };

        for &i in &[i0, i1, i2] {
            let pos = mesh.positions[i];
            positions.push(pos);
            if let (Some(out), Some(src)) = (normals.as_mut(), mesh.normals.as_ref()) {
                out.push(src[i]);
            }
            uvs.push(apply_scale_offset(
                planar_uv(pos, min, extent, dominant),
                scale,
                offset,
            ));
            corner_sources.push(i);
        }
    }

    // Re-index the extra attribute lanes through the corner map.
    let mut attributes = crate::set::AttributeMap::new();
    for (key, data) in &mesh.attributes {
        let reindexed = match data {
            AttributeData::Float(v) => {
                AttributeData::Float(Arc::new(corner_sources.iter().map(|&i| v[i]).collect()))
            }
            AttributeData::Vec2(v) => {
                AttributeData::Vec2(Arc::new(corner_sources.iter().map(|&i| v[i]).collect()))
            }
            AttributeData::Vec3(v) => {
                AttributeData::Vec3(Arc::new(corner_sources.iter().map(|&i| v[i]).collect()))
            }
            AttributeData::Vec4(v) => {
                AttributeData::Vec4(Arc::new(corner_sources.iter().map(|&i| v[i]).collect()))
            }
        };
        attributes.insert(key.clone(), reindexed);
    }

    let index_count = positions.len() as u32;
    KernelMesh {
        name: mesh.name.clone(),
        positions: Arc::new(positions),
        normals: normals.map(Arc::new),
        tex_coords: Some(Arc::new(uvs)),
        indices: Arc::new((0..index_count).collect()),
        material_index: mesh.material_index,
        topology: mesh.topology,
        attributes,
        primitive_attributes: mesh.primitive_attributes.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{generate_box, generate_sphere};

    fn unit_box_set() -> GeometrySet {
        GeometrySet::from_mesh(generate_box(2.0, 2.0, 2.0, 1, 1, 1))
    }

    #[test]
    fn non_triangle_meshes_pass_through_untouched() {
        let cloud = crate::set::KernelMesh::points("p", vec![[0.0; 3], [1.0; 3]]);
        let set =
            GeometrySet::from_parts(vec![cloud, generate_box(2.0, 2.0, 2.0, 1, 1, 1)], vec![]);
        // Box mode is the destructive one: it rebuilds meshes per triangle,
        // which would erase a point cloud entirely without the gate.
        let out = uv_project(&set, UvProjection::Box, UvAxis::Y, [1.0; 2], [0.0; 2]);
        assert!(
            Arc::ptr_eq(&out.meshes[0].positions, &set.meshes[0].positions),
            "cloud buffers untouched"
        );
        assert!(
            out.meshes[0].tex_coords.is_none(),
            "no UVs invented for points"
        );
        assert!(out.meshes[1].tex_coords.is_some(), "the box got its UVs");
    }

    #[test]
    fn planar_y_normalizes_over_bounds() {
        let out = uv_project(
            &unit_box_set(),
            UvProjection::Planar,
            UvAxis::Y,
            [1.0, 1.0],
            [0.0, 0.0],
        );
        let uvs = out.meshes[0].tex_coords.as_ref().unwrap();
        assert_eq!(uvs.len(), out.meshes[0].positions.len());
        for uv in uvs.iter() {
            assert!((-1e-5..=1.0 + 1e-5).contains(&uv[0]), "u in range: {uv:?}");
            assert!((-1e-5..=1.0 + 1e-5).contains(&uv[1]), "v in range: {uv:?}");
        }
        // Corners hit the exact normalized extremes somewhere.
        assert!(uvs.iter().any(|uv| uv[0] < 1e-5));
        assert!(uvs.iter().any(|uv| uv[0] > 1.0 - 1e-5));
    }

    #[test]
    fn scale_and_offset_apply_after_normalization() {
        let out = uv_project(
            &unit_box_set(),
            UvProjection::Planar,
            UvAxis::Y,
            [2.0, 3.0],
            [0.5, -1.0],
        );
        let uvs = out.meshes[0].tex_coords.as_ref().unwrap();
        let umin = uvs.iter().map(|uv| uv[0]).fold(f32::INFINITY, f32::min);
        let umax = uvs.iter().map(|uv| uv[0]).fold(f32::NEG_INFINITY, f32::max);
        let vmin = uvs.iter().map(|uv| uv[1]).fold(f32::INFINITY, f32::min);
        let vmax = uvs.iter().map(|uv| uv[1]).fold(f32::NEG_INFINITY, f32::max);
        assert!((umin - 0.5).abs() < 1e-5 && (umax - 2.5).abs() < 1e-5);
        assert!((vmin + 1.0).abs() < 1e-5 && (vmax - 2.0).abs() < 1e-5);
    }

    #[test]
    fn box_mode_rebuilds_per_corner_and_covers_each_face() {
        let set = unit_box_set();
        let tris_in = set.meshes[0].triangle_count();
        let out = uv_project(&set, UvProjection::Box, UvAxis::Y, [1.0, 1.0], [0.0, 0.0]);
        let mesh = &out.meshes[0];
        assert_eq!(mesh.triangle_count(), tris_in, "topology count preserved");
        assert_eq!(
            mesh.positions.len(),
            tris_in * 3,
            "non-indexed per-corner rebuild"
        );
        let uvs = mesh.tex_coords.as_ref().unwrap();
        // An axis-aligned cube face spans the full normalized range on
        // both of its cross axes.
        assert!(uvs.iter().any(|uv| uv[0] < 1e-5 && uv[1] < 1e-5));
        assert!(
            uvs.iter()
                .any(|uv| uv[0] > 1.0 - 1e-5 && uv[1] > 1.0 - 1e-5)
        );
        // Normals survived the rebuild with matching length.
        assert_eq!(
            mesh.normals.as_ref().map(|n| n.len()),
            Some(mesh.positions.len())
        );
    }

    #[test]
    fn spherical_covers_longitude_and_latitude() {
        let set = GeometrySet::from_mesh(generate_sphere(1.0, 24, 16));
        let out = uv_project(
            &set,
            UvProjection::Spherical,
            UvAxis::Y,
            [1.0, 1.0],
            [0.0, 0.0],
        );
        let uvs = out.meshes[0].tex_coords.as_ref().unwrap();
        let umin = uvs.iter().map(|uv| uv[0]).fold(f32::INFINITY, f32::min);
        let umax = uvs.iter().map(|uv| uv[0]).fold(f32::NEG_INFINITY, f32::max);
        let vmin = uvs.iter().map(|uv| uv[1]).fold(f32::INFINITY, f32::min);
        let vmax = uvs.iter().map(|uv| uv[1]).fold(f32::NEG_INFINITY, f32::max);
        assert!(umin < 0.1 && umax > 0.9, "longitude sweep: {umin} {umax}");
        assert!(vmin < 0.15 && vmax > 0.85, "latitude sweep: {vmin} {vmax}");
    }

    #[test]
    fn cylindrical_v_tracks_height() {
        let out = uv_project(
            &unit_box_set(),
            UvProjection::Cylindrical,
            UvAxis::Y,
            [1.0, 1.0],
            [0.0, 0.0],
        );
        let mesh = &out.meshes[0];
        let uvs = mesh.tex_coords.as_ref().unwrap();
        for (pos, uv) in mesh.positions.iter().zip(uvs.iter()) {
            let expected_v = f32::midpoint(pos[1], 1.0);
            assert!((uv[1] - expected_v).abs() < 1e-5);
        }
    }
}
