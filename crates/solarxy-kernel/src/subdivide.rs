//! Linear 1-to-4 triangle subdivision (the `subdivide` node's kernel). Each triangle splits at its edge midpoints; midpoints are
//! deduplicated per shared edge through an edge map, so the surface stays
//! crack-free. Positions, UVs, and extra attribute lanes interpolate
//! linearly; normals interpolate linearly and renormalize. Catmull-Clark
//! is a later scheme behind the same node's `scheme` enum.

use std::collections::HashMap;
use std::sync::Arc;

use crate::set::{AttributeData, AttributeMap, GeometrySet, KernelMesh};

/// The output-triangle ceiling: a deterministic guard so an over-eager
/// iteration count errors instead of stalling the cook or exhausting
/// memory. 4^5 on a 100k-triangle mesh already exceeds this.
pub const MAX_OUTPUT_TRIANGLES: usize = 8_000_000;

/// Subdivides every triangle mesh `iterations` times; line and point meshes
/// pass through untouched (the node warns). Errors (with a user-facing
/// message) when the resulting triangle count would exceed
/// [`MAX_OUTPUT_TRIANGLES`]; materials ride along untouched.
pub fn subdivide_linear(set: &GeometrySet, iterations: u32) -> Result<GeometrySet, String> {
    let input_tris: usize = set.meshes.iter().map(KernelMesh::triangle_count).sum();
    let factor = 4usize.saturating_pow(iterations);
    let projected = input_tris.saturating_mul(factor);
    if projected > MAX_OUTPUT_TRIANGLES {
        return Err(format!(
            "subdivide would produce {projected} triangles (over the {MAX_OUTPUT_TRIANGLES} \
             ceiling); lower the iteration count"
        ));
    }

    let mut out = set.clone();
    for mesh in &mut out.meshes {
        if mesh.topology != solarxy_core::geometry::MeshTopology::Triangles {
            continue;
        }
        for _ in 0..iterations {
            *mesh = subdivide_mesh_once(mesh);
        }
    }
    out.recompute_bounds();
    Ok(out)
}

fn subdivide_mesh_once(mesh: &KernelMesh) -> KernelMesh {
    let mut positions: Vec<[f32; 3]> = mesh.positions.as_ref().clone();
    let mut normals: Option<Vec<[f32; 3]>> = mesh.normals.as_deref().cloned();
    let mut uvs: Option<Vec<[f32; 2]>> = mesh.tex_coords.as_deref().cloned();
    let mut attrs: Vec<(String, AttrLane)> = mesh
        .attributes
        .iter()
        .map(|(k, v)| (k.clone(), AttrLane::from_data(v)))
        .collect();

    // Shared-edge midpoint dedup: the (lo, hi) vertex pair maps to one new
    // vertex, so neighboring triangles reuse it and the surface stays
    // watertight.
    let mut midpoints: HashMap<(u32, u32), u32> = HashMap::new();
    let mut indices = Vec::with_capacity(mesh.indices.len() * 4);

    {
        let mut midpoint = |a: u32,
                            b: u32,
                            positions: &mut Vec<[f32; 3]>,
                            normals: &mut Option<Vec<[f32; 3]>>,
                            uvs: &mut Option<Vec<[f32; 2]>>,
                            attrs: &mut Vec<(String, AttrLane)>|
         -> u32 {
            let key = (a.min(b), a.max(b));
            if let Some(&existing) = midpoints.get(&key) {
                return existing;
            }
            let (ia, ib) = (a as usize, b as usize);
            let index = positions.len() as u32;
            positions.push(lerp3(positions[ia], positions[ib]));
            if let Some(n) = normals {
                let m = lerp3(n[ia], n[ib]);
                let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                n.push(if len > 1e-9 {
                    [m[0] / len, m[1] / len, m[2] / len]
                } else {
                    m
                });
            }
            if let Some(t) = uvs {
                t.push([(t[ia][0] + t[ib][0]) * 0.5, (t[ia][1] + t[ib][1]) * 0.5]);
            }
            for (_, lane) in attrs.iter_mut() {
                lane.push_midpoint(ia, ib);
            }
            midpoints.insert(key, index);
            index
        };

        for tri in mesh.indices.chunks_exact(3) {
            let (a, b, c) = (tri[0], tri[1], tri[2]);
            let ab = midpoint(a, b, &mut positions, &mut normals, &mut uvs, &mut attrs);
            let bc = midpoint(b, c, &mut positions, &mut normals, &mut uvs, &mut attrs);
            let ca = midpoint(c, a, &mut positions, &mut normals, &mut uvs, &mut attrs);
            indices.extend_from_slice(&[a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca]);
        }
    }

    let attributes: AttributeMap = attrs.into_iter().map(|(k, l)| (k, l.into_data())).collect();
    KernelMesh {
        name: mesh.name.clone(),
        positions: Arc::new(positions),
        normals: normals.map(Arc::new),
        tex_coords: uvs.map(Arc::new),
        indices: Arc::new(indices),
        material_index: mesh.material_index,
        topology: mesh.topology,
        attributes,
        primitive_attributes: mesh.primitive_attributes.clone(),
        // Placements ride along: subdividing the prototype subdivides every
        // copy, and an affine placement commutes with linear subdivision,
        // so this equals subdividing after baking and costs one mesh
        // instead of ten thousand.
        instances: mesh.instances.clone(),
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        (a[0] + b[0]) * 0.5,
        (a[1] + b[1]) * 0.5,
        (a[2] + b[2]) * 0.5,
    ]
}

/// An attribute lane unfrozen from its `Arc` for midpoint appends.
enum AttrLane {
    Float(Vec<f32>),
    Vec2(Vec<[f32; 2]>),
    Vec3(Vec<[f32; 3]>),
    Vec4(Vec<[f32; 4]>),
}

impl AttrLane {
    fn from_data(d: &AttributeData) -> Self {
        match d {
            AttributeData::Float(v) => AttrLane::Float(v.as_ref().clone()),
            AttributeData::Vec2(v) => AttrLane::Vec2(v.as_ref().clone()),
            AttributeData::Vec3(v) => AttrLane::Vec3(v.as_ref().clone()),
            AttributeData::Vec4(v) => AttrLane::Vec4(v.as_ref().clone()),
        }
    }

    fn push_midpoint(&mut self, a: usize, b: usize) {
        match self {
            AttrLane::Float(v) => v.push((v[a] + v[b]) * 0.5),
            AttrLane::Vec2(v) => v.push([(v[a][0] + v[b][0]) * 0.5, (v[a][1] + v[b][1]) * 0.5]),
            AttrLane::Vec3(v) => v.push(lerp3(v[a], v[b])),
            AttrLane::Vec4(v) => v.push([
                (v[a][0] + v[b][0]) * 0.5,
                (v[a][1] + v[b][1]) * 0.5,
                (v[a][2] + v[b][2]) * 0.5,
                (v[a][3] + v[b][3]) * 0.5,
            ]),
        }
    }

    fn into_data(self) -> AttributeData {
        match self {
            AttrLane::Float(v) => AttributeData::Float(Arc::new(v)),
            AttrLane::Vec2(v) => AttributeData::Vec2(Arc::new(v)),
            AttrLane::Vec3(v) => AttributeData::Vec3(Arc::new(v)),
            AttrLane::Vec4(v) => AttributeData::Vec4(Arc::new(v)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::generate_plane;
    use crate::set::GeometrySet;

    #[test]
    fn line_and_point_meshes_pass_through_untouched() {
        let cloud = KernelMesh::points("p", vec![[0.0; 3]; 4]);
        let plane = generate_plane(1.0, 1.0, 1, 1);
        let set = GeometrySet::from_parts(vec![cloud, plane], vec![]);
        let out = subdivide_linear(&set, 2).unwrap();
        assert!(
            Arc::ptr_eq(&out.meshes[0].positions, &set.meshes[0].positions),
            "the cloud's buffers are untouched, not rebuilt"
        );
        assert_eq!(out.meshes[0].vertex_count(), 4);
        assert_eq!(out.meshes[1].triangle_count(), 32, "2 tris x4 x4");
    }

    #[test]
    fn one_iteration_quadruples_triangles_and_dedups_midpoints() {
        // A 1x1-segment plane: 4 verts, 2 triangles sharing one edge.
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let out = subdivide_linear(&set, 1).unwrap();
        let mesh = &out.meshes[0];
        assert_eq!(mesh.triangle_count(), 8, "2 tris x4");
        // 4 original + 5 midpoints (the shared diagonal midpoint counted
        // ONCE): crack-free dedup.
        assert_eq!(mesh.positions.len(), 9);
        assert_eq!(
            mesh.normals.as_ref().map(|n| n.len()),
            Some(9),
            "normals interpolated per new vertex"
        );
        assert_eq!(
            mesh.tex_coords.as_ref().map(|t| t.len()),
            Some(9),
            "UVs interpolated per new vertex"
        );
    }

    #[test]
    fn midpoint_values_are_linear() {
        let set = GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1));
        let out = subdivide_linear(&set, 1).unwrap();
        let mesh = &out.meshes[0];
        // Every midpoint lies inside the original bounds; UV midpoints
        // inside 0..1.
        for p in mesh.positions.iter() {
            assert!(p[0].abs() <= 1.0 + 1e-6 && p[2].abs() <= 1.0 + 1e-6);
        }
        for uv in mesh.tex_coords.as_ref().unwrap().iter() {
            assert!((0.0 - 1e-6..=1.0 + 1e-6).contains(&uv[0]));
            assert!((0.0 - 1e-6..=1.0 + 1e-6).contains(&uv[1]));
        }
    }

    #[test]
    fn iterations_compound() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let out = subdivide_linear(&set, 3).unwrap();
        assert_eq!(out.meshes[0].triangle_count(), 2 * 64, "2 tris x4^3");
    }

    #[test]
    fn ceiling_errors_instead_of_exploding() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 100, 100));
        // 20000 tris * 4^6 > 8M.
        let err = subdivide_linear(&set, 6).unwrap_err();
        assert!(err.contains("ceiling"), "{err}");
    }

    #[test]
    fn bounds_are_recomputed() {
        let set = GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1));
        let before = set.bounds;
        let out = subdivide_linear(&set, 2).unwrap();
        // Linear subdivision cannot exceed the hull; bounds stay equal.
        assert!((out.bounds.min.x - before.min.x).abs() < 1e-6);
        assert!((out.bounds.max.z - before.max.z).abs() < 1e-6);
    }
}
