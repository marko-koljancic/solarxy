//! Primitive removal by region or by facing (the `delete` node's kernel).
//!
//! The kernel has no per-face attributes, no groups, and no primitive ids, so
//! there was no selection model to build on and this one is defined from
//! scratch: the predicate is evaluated per triangle, against its centroid
//! (bbox mode) or its geometric face normal (normal mode).
//!
//! Deletion is not just an index filter. Dropping triangles orphans vertices,
//! so surviving triangles are re-indexed and every vertex lane (positions,
//! normals, UVs, and each named [`AttributeData`] lane) is compacted through a
//! single old-to-new index remap, following the lane-rebuild pattern
//! `subdivide` established. Meshes that lose every triangle drop out of the
//! set entirely, and an empty result is legal (the node warns; it is not a cook
//! error). Line and point meshes have no triangles for the predicate to see
//! and pass through untouched.

use std::sync::Arc;

use crate::set::{AttributeData, AttributeMap, GeometrySet, KernelMesh};

/// Which triangles the predicate selects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeleteMode {
    /// Selects triangles whose centroid lies inside the axis-aligned region
    /// box `center +/- size/2`.
    Bbox { center: [f32; 3], size: [f32; 3] },
    /// Selects triangles whose geometric face normal lies within `angle_rad`
    /// of `direction`.
    Normal { direction: [f32; 3], angle_rad: f32 },
}

/// A sentinel angle that no face can satisfy (a real angle is never negative),
/// used to turn a degenerate direction into "selects nothing" without a second
/// mode variant.
const NO_SELECTION: f32 = -1.0;

/// The outcome of a delete: the surviving geometry plus what the node needs to
/// report honestly.
#[derive(Debug)]
pub struct DeleteResult {
    pub set: GeometrySet,
    /// Triangles removed across the whole set.
    pub removed: usize,
    /// True when `direction` was degenerate (near-zero) and the normal-mode
    /// predicate was therefore skipped rather than guessed at.
    pub degenerate_direction: bool,
}

/// Deletes the triangles the predicate selects (or the ones it does not, when
/// `invert`).
///
/// A degenerate `direction` in normal mode selects nothing and reports itself
/// through [`DeleteResult::degenerate_direction`], rather than erroring or
/// silently normalizing a zero vector into garbage.
#[must_use]
pub fn delete(set: &GeometrySet, mode: DeleteMode, invert: bool) -> DeleteResult {
    let mut degenerate_direction = false;
    let mode = match mode {
        DeleteMode::Normal {
            direction,
            angle_rad,
        } => {
            if let Some(unit) = normalize(direction) {
                DeleteMode::Normal {
                    direction: unit,
                    angle_rad,
                }
            } else {
                degenerate_direction = true;
                // Selects nothing: with `invert` that means "delete everything",
                // which is the honest reading of an inverted empty selection.
                DeleteMode::Normal {
                    direction: [0.0; 3],
                    angle_rad: NO_SELECTION,
                }
            }
        }
        other @ DeleteMode::Bbox { .. } => other,
    };

    let mut removed = 0usize;
    let mut meshes: Vec<KernelMesh> = Vec::with_capacity(set.meshes.len());
    for mesh in &set.meshes {
        // The predicate is defined per triangle; a line or point mesh has
        // none, and running it through the triangle filter would silently
        // drop every point. Pass such meshes through untouched (the node
        // warns); a point-domain predicate is recorded future work.
        if mesh.topology != solarxy_core::geometry::MeshTopology::Triangles {
            meshes.push(mesh.clone());
            continue;
        }
        let (kept, dropped) = filter_mesh(mesh, mode, invert);
        removed += dropped;
        if let Some(m) = kept {
            meshes.push(m);
        }
    }

    // Materials ride along; `from_parts` recomputes bounds over what survived.
    // An index whose mesh vanished simply stops being referenced; leaving the
    // material table intact keeps indices stable and costs nothing.
    let out = GeometrySet::from_parts(meshes, set.materials.clone());
    DeleteResult {
        set: out,
        removed,
        degenerate_direction,
    }
}

/// Filters one mesh. Returns `None` when nothing survived.
fn filter_mesh(mesh: &KernelMesh, mode: DeleteMode, invert: bool) -> (Option<KernelMesh>, usize) {
    let tri_count = mesh.triangle_count();
    let mut kept_tris: Vec<[u32; 3]> = Vec::with_capacity(tri_count);
    let mut removed = 0usize;

    for tri in mesh.indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0], tri[1], tri[2]);
        let (a, b, c) = (
            mesh.positions[i0 as usize],
            mesh.positions[i1 as usize],
            mesh.positions[i2 as usize],
        );
        // `selected` means "the predicate picked this triangle"; `invert` flips
        // which side of the predicate gets deleted. Equal means "keep".
        let selected = selects(mode, a, b, c);
        if selected == invert {
            kept_tris.push([i0, i1, i2]);
        } else {
            removed += 1;
        }
    }

    if kept_tris.is_empty() {
        return (None, removed);
    }
    if removed == 0 {
        // Nothing changed: hand back the mesh with its buffers still shared.
        return (Some(mesh.clone()), 0);
    }

    // Build the old-to-new vertex remap over exactly the vertices the surviving
    // triangles still reference, preserving original order for determinism.
    let mut remap: Vec<Option<u32>> = vec![None; mesh.vertex_count()];
    let mut old_of_new: Vec<u32> = Vec::new();
    for tri in &kept_tris {
        for &old in tri {
            if remap[old as usize].is_none() {
                remap[old as usize] = Some(
                    u32::try_from(old_of_new.len()).expect("vertex count fits u32 (input did)"),
                );
                old_of_new.push(old);
            }
        }
    }

    let indices: Vec<u32> = kept_tris
        .iter()
        .flat_map(|t| {
            t.iter()
                .map(|&old| remap[old as usize].expect("visited above"))
        })
        .collect();

    let positions: Vec<[f32; 3]> = old_of_new
        .iter()
        .map(|&old| mesh.positions[old as usize])
        .collect();
    let normals = mesh.normals.as_ref().map(|buf| {
        Arc::new(
            old_of_new
                .iter()
                .map(|&old| buf[old as usize])
                .collect::<Vec<_>>(),
        )
    });
    let tex_coords = mesh.tex_coords.as_ref().map(|buf| {
        Arc::new(
            old_of_new
                .iter()
                .map(|&old| buf[old as usize])
                .collect::<Vec<_>>(),
        )
    });
    let attributes: AttributeMap = mesh
        .attributes
        .iter()
        .map(|(k, data)| (k.clone(), compact_lane(data, &old_of_new)))
        .collect();

    (
        Some(KernelMesh {
            name: mesh.name.clone(),
            positions: Arc::new(positions),
            normals,
            tex_coords,
            indices: Arc::new(indices),
            material_index: mesh.material_index,
            topology: mesh.topology,
            attributes,
            primitive_attributes: mesh.primitive_attributes.clone(),
            instances: None,
        }),
        removed,
    )
}

/// Gathers one attribute lane down to the surviving vertices, preserving its
/// element type.
fn compact_lane(data: &AttributeData, old_of_new: &[u32]) -> AttributeData {
    macro_rules! gather {
        ($v:expr) => {
            Arc::new(
                old_of_new
                    .iter()
                    .map(|&old| $v[old as usize])
                    .collect::<Vec<_>>(),
            )
        };
    }
    match data {
        AttributeData::Float(v) => AttributeData::Float(gather!(v)),
        AttributeData::Vec2(v) => AttributeData::Vec2(gather!(v)),
        AttributeData::Vec3(v) => AttributeData::Vec3(gather!(v)),
        AttributeData::Vec4(v) => AttributeData::Vec4(gather!(v)),
    }
}

/// Whether the predicate picks this triangle.
fn selects(mode: DeleteMode, a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> bool {
    match mode {
        DeleteMode::Bbox { center, size } => {
            let cen = centroid(a, b, c);
            (0..3).all(|i| {
                let half = (size[i] * 0.5).abs();
                (cen[i] - center[i]).abs() <= half
            })
        }
        DeleteMode::Normal {
            direction,
            angle_rad,
        } => {
            if angle_rad <= NO_SELECTION {
                return false; // the degenerate-direction sentinel
            }
            let Some(n) = normalize(cross(sub(b, a), sub(c, a))) else {
                return false; // a degenerate triangle has no facing
            };
            // cos is monotonically decreasing on [0, pi], so "within angle" is
            // "cosine at least cos(angle)". Comparing cosines avoids an acos.
            dot(n, direction) >= angle_rad.cos()
        }
    }
}

fn centroid(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    [
        (a[0] + b[0] + c[0]) / 3.0,
        (a[1] + b[1] + c[1]) / 3.0,
        (a[2] + b[2] + c[2]) / 3.0,
    ]
}

fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let len2 = dot(v, v);
    if len2 <= 1e-20 {
        return None;
    }
    let inv = len2.sqrt().recip();
    Some([v[0] * inv, v[1] * inv, v[2] * inv])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{generate_box, generate_plane};

    fn boxed() -> GeometrySet {
        GeometrySet::from_mesh(generate_box(2.0, 2.0, 2.0, 1, 1, 1))
    }

    /// The corruption regression this milestone's spec calls out by name:
    /// the triangle predicate sees no triangles in a point cloud, and
    /// without the topology gate the compaction would silently delete
    /// every point. Same hazard for polylines.
    #[test]
    fn delete_passes_point_clouds_and_polylines_through_untouched() {
        let set = GeometrySet::from_parts(
            vec![
                KernelMesh::points("p", vec![[0.0; 3]; 8]),
                KernelMesh::polyline("l", vec![[0.0; 3], [1.0; 3]], vec![0, 1]),
                generate_box(2.0, 2.0, 2.0, 1, 1, 1),
            ],
            vec![],
        );
        // A region covering everything: every triangle goes; every point
        // and segment stays.
        let out = delete(
            &set,
            DeleteMode::Bbox {
                center: [0.0; 3],
                size: [100.0; 3],
            },
            false,
        );
        assert_eq!(
            out.set.mesh_count(),
            2,
            "box deleted, cloud and line survive"
        );
        assert_eq!(out.set.meshes[0].vertex_count(), 8);
        assert_eq!(
            out.set.meshes[0].topology,
            solarxy_core::geometry::MeshTopology::Points
        );
        assert_eq!(out.set.meshes[1].primitive_count(), 1);
        assert_eq!(out.removed, 12, "only triangles are counted as removed");
    }

    #[test]
    fn bbox_mode_removes_only_triangles_whose_centroid_is_inside() {
        // A box spans -1..1. A region covering only x > 0 catches the +X face.
        let set = boxed();
        let before = set.triangle_count();
        let out = delete(
            &set,
            DeleteMode::Bbox {
                center: [1.0, 0.0, 0.0],
                size: [0.5, 4.0, 4.0], // x in 0.75..1.25 -> only the +X face
            },
            false,
        );
        assert_eq!(out.removed, 2, "the +X face is two triangles");
        assert_eq!(out.set.triangle_count(), before - 2);
    }

    #[test]
    fn invert_keeps_exactly_what_the_predicate_selected() {
        let set = boxed();
        let region = DeleteMode::Bbox {
            center: [1.0, 0.0, 0.0],
            size: [0.5, 4.0, 4.0],
        };
        let out = delete(&set, region, true);
        assert_eq!(
            out.set.triangle_count(),
            2,
            "inverted: only the selected +X face survives"
        );
    }

    #[test]
    fn normal_mode_removes_faces_pointing_along_the_direction() {
        let set = boxed();
        let out = delete(
            &set,
            DeleteMode::Normal {
                direction: [0.0, 1.0, 0.0],
                angle_rad: std::f32::consts::FRAC_PI_4, // 45 degrees
            },
            false,
        );
        // Only the +Y face is within 45 degrees of +Y; the four sides are at 90.
        assert_eq!(out.removed, 2, "the +Y face, two triangles");
    }

    #[test]
    fn a_degenerate_direction_deletes_nothing_and_says_so() {
        let set = boxed();
        let before = set.triangle_count();
        let out = delete(
            &set,
            DeleteMode::Normal {
                direction: [0.0, 0.0, 0.0],
                angle_rad: std::f32::consts::PI,
            },
            false,
        );
        assert!(out.degenerate_direction);
        assert_eq!(out.removed, 0);
        assert_eq!(out.set.triangle_count(), before);
    }

    #[test]
    fn deleting_everything_yields_a_legal_empty_set() {
        let set = boxed();
        let out = delete(
            &set,
            DeleteMode::Bbox {
                center: [0.0; 3],
                size: [100.0; 3],
            },
            false,
        );
        assert!(out.set.is_renderable_empty());
        assert_eq!(out.set.mesh_count(), 0, "the emptied mesh drops out");
        assert_eq!(out.removed as u64, set.triangle_count());
    }

    /// The compaction contract: no orphan vertices, every index in range, and
    /// the surviving vertex data is the data that belonged to those vertices.
    #[test]
    fn surviving_vertices_are_compacted_and_reindexed_coherently() {
        let set = boxed();
        let out = delete(
            &set,
            DeleteMode::Normal {
                direction: [0.0, 1.0, 0.0],
                angle_rad: std::f32::consts::FRAC_PI_4,
            },
            false,
        );
        let mesh = &out.set.meshes[0];

        // Every index is in range.
        for &i in mesh.indices.iter() {
            assert!((i as usize) < mesh.vertex_count(), "index {i} out of range");
        }
        // No orphans: every vertex is referenced by some triangle.
        let mut used = vec![false; mesh.vertex_count()];
        for &i in mesh.indices.iter() {
            used[i as usize] = true;
        }
        assert!(used.iter().all(|&u| u), "an orphan vertex survived");

        // Lanes stayed the same length as the position buffer.
        if let Some(n) = &mesh.normals {
            assert_eq!(n.len(), mesh.vertex_count());
        }
        if let Some(uv) = &mesh.tex_coords {
            assert_eq!(uv.len(), mesh.vertex_count());
        }

        // Every surviving normal still points somewhere sane (the +Y face is
        // gone, so no surviving normal should be +Y).
        let normals = mesh.normals.as_ref().unwrap();
        for n in normals.iter() {
            assert!(n[1] < 0.99, "a +Y normal survived: {n:?}");
        }
    }

    #[test]
    fn named_attribute_lanes_compact_with_the_vertices() {
        let mut set = GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1));
        let vcount = set.meshes[0].vertex_count();
        // Tag each vertex with its own index so we can prove the gather is
        // value-correct, not merely length-correct.
        let lane: Vec<f32> = (0..vcount).map(|i| i as f32).collect();
        set.meshes[0]
            .attributes
            .insert("tag".to_string(), AttributeData::Float(Arc::new(lane)));

        // Delete one of the plane's two triangles by region.
        let out = delete(
            &set,
            DeleteMode::Bbox {
                center: [-0.9, 0.0, -0.9],
                size: [0.5, 0.5, 0.5],
            },
            false,
        );
        let mesh = &out.set.meshes[0];
        let Some(AttributeData::Float(tag)) = mesh.attributes.get("tag") else {
            panic!("the lane survived with its type");
        };
        assert_eq!(
            tag.len(),
            mesh.vertex_count(),
            "lane matches the compacted vertices"
        );
        // Each surviving tag must equal the original index of a vertex that is
        // still positioned where that original vertex was. (The lane is a gather,
        // so the positions are bit-identical; compare with an epsilon anyway
        // rather than asserting exact float equality.)
        for (new_i, t) in tag.iter().enumerate() {
            let old_i = *t as usize;
            let got = mesh.positions[new_i];
            let want = set.meshes[0].positions[old_i];
            assert!(
                (0..3).all(|k| (got[k] - want[k]).abs() < 1e-6),
                "tag {t} does not match the vertex it rode in on: {got:?} vs {want:?}"
            );
        }
    }

    #[test]
    fn an_untouched_mesh_keeps_sharing_its_buffers() {
        let set = boxed();
        let out = delete(
            &set,
            DeleteMode::Bbox {
                center: [100.0, 100.0, 100.0],
                size: [1.0; 3],
            },
            false,
        );
        assert_eq!(out.removed, 0);
        assert!(
            Arc::ptr_eq(&out.set.meshes[0].positions, &set.meshes[0].positions),
            "a no-op delete must not rebuild buffers"
        );
    }

    #[test]
    fn material_indices_survive() {
        let mut set = boxed();
        set.materials = vec![Arc::new(solarxy_core::geometry::RawMaterialData {
            name: "red".to_string(),
            ..Default::default()
        })];
        set.meshes[0].material_index = Some(0);

        let out = delete(
            &set,
            DeleteMode::Normal {
                direction: [0.0, 1.0, 0.0],
                angle_rad: std::f32::consts::FRAC_PI_4,
            },
            false,
        );
        assert_eq!(out.set.meshes[0].material_index, Some(0));
        assert_eq!(out.set.materials.len(), 1);
    }
}
