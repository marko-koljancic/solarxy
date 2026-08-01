//! Merge concatenation (the merge node's kernel): concatenates
//! `GeometrySet`s in input order, preserving mesh names and deduplicating
//! materials by content so a fan-out-and-recombine graph does not multiply
//! identical materials.
//!
//! Dedup is hash-bucketed with an equality confirmation, so a 64-bit hash
//! collision can never silently fuse two distinct materials. Every mesh's
//! `material_index` is remapped to the deduplicated table; an index that
//! was already out of range in its input set becomes `None` (the validate
//! node reports such references upstream).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use solarxy_core::geometry::{RawImageData, RawMaterialData};

use crate::set::{GeometrySet, KernelMesh};

/// Concatenates sets in slice order. An empty input list (or all-empty
/// sets) yields the empty set; the merge node adds the user-facing warning.
#[must_use]
pub fn merge(inputs: &[Arc<GeometrySet>]) -> GeometrySet {
    let mut materials: Vec<Arc<RawMaterialData>> = Vec::new();
    // Content hash -> indices into `materials` sharing that hash.
    let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut meshes: Vec<KernelMesh> = Vec::new();

    for set in inputs {
        // Map this input's material indices into the deduplicated table.
        let remap: Vec<usize> = set
            .materials
            .iter()
            .map(|mat| {
                let hash = material_content_hash(mat);
                let bucket = buckets.entry(hash).or_default();
                if let Some(&existing) = bucket.iter().find(|&&i| *materials[i] == **mat) {
                    existing
                } else {
                    let index = materials.len();
                    materials.push(Arc::clone(mat));
                    bucket.push(index);
                    index
                }
            })
            .collect();

        for mesh in &set.meshes {
            let material_index = mesh.material_index.and_then(|i| remap.get(i).copied());
            meshes.push(KernelMesh {
                name: mesh.name.clone(),
                positions: Arc::clone(&mesh.positions),
                normals: mesh.normals.clone(),
                tex_coords: mesh.tex_coords.clone(),
                indices: Arc::clone(&mesh.indices),
                material_index,
                topology: mesh.topology,
                attributes: mesh.attributes.clone(),
                primitive_attributes: mesh.primitive_attributes.clone(),
                // Placements ride along per mesh, which is the whole reason
                // they live there: merging a scatter's prototype with the
                // surface it was scattered over keeps the prototype
                // instanced and leaves the surface alone. A single list for
                // the set could not say that, and merge would have to bake.
                instances: mesh.instances.clone(),
            });
        }
    }

    GeometrySet::from_parts(meshes, materials)
}

/// Content hash over every material field. `f32` values hash by bits with
/// `-0.0` folded onto `+0.0` and NaN canonicalized, matching `PartialEq`
/// on the non-NaN domain (NaN-bearing materials simply never dedup).
#[must_use]
pub fn material_content_hash(mat: &RawMaterialData) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let h = &mut hasher;

    mat.name.hash(h);
    mat.diffuse_texture_path.hash(h);
    mat.normal_texture_path.hash(h);
    hash_opt_image(mat.diffuse_texture_data.as_deref(), h);
    hash_opt_image(mat.normal_texture_data.as_deref(), h);
    mat.metallic_roughness_texture_path.hash(h);
    hash_opt_image(mat.metallic_roughness_texture_data.as_deref(), h);
    mat.occlusion_texture_path.hash(h);
    hash_opt_image(mat.occlusion_texture_data.as_deref(), h);
    mat.emissive_texture_path.hash(h);
    hash_opt_image(mat.emissive_texture_data.as_deref(), h);
    hash_f32(mat.roughness_factor, h);
    hash_f32(mat.metallic_factor, h);
    for c in mat.emissive_factor {
        hash_f32(c, h);
    }
    for c in mat.base_color_factor {
        hash_f32(c, h);
    }
    u32::from(mat.alpha_mode).hash(h);
    hash_f32(mat.alpha_cutoff, h);
    hash_opt_rgb(mat.ambient.as_ref(), h);
    hash_opt_rgb(mat.diffuse.as_ref(), h);
    hash_opt_rgb(mat.specular.as_ref(), h);
    hash_opt_f32(mat.shininess, h);
    hash_opt_f32(mat.dissolve, h);
    hash_opt_f32(mat.optical_density, h);
    mat.ambient_texture_name.hash(h);
    mat.diffuse_texture_name.hash(h);
    mat.specular_texture_name.hash(h);
    mat.normal_texture_name.hash(h);
    mat.shininess_texture_name.hash(h);
    mat.dissolve_texture_name.hash(h);

    // The principled surface properties. Two materials differing only here
    // still compare unequal and stay separate, because the dedup above
    // confirms equality inside the bucket rather than trusting the hash, so
    // omitting one of these would cost a collision and not a wrong merge.
    // They are hashed anyway: a scene of glass variants is exactly the case
    // that would otherwise degrade to a linear scan.
    hash_f32(mat.ior, h);
    hash_f32(mat.transmission, h);
    hash_f32(mat.thickness, h);
    for c in mat.attenuation_color {
        hash_f32(c, h);
    }
    hash_f32(mat.attenuation_distance, h);
    hash_f32(mat.clearcoat, h);
    hash_f32(mat.clearcoat_roughness, h);
    for c in mat.sheen_color {
        hash_f32(c, h);
    }
    hash_f32(mat.sheen_roughness, h);
    hash_f32(mat.iridescence, h);
    hash_f32(mat.iridescence_ior, h);
    hash_f32(mat.iridescence_thickness_min, h);
    hash_f32(mat.iridescence_thickness_max, h);
    hash_f32(mat.specular_intensity, h);
    for c in mat.specular_color {
        hash_f32(c, h);
    }
    hash_f32(mat.anisotropy, h);
    hash_f32(mat.anisotropy_rotation, h);
    hash_f32(mat.emissive_strength, h);

    mat.transmission_texture_path.hash(h);
    hash_opt_image(mat.transmission_texture_data.as_deref(), h);
    mat.thickness_texture_path.hash(h);
    hash_opt_image(mat.thickness_texture_data.as_deref(), h);
    mat.clearcoat_texture_path.hash(h);
    hash_opt_image(mat.clearcoat_texture_data.as_deref(), h);
    mat.clearcoat_roughness_texture_path.hash(h);
    hash_opt_image(mat.clearcoat_roughness_texture_data.as_deref(), h);
    mat.clearcoat_normal_texture_path.hash(h);
    hash_opt_image(mat.clearcoat_normal_texture_data.as_deref(), h);
    mat.sheen_color_texture_path.hash(h);
    hash_opt_image(mat.sheen_color_texture_data.as_deref(), h);
    mat.sheen_roughness_texture_path.hash(h);
    hash_opt_image(mat.sheen_roughness_texture_data.as_deref(), h);
    mat.iridescence_texture_path.hash(h);
    hash_opt_image(mat.iridescence_texture_data.as_deref(), h);
    mat.iridescence_thickness_texture_path.hash(h);
    hash_opt_image(mat.iridescence_thickness_texture_data.as_deref(), h);
    mat.specular_texture_path.hash(h);
    hash_opt_image(mat.specular_texture_data.as_deref(), h);
    mat.specular_color_texture_path.hash(h);
    hash_opt_image(mat.specular_color_texture_data.as_deref(), h);
    mat.anisotropy_texture_path.hash(h);
    hash_opt_image(mat.anisotropy_texture_data.as_deref(), h);

    hasher.finish()
}

fn hash_f32<H: Hasher>(v: f32, h: &mut H) {
    let bits = if v.is_nan() {
        f32::NAN.to_bits()
    } else if v == 0.0 {
        0.0_f32.to_bits() // folds -0.0 onto +0.0
    } else {
        v.to_bits()
    };
    bits.hash(h);
}

fn hash_opt_f32<H: Hasher>(v: Option<f32>, h: &mut H) {
    v.is_some().hash(h);
    if let Some(v) = v {
        hash_f32(v, h);
    }
}

fn hash_opt_rgb<H: Hasher>(v: Option<&[f32; 3]>, h: &mut H) {
    v.is_some().hash(h);
    if let Some(rgb) = v {
        for c in rgb {
            hash_f32(*c, h);
        }
    }
}

fn hash_opt_image<H: Hasher>(v: Option<&RawImageData>, h: &mut H) {
    v.is_some().hash(h);
    if let Some(img) = v {
        // The image's own content hash (stamped at construction) stands in
        // for the pixel bytes, so material dedup no longer re-walks them.
        img.hash.hash(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{generate_box, generate_plane};
    use solarxy_core::scene::InstanceXform;

    fn material(name: &str, roughness: f32) -> RawMaterialData {
        RawMaterialData {
            name: name.to_string(),
            roughness_factor: roughness,
            alpha_cutoff: 0.5,
            ..RawMaterialData::default()
        }
    }

    fn set_with_material(mesh_name: &str, mat: RawMaterialData) -> Arc<GeometrySet> {
        let mut mesh = generate_plane(1.0, 1.0, 1, 1);
        mesh.name = mesh_name.to_string();
        mesh.material_index = Some(0);
        Arc::new(GeometrySet::from_parts(vec![mesh], vec![Arc::new(mat)]))
    }

    #[test]
    fn mixed_topologies_concatenate_and_keep_their_tags() {
        use solarxy_core::geometry::MeshTopology;
        let sets = [
            Arc::new(GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1))),
            Arc::new(GeometrySet::from_mesh(KernelMesh::points(
                "p",
                vec![[0.0; 3]],
            ))),
            Arc::new(GeometrySet::from_mesh(KernelMesh::polyline(
                "l",
                vec![[0.0; 3], [1.0; 3]],
                vec![0, 1],
            ))),
        ];
        let out = merge(&sets);
        assert_eq!(out.mesh_count(), 3);
        assert_eq!(out.meshes[0].topology, MeshTopology::Triangles);
        assert_eq!(out.meshes[1].topology, MeshTopology::Points);
        assert_eq!(out.meshes[2].topology, MeshTopology::Lines);
        assert!(!out.is_renderable_empty());
    }

    #[test]
    fn empty_input_yields_empty_set() {
        let out = merge(&[]);
        assert!(out.is_renderable_empty());
        assert!(out.materials.is_empty());
    }

    #[test]
    fn concatenation_preserves_input_order_and_names() {
        let a = Arc::new(GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1)));
        let b = set_with_material("second", material("m", 0.5));
        let c = Arc::new(GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)));
        let out = merge(&[a, b, c]);
        let names: Vec<&str> = out.meshes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["box", "second", "plane"]);
    }

    #[test]
    fn identical_materials_dedup_and_remap() {
        // merge(A, A-alike): one output material, both meshes index 0.
        let a = set_with_material("a", material("m", 0.5));
        let b = set_with_material("b", material("m", 0.5));
        let out = merge(&[a.clone(), b]);
        assert_eq!(out.materials.len(), 1);
        assert_eq!(out.meshes[0].material_index, Some(0));
        assert_eq!(out.meshes[1].material_index, Some(0));
        // Geometry buffers ride along by refcount.
        assert!(Arc::ptr_eq(
            &out.meshes[0].positions,
            &a.meshes[0].positions
        ));
    }

    #[test]
    fn one_float_of_difference_prevents_dedup() {
        let a = set_with_material("a", material("m", 0.5));
        let b = set_with_material("b", material("m", 0.5000001));
        let out = merge(&[a, b]);
        assert_eq!(out.materials.len(), 2);
        assert_eq!(out.meshes[0].material_index, Some(0));
        assert_eq!(out.meshes[1].material_index, Some(1));
    }

    #[test]
    fn negative_zero_dedups_with_positive_zero() {
        let a = set_with_material("a", material("m", 0.0));
        let b = set_with_material("b", material("m", -0.0));
        let out = merge(&[a, b]);
        // PartialEq agrees (0.0 == -0.0) and the hash folds them together.
        assert_eq!(out.materials.len(), 1);
    }

    #[test]
    fn out_of_range_material_index_becomes_none() {
        let mut mesh = generate_plane(1.0, 1.0, 1, 1);
        mesh.material_index = Some(7); // no materials in this set
        let bad = Arc::new(GeometrySet::from_mesh(mesh));
        let out = merge(&[bad]);
        assert_eq!(out.meshes[0].material_index, None);
    }

    #[test]
    fn union_bounds_cover_all_inputs() {
        let near = Arc::new(GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)));
        let far_mesh = {
            let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
            crate::transform::bake_transform(
                &set,
                &crate::transform::compose_trs(
                    [100.0, 0.0, 0.0],
                    [0.0; 3],
                    crate::transform::RotateOrder::Xyz,
                    [1.0; 3],
                    [0.0; 3],
                ),
            )
            .unwrap()
        };
        let out = merge(&[near, Arc::new(far_mesh)]);
        assert!((out.bounds.min.x + 0.5).abs() < 1e-5);
        assert!((out.bounds.max.x - 100.5).abs() < 1e-5);
    }

    /// The reason placements live on the mesh rather than on the set.
    ///
    /// Merging a scatter's instanced prototype with the surface it was
    /// scattered over is the commonest graph there is, and a single list
    /// for the whole set could not express the result: it would replicate
    /// the surface too. So merge would have had to bake, and the most
    /// reached-for downstream node would be the one place instancing
    /// could never survive.
    #[test]
    fn merging_an_instanced_set_with_a_plain_one_keeps_both_as_they_were() {
        let mut prototype = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        prototype.name = "prototype".to_string();
        let places = vec![
            InstanceXform::IDENTITY,
            InstanceXform([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [9.0, 0.0, 0.0, 1.0],
            ]),
        ];
        let scattered = Arc::new(GeometrySet::from_parts_instanced(
            vec![prototype],
            Vec::new(),
            places.clone(),
        ));
        let mut ground = generate_plane(1.0, 1.0, 1, 1);
        ground.name = "ground".to_string();
        let surface = Arc::new(GeometrySet::from_mesh(ground));

        let out = merge(&[Arc::clone(&scattered), Arc::clone(&surface)]);
        assert_eq!(out.mesh_count(), 2);

        let proto = out.meshes.iter().find(|m| m.name == "prototype").unwrap();
        let plain = out.meshes.iter().find(|m| m.name == "ground").unwrap();
        assert_eq!(
            proto.instances.as_ref().map(|i| i.to_vec()),
            Some(places),
            "the prototype keeps its placements through the merge"
        );
        assert!(
            plain.instances.is_none(),
            "the surface is placed once and must not inherit the scatter"
        );
        // And the bounds see the far copy, so a camera frames the scatter
        // rather than the prototype sitting at the origin.
        assert!(out.bounds.max.x > 9.0, "{:?}", out.bounds);
    }

    /// Merge is the operation that would otherwise have to bake, so its
    /// output must equal what baking first would have produced.
    #[test]
    fn a_merged_instanced_set_bakes_to_the_same_geometry() {
        let places = vec![
            InstanceXform::IDENTITY,
            InstanceXform([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [9.0, 0.0, 0.0, 1.0],
            ]),
        ];
        let scattered = Arc::new(GeometrySet::from_parts_instanced(
            vec![generate_box(1.0, 1.0, 1.0, 1, 1, 1)],
            Vec::new(),
            places,
        ));
        let surface = Arc::new(GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)));

        // Bake first, then merge; against merge first, then bake.
        let baked_first = merge(&[
            Arc::new(scattered.baked().expect("bake").into_owned()),
            Arc::clone(&surface),
        ]);
        let merged_first = merge(&[scattered, surface])
            .baked()
            .expect("bake")
            .into_owned();

        let positions = |set: &GeometrySet| -> Vec<[f32; 3]> {
            let mut all: Vec<[f32; 3]> = set
                .meshes
                .iter()
                .flat_map(|m| m.positions.iter().copied())
                .collect();
            all.sort_by(|a, b| a.partial_cmp(b).unwrap());
            all
        };
        assert_eq!(positions(&baked_first), positions(&merged_first));
    }
}
