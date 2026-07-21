//! The [`GeometrySet`] payload: the value that flows on `Geometry` wires in
//! the node graph (`DataType::Geometry` wraps `Arc<GeometrySet>`).
//!
//! Sharing model, two layers, both load-bearing:
//!
//! - The **whole set** travels as `Arc<GeometrySet>` on wires and in the cook
//!   engine's keep-last-good caches, so fan-out and caching never copy.
//! - **Each attribute buffer** inside a [`KernelMesh`] is itself `Arc`-shared
//!   (the same shapes as `solarxy_core::scene::CookedMesh`), so a mutating
//!   operator clones only the buffers it rewrites: `transform` rewrites
//!   positions and normals while UVs, indices, and materials ride along by
//!   refcount bump, and [`GeometrySet::to_cooked`] is a near-zero-copy
//!   structural map into the renderer contract.
//!
//! Because keep-last-good keeps an upstream node's output `Arc` alive, a
//! mutating cook body must never assume sole ownership of its input set;
//! the per-buffer `Arc`s are what keep its defensive clone cheap.

use std::collections::BTreeMap;
use std::sync::Arc;

use solarxy_core::AABB;
use solarxy_core::geometry::{
    MeshTopology, RawMaterialData, RawMeshData, RawModelData, compute_bounds, compute_normals,
};
use solarxy_core::scene::{CookedGeometry, CookedMesh};

/// Which element of a mesh an attribute lane describes: one value per
/// point (vertex) or one value per primitive (triangle, segment, or point
/// primitive, per the mesh topology). Every 0.8.0 producer writes the
/// point domain; the primitive domain is the settled growth axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeDomain {
    Point,
    Primitive,
}

/// Reserved well-known attribute names. Operators and the renderer agree on
/// these by name; a lane under a reserved name must carry the documented
/// type or consumers refuse it.
pub mod reserved {
    /// Per-point color, `Vec4`, linear RGBA (sRGB conversion happens at
    /// import). Drives vertex-color display.
    pub const COLOR: &str = "color";
    /// Per-point normal, `Vec3`, unit length. The attribute-lane twin of
    /// `KernelMesh::normals`; scatter writes it for copy orientation.
    pub const NORMAL: &str = "N";
    /// Per-point texture coordinate, `Vec2`. The attribute-lane twin of
    /// `KernelMesh::tex_coords`.
    pub const UV: &str = "uv";
    /// Per-point uniform scale, `Float`. Reserved: no 0.8.0 producer or
    /// consumer; `copy_to_points` consumes it in a later release.
    pub const PSCALE: &str = "pscale";
}

/// One extra attribute buffer beyond the fixed position/normal/UV set,
/// with one element per point or per primitive depending on which
/// [`AttributeDomain`] map holds it.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeData {
    Float(Arc<Vec<f32>>),
    Vec2(Arc<Vec<[f32; 2]>>),
    Vec3(Arc<Vec<[f32; 3]>>),
    Vec4(Arc<Vec<[f32; 4]>>),
}

impl AttributeData {
    /// Number of per-vertex elements in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            AttributeData::Float(v) => v.len(),
            AttributeData::Vec2(v) => v.len(),
            AttributeData::Vec3(v) => v.len(),
            AttributeData::Vec4(v) => v.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Named extra attributes. `BTreeMap` for deterministic iteration order
/// (matching the workspace convention set by the renderer's `SceneObjects`).
pub type AttributeMap = BTreeMap<String, AttributeData>;

/// One mesh inside a [`GeometrySet`]: a triangle mesh, a polyline, or a
/// point cloud per its `topology`. Buffer field shapes are identical to
/// `solarxy_core::scene::CookedMesh` so conversion is a refcount bump, and
/// the two definitions cannot drift apart without
/// [`GeometrySet::to_cooked`] failing to compile.
#[derive(Debug, Clone)]
pub struct KernelMesh {
    pub name: String,
    pub positions: Arc<Vec<[f32; 3]>>,
    /// Per-vertex normals; `None` means downstream computes or skips.
    pub normals: Option<Arc<Vec<[f32; 3]>>>,
    pub tex_coords: Option<Arc<Vec<[f32; 2]>>>,
    pub indices: Arc<Vec<u32>>,
    /// Index into the owning [`GeometrySet::materials`].
    pub material_index: Option<usize>,
    /// How `indices` connect `positions` into primitives: triangle triples
    /// (the default), segment pairs, or ignored for a point cloud.
    pub topology: MeshTopology,
    /// Extra point-domain attributes (one value per vertex). Reserved
    /// names in [`reserved`] carry contractual types; everything else is
    /// free-form for nodes that consume it.
    pub attributes: AttributeMap,
    /// Extra primitive-domain attributes (one value per triangle, segment,
    /// or point primitive). No 0.8.0 producer writes these; the domain
    /// exists so operators handle both axes from day one.
    pub primitive_attributes: AttributeMap,
}

impl KernelMesh {
    /// A triangle mesh from bare positions and indices, no
    /// normals/UVs/material.
    #[must_use]
    pub fn new(name: impl Into<String>, positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Self {
        Self {
            name: name.into(),
            positions: Arc::new(positions),
            normals: None,
            tex_coords: None,
            indices: Arc::new(indices),
            material_index: None,
            topology: MeshTopology::Triangles,
            attributes: AttributeMap::new(),
            primitive_attributes: AttributeMap::new(),
        }
    }

    /// A point cloud: every position is a primitive, indices unused.
    #[must_use]
    pub fn points(name: impl Into<String>, positions: Vec<[f32; 3]>) -> Self {
        let mut mesh = Self::new(name, positions, Vec::new());
        mesh.topology = MeshTopology::Points;
        mesh
    }

    /// A polyline mesh: `indices` are read as segment pairs.
    #[must_use]
    pub fn polyline(name: impl Into<String>, positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Self {
        let mut mesh = Self::new(name, positions, indices);
        mesh.topology = MeshTopology::Lines;
        mesh
    }

    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Triangle count; zero for line and point topologies (the cook
    /// `prims` statistic stays triangles-only by design).
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        match self.topology {
            MeshTopology::Triangles => self.indices.len() / 3,
            MeshTopology::Lines | MeshTopology::Points => 0,
        }
    }

    /// Primitive count under this mesh's own topology: triangles,
    /// segments, or points. The generalized ceiling/statistics measure for
    /// operators that scale with primitives regardless of kind.
    #[must_use]
    pub fn primitive_count(&self) -> usize {
        match self.topology {
            MeshTopology::Triangles => self.indices.len() / 3,
            MeshTopology::Lines => self.indices.len() / 2,
            MeshTopology::Points => self.positions.len(),
        }
    }

    /// Whether this mesh contributes drawable primitives: at least one
    /// full triangle or segment, or any position at all for a point cloud.
    #[must_use]
    pub fn is_renderable(&self) -> bool {
        if self.positions.is_empty() {
            return false;
        }
        match self.topology {
            MeshTopology::Triangles => self.indices.len() >= 3,
            MeshTopology::Lines => self.indices.len() >= 2,
            MeshTopology::Points => true,
        }
    }

    /// Tight bounds over this mesh's positions
    /// (the empty-input fallback follows `compute_bounds`).
    #[must_use]
    pub fn bounds(&self) -> AABB {
        compute_bounds(&self.positions)
    }

    /// The attribute map for one domain.
    #[must_use]
    pub fn domain_attributes(&self, domain: AttributeDomain) -> &AttributeMap {
        match domain {
            AttributeDomain::Point => &self.attributes,
            AttributeDomain::Primitive => &self.primitive_attributes,
        }
    }

    /// Mutable access to the attribute map for one domain.
    pub fn domain_attributes_mut(&mut self, domain: AttributeDomain) -> &mut AttributeMap {
        match domain {
            AttributeDomain::Point => &mut self.attributes,
            AttributeDomain::Primitive => &mut self.primitive_attributes,
        }
    }

    /// Recomputes per-vertex normals from the triangle topology via the
    /// core face-normal-accumulation kernel, replacing any existing buffer.
    /// A no-op for line and point topologies, whose indices are not
    /// triangle triples (their normals, when meaningful, ride the reserved
    /// `N` attribute lane instead).
    pub fn recompute_normals(&mut self) {
        if self.topology != MeshTopology::Triangles {
            return;
        }
        self.normals = Some(Arc::new(compute_normals(&self.positions, &self.indices)));
    }
}

/// The cook payload: an ordered list of meshes plus their materials, with
/// union bounds. Subsumes the "group" case (merge concatenates sets); there
/// is no separate object or group type on wires.
#[derive(Debug, Clone)]
pub struct GeometrySet {
    pub meshes: Vec<KernelMesh>,
    pub materials: Vec<Arc<RawMaterialData>>,
    /// Union bounds over all renderable meshes (object space).
    pub bounds: AABB,
}

impl GeometrySet {
    /// The empty set (what `Mute` bypass and empty merges produce). Bounds
    /// hold the `compute_bounds` empty-input fallback.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            meshes: Vec::new(),
            materials: Vec::new(),
            bounds: compute_bounds(&[]),
        }
    }

    /// A single-mesh set with no materials.
    #[must_use]
    pub fn from_mesh(mesh: KernelMesh) -> Self {
        Self::from_parts(vec![mesh], Vec::new())
    }

    /// A set from meshes + materials, computing union bounds.
    #[must_use]
    pub fn from_parts(meshes: Vec<KernelMesh>, materials: Vec<Arc<RawMaterialData>>) -> Self {
        let bounds = union_bounds(&meshes);
        Self {
            meshes,
            materials,
            bounds,
        }
    }

    /// Renderable-empty means no mesh contributes drawable primitives
    /// (triangles, segments, or points per each mesh's topology): the
    /// condition the cook engine's keep-last-good policy tests
    /// (a transiently empty result keeps the previous output visible).
    #[must_use]
    pub fn is_renderable_empty(&self) -> bool {
        !self.meshes.iter().any(KernelMesh::is_renderable)
    }

    /// Total vertex count (the `points` cook statistic).
    #[must_use]
    pub fn point_count(&self) -> u64 {
        self.meshes.iter().map(|m| m.vertex_count() as u64).sum()
    }

    /// Whether any mesh carries a line or point topology. Triangle-only
    /// operators test this to warn once about pass-through meshes.
    #[must_use]
    pub fn has_non_triangle_meshes(&self) -> bool {
        self.meshes
            .iter()
            .any(|m| m.topology != MeshTopology::Triangles)
    }

    /// Total triangle count (the `prims` cook statistic). Deliberately
    /// triangles-only: line and point meshes contribute zero, so a pure
    /// point cloud truthfully reads "N pts, 0 prims".
    #[must_use]
    pub fn triangle_count(&self) -> u64 {
        self.meshes.iter().map(|m| m.triangle_count() as u64).sum()
    }

    #[must_use]
    pub fn mesh_count(&self) -> u32 {
        self.meshes.len() as u32
    }

    /// Recomputes union bounds after mesh mutation.
    pub fn recompute_bounds(&mut self) {
        self.bounds = union_bounds(&self.meshes);
    }

    /// Near-zero-copy conversion into the renderer contract: every buffer
    /// crosses by refcount bump, including the topology tag and the
    /// reserved `color` lane (lifted into [`CookedMesh::colors`] when it is
    /// a position-count Vec4; anything else stays kernel-side). Other
    /// [`AttributeMap`] channels do not cross.
    #[must_use]
    pub fn to_cooked(&self) -> CookedGeometry {
        CookedGeometry {
            meshes: self
                .meshes
                .iter()
                .map(|m| {
                    let colors = match m.attributes.get(reserved::COLOR) {
                        Some(AttributeData::Vec4(v)) if v.len() == m.positions.len() => {
                            Some(Arc::clone(v))
                        }
                        _ => None,
                    };
                    CookedMesh {
                        name: m.name.clone(),
                        positions: Arc::clone(&m.positions),
                        normals: m.normals.clone(),
                        tex_coords: m.tex_coords.clone(),
                        indices: Arc::clone(&m.indices),
                        material_index: m.material_index,
                        topology: m.topology,
                        colors,
                    }
                })
                .collect(),
            materials: self.materials.clone(),
            bounds: self.bounds,
        }
    }

    /// Ingests a loader result (the import-node path). Buffers move into
    /// `Arc`s without copying; `polygon_count` is dropped (the set derives
    /// triangle counts from indices).
    #[must_use]
    pub fn from_raw(raw: RawModelData) -> Self {
        let meshes = raw
            .meshes
            .into_iter()
            .map(|m| KernelMesh {
                name: m.name,
                positions: Arc::new(m.positions),
                normals: m.normals.map(Arc::new),
                tex_coords: m.tex_coords.map(Arc::new),
                indices: Arc::new(m.indices),
                material_index: m.material_index,
                topology: m.topology,
                attributes: AttributeMap::new(),
                primitive_attributes: AttributeMap::new(),
            })
            .collect();
        let materials = raw.materials.into_iter().map(Arc::new).collect();
        Self::from_parts(meshes, materials)
    }

    /// Rebuilds a [`RawModelData`] view (the validate-node path). This is
    /// the one deliberate full copy in the kernel: the validation pipeline
    /// owns plain `Vec` buffers. `polygon_count` is the triangle count (the
    /// set is already triangulated, so pre-triangulation polygons are gone).
    #[must_use]
    pub fn to_raw(&self) -> RawModelData {
        RawModelData {
            meshes: self
                .meshes
                .iter()
                .map(|m| RawMeshData {
                    name: m.name.clone(),
                    positions: (*m.positions).clone(),
                    indices: (*m.indices).clone(),
                    normals: m.normals.as_deref().cloned(),
                    tex_coords: m.tex_coords.as_deref().cloned(),
                    material_index: m.material_index,
                    topology: m.topology,
                })
                .collect(),
            materials: self.materials.iter().map(|m| (**m).clone()).collect(),
            polygon_count: self.triangle_count() as usize,
        }
    }
}

/// Union bounds over the renderable meshes; the `compute_bounds`
/// empty-input fallback when none are.
fn union_bounds(meshes: &[KernelMesh]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut any = false;
    for mesh in meshes {
        if mesh.positions.is_empty() {
            continue;
        }
        any = true;
        let b = mesh.bounds();
        let bmin = [b.min.x, b.min.y, b.min.z];
        let bmax = [b.max.x, b.max.y, b.max.z];
        for i in 0..3 {
            min[i] = min[i].min(bmin[i]);
            max[i] = max[i].max(bmax[i]);
        }
    }
    if !any {
        return compute_bounds(&[]);
    }
    AABB {
        min: cgmath::Point3::new(min[0], min[1], min[2]),
        max: cgmath::Point3::new(max[0], max[1], max[2]),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    fn material(name: &str) -> RawMaterialData {
        RawMaterialData {
            name: name.to_string(),
            diffuse_texture_path: None,
            normal_texture_path: None,
            diffuse_texture_data: None,
            normal_texture_data: None,
            metallic_roughness_texture_path: None,
            metallic_roughness_texture_data: None,
            occlusion_texture_path: None,
            occlusion_texture_data: None,
            emissive_texture_path: None,
            emissive_texture_data: None,
            roughness_factor: 0.5,
            metallic_factor: 0.0,
            occlusion_strength: 1.0,
            emissive_factor: [0.0; 3],
            base_color_factor: [1.0, 1.0, 1.0, 1.0],
            alpha_mode: solarxy_core::geometry::AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            shading_model: solarxy_core::geometry::ShadingModel::default(),
            toon_steps: 3.0,
            ambient: None,
            diffuse: None,
            specular: None,
            shininess: None,
            dissolve: None,
            optical_density: None,
            ambient_texture_name: None,
            diffuse_texture_name: None,
            specular_texture_name: None,
            normal_texture_name: None,
            shininess_texture_name: None,
            dissolve_texture_name: None,
        }
    }

    fn tri_mesh(name: &str, offset: f32) -> KernelMesh {
        KernelMesh::new(
            name,
            vec![
                [offset, 0.0, 0.0],
                [offset + 1.0, 0.0, 0.0],
                [offset, 1.0, 0.0],
            ],
            vec![0, 1, 2],
        )
    }

    #[test]
    fn empty_set_is_renderable_empty_with_fallback_bounds() {
        let set = GeometrySet::empty();
        assert!(set.is_renderable_empty());
        assert_eq!(set.point_count(), 0);
        assert_eq!(set.triangle_count(), 0);
        assert_eq!(set.mesh_count(), 0);
        // compute_bounds empty-input convention.
        assert_eq!(
            [set.bounds.min.x, set.bounds.min.y, set.bounds.min.z],
            [-1.0, -1.0, -1.0]
        );
        assert_eq!(
            [set.bounds.max.x, set.bounds.max.y, set.bounds.max.z],
            [1.0, 1.0, 1.0]
        );
    }

    #[test]
    fn mesh_with_positions_but_no_indices_is_not_renderable() {
        let mesh = KernelMesh::new("m", vec![[0.0; 3]], vec![]);
        assert!(!mesh.is_renderable());
        let set = GeometrySet::from_mesh(mesh);
        assert!(set.is_renderable_empty());
    }

    #[test]
    fn union_bounds_span_all_meshes() {
        let set = GeometrySet::from_parts(vec![tri_mesh("a", 0.0), tri_mesh("b", 10.0)], vec![]);
        assert_eq!(
            [set.bounds.min.x, set.bounds.min.y, set.bounds.min.z],
            [0.0, 0.0, 0.0]
        );
        assert_eq!(
            [set.bounds.max.x, set.bounds.max.y, set.bounds.max.z],
            [11.0, 1.0, 0.0]
        );
        assert_eq!(set.point_count(), 6);
        assert_eq!(set.triangle_count(), 2);
        assert_eq!(set.mesh_count(), 2);
    }

    #[test]
    fn to_cooked_shares_buffers_by_refcount() {
        let mut mesh = tri_mesh("a", 0.0);
        mesh.recompute_normals();
        mesh.material_index = Some(0);
        let set = GeometrySet::from_parts(vec![mesh], vec![Arc::new(material("mat"))]);

        let cooked = set.to_cooked();
        assert_eq!(cooked.meshes.len(), 1);
        assert!(Arc::ptr_eq(
            &cooked.meshes[0].positions,
            &set.meshes[0].positions
        ));
        assert!(Arc::ptr_eq(
            &cooked.meshes[0].indices,
            &set.meshes[0].indices
        ));
        assert!(Arc::ptr_eq(
            cooked.meshes[0].normals.as_ref().unwrap(),
            set.meshes[0].normals.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(&cooked.materials[0], &set.materials[0]));
        assert_eq!(cooked.meshes[0].material_index, Some(0));
        assert_eq!(cooked.bounds.min.x, set.bounds.min.x);
        assert_eq!(cooked.bounds.max.x, set.bounds.max.x);
    }

    /// The W2a contract: topology crosses into the renderer contract, and
    /// the reserved `color` lane lifts into `CookedMesh::colors` by
    /// refcount when (and only when) it is a position-count Vec4.
    #[test]
    fn to_cooked_maps_topology_and_lifts_the_color_lane() {
        let mut cloud = KernelMesh::points("cloud", vec![[0.0; 3], [1.0; 3]]);
        let lane = Arc::new(vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]]);
        cloud.attributes.insert(
            reserved::COLOR.to_string(),
            AttributeData::Vec4(Arc::clone(&lane)),
        );
        let mut wire = KernelMesh::polyline("wire", vec![[0.0; 3], [1.0; 3]], vec![0, 1]);
        wire.attributes.insert(
            reserved::COLOR.to_string(),
            AttributeData::Vec4(Arc::new(vec![[1.0; 4]])),
        );
        let set = GeometrySet::from_parts(vec![cloud, wire], vec![]);

        let cooked = set.to_cooked();
        assert_eq!(cooked.meshes[0].topology, MeshTopology::Points);
        assert!(
            Arc::ptr_eq(cooked.meshes[0].colors.as_ref().unwrap(), &lane),
            "the color lane crosses by refcount bump"
        );
        assert_eq!(cooked.meshes[1].topology, MeshTopology::Lines);
        assert!(
            cooked.meshes[1].colors.is_none(),
            "a length-mismatched lane stays kernel-side"
        );
    }

    #[test]
    fn raw_round_trip_preserves_validated_fields() {
        let raw = RawModelData {
            meshes: vec![RawMeshData {
                name: "m".to_string(),
                positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
                normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
                tex_coords: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]),
                material_index: Some(0),
                topology: MeshTopology::Triangles,
            }],
            materials: vec![material("mat")],
            polygon_count: 1,
        };

        let set = GeometrySet::from_raw(raw);
        assert_eq!(set.mesh_count(), 1);
        assert_eq!(set.materials.len(), 1);

        let back = set.to_raw();
        assert_eq!(back.meshes.len(), 1);
        let m = &back.meshes[0];
        assert_eq!(m.name, "m");
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.indices, vec![0, 1, 2]);
        assert_eq!(m.normals.as_ref().map(Vec::len), Some(3));
        assert_eq!(m.tex_coords.as_ref().map(Vec::len), Some(3));
        assert_eq!(m.material_index, Some(0));
        assert_eq!(back.materials[0].name, "mat");
        // Triangulated set: polygon count equals triangle count.
        assert_eq!(back.polygon_count, 1);
    }

    #[test]
    fn recompute_normals_produces_unit_normals() {
        let mut mesh = tri_mesh("a", 0.0);
        mesh.recompute_normals();
        let normals = mesh.normals.as_ref().unwrap();
        assert_eq!(normals.len(), 3);
        for n in normals.iter() {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-6);
            // CCW triangle in the XY plane faces +Z.
            assert!(n[2] > 0.99);
        }
    }

    #[test]
    fn attribute_data_len_matches_buffer() {
        let a = AttributeData::Vec4(Arc::new(vec![[1.0, 0.0, 0.0, 1.0]; 5]));
        assert_eq!(a.len(), 5);
        assert!(!a.is_empty());
        let mut map = AttributeMap::new();
        map.insert("color".to_string(), a);
        assert!(map.contains_key("color"));
    }

    #[test]
    fn renderability_truth_table() {
        // Triangles: need at least one full triple.
        assert!(tri_mesh("t", 0.0).is_renderable());
        assert!(!KernelMesh::new("t", vec![[0.0; 3]; 3], vec![0, 1]).is_renderable());
        assert!(!KernelMesh::new("t", vec![], vec![]).is_renderable());
        // Lines: need at least one full pair.
        assert!(KernelMesh::polyline("l", vec![[0.0; 3], [1.0; 3]], vec![0, 1]).is_renderable());
        assert!(!KernelMesh::polyline("l", vec![[0.0; 3], [1.0; 3]], vec![0]).is_renderable());
        assert!(!KernelMesh::polyline("l", vec![], vec![]).is_renderable());
        // Points: any position renders; indices are irrelevant.
        assert!(KernelMesh::points("p", vec![[0.0; 3]]).is_renderable());
        assert!(!KernelMesh::points("p", vec![]).is_renderable());
    }

    #[test]
    fn topology_counts() {
        let tri = tri_mesh("t", 0.0);
        assert_eq!(tri.triangle_count(), 1);
        assert_eq!(tri.primitive_count(), 1);

        let line = KernelMesh::polyline("l", vec![[0.0; 3], [1.0; 3], [2.0; 3]], vec![0, 1, 1, 2]);
        assert_eq!(line.triangle_count(), 0);
        assert_eq!(line.primitive_count(), 2);

        let pts = KernelMesh::points("p", vec![[0.0; 3]; 5]);
        assert_eq!(pts.triangle_count(), 0);
        assert_eq!(pts.primitive_count(), 5);

        // Set-level: prims stays triangles-only, points counts vertices.
        let set = GeometrySet::from_parts(vec![tri, line, pts], vec![]);
        assert_eq!(set.triangle_count(), 1);
        assert_eq!(set.point_count(), 3 + 3 + 5);
    }

    #[test]
    fn point_cloud_set_is_not_renderable_empty() {
        let set = GeometrySet::from_mesh(KernelMesh::points("p", vec![[0.0; 3], [2.0, 4.0, 6.0]]));
        assert!(!set.is_renderable_empty());
        // Bounds span the positions like any other mesh.
        assert_eq!(
            [set.bounds.max.x, set.bounds.max.y, set.bounds.max.z],
            [2.0, 4.0, 6.0]
        );
    }

    #[test]
    fn raw_round_trip_preserves_topology() {
        let set = GeometrySet::from_parts(
            vec![
                KernelMesh::points("p", vec![[0.0; 3]]),
                KernelMesh::polyline("l", vec![[0.0; 3], [1.0; 3]], vec![0, 1]),
            ],
            vec![],
        );
        let raw = set.to_raw();
        assert_eq!(raw.meshes[0].topology, MeshTopology::Points);
        assert_eq!(raw.meshes[1].topology, MeshTopology::Lines);
        let back = GeometrySet::from_raw(raw);
        assert_eq!(back.meshes[0].topology, MeshTopology::Points);
        assert_eq!(back.meshes[1].topology, MeshTopology::Lines);
    }

    #[test]
    fn domain_attributes_route_to_their_maps() {
        let mut mesh = tri_mesh("t", 0.0);
        mesh.domain_attributes_mut(AttributeDomain::Point).insert(
            reserved::COLOR.to_string(),
            AttributeData::Vec4(Arc::new(vec![[1.0, 0.0, 0.0, 1.0]; 3])),
        );
        mesh.domain_attributes_mut(AttributeDomain::Primitive)
            .insert(
                "area".to_string(),
                AttributeData::Float(Arc::new(vec![0.5])),
            );
        // The point-domain accessor is the plain `attributes` field.
        assert!(mesh.attributes.contains_key(reserved::COLOR));
        assert!(
            mesh.domain_attributes(AttributeDomain::Point)
                .contains_key(reserved::COLOR)
        );
        assert!(
            mesh.domain_attributes(AttributeDomain::Primitive)
                .contains_key("area")
        );
        assert!(!mesh.attributes.contains_key("area"));
    }

    #[test]
    fn recompute_normals_is_a_noop_off_triangles() {
        let mut pts = KernelMesh::points("p", vec![[0.0; 3]; 4]);
        pts.recompute_normals();
        assert!(pts.normals.is_none());

        let mut line = KernelMesh::polyline("l", vec![[0.0; 3], [1.0; 3]], vec![0, 1]);
        line.recompute_normals();
        assert!(line.normals.is_none());
    }
}
