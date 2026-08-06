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
use solarxy_core::scene::{CookedGeometry, CookedMesh, InstanceXform};

/// Which element of a mesh an attribute lane describes: one value per
/// point (vertex) or one value per primitive (triangle, segment, or point
/// primitive, per the mesh topology). Most producers write the point
/// domain; `attribute_promote` and `attribute_copy` populate the
/// primitive domain.
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
    /// Per-point uniform scale, `Float`. Consumed by `copy_to_points`,
    /// which MULTIPLIES its own Scale parameter by this lane so the
    /// parameter stays a global dial. Authored by `attribute_wrangle`,
    /// which is what made the lane useful: it was reserved in 0.8.0 with
    /// nothing in the product able to write it.
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
    /// or point primitive). Written by `attribute_promote` and
    /// `attribute_copy`; read back through the attribute table's
    /// Primitive tab.
    pub primitive_attributes: AttributeMap,
    /// Per-instance placements for this mesh, or `None` for the ordinary
    /// single-placement case.
    ///
    /// `None` preserves the pre-instancing meaning exactly: one implicit
    /// identity placement.
    ///
    /// It sits on the mesh rather than on [`GeometrySet`] because a set can
    /// hold meshes placed differently. Merging a scatter's prototype with
    /// the surface it was scattered over produces one set holding one
    /// instanced mesh and one plain one, and a single list for the whole set
    /// cannot express that: it would replicate the surface too. A multi-mesh
    /// prototype still stays rigid within each copy, because the copy
    /// operations hand every one of its meshes the same shared list.
    ///
    /// **An operation that cannot carry placements through must call
    /// [`GeometrySet::baked`] first** rather than dropping them. Silently
    /// losing the list deletes every copy but one with no error anywhere,
    /// which is the worst failure this representation can have.
    pub instances: Option<Arc<Vec<InstanceXform>>>,
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
            instances: None,
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

    /// Tight bounds over this mesh's positions, in the mesh's own space and
    /// ignoring its placements (the empty-input fallback follows
    /// `compute_bounds`). [`Self::placed_bounds`] is the one to frame with.
    #[must_use]
    pub fn bounds(&self) -> AABB {
        compute_bounds(&self.positions)
    }

    /// Bounds over every placement of this mesh, which is what a camera
    /// should frame: the local box alone would put the camera inside one
    /// rock of a ten-thousand-copy scatter.
    #[must_use]
    pub fn placed_bounds(&self) -> AABB {
        let local = self.bounds();
        match &self.instances {
            Some(list) => instanced_bounds(&local, list),
            None => local,
        }
    }

    /// How many placements this mesh draws: the placement count, or 1 for
    /// the implicit identity.
    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.instances.as_ref().map_or(1, |i| i.len())
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
    /// Union bounds over all renderable meshes, **including every
    /// placement** (object space).
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

    /// A set from meshes + materials, computing union bounds. Each mesh
    /// keeps whatever placements it already carries, so this is also the
    /// constructor a merge uses.
    #[must_use]
    pub fn from_parts(meshes: Vec<KernelMesh>, materials: Vec<Arc<RawMaterialData>>) -> Self {
        let bounds = union_bounds(&meshes);
        Self {
            meshes,
            materials,
            bounds,
        }
    }

    /// A set whose every mesh is placed once per instance: the copy
    /// operations' constructor.
    ///
    /// All the meshes share **one** placement list, by the same `Arc`, which
    /// is what keeps a multi-mesh prototype rigid within each copy: they
    /// cannot disagree because there is only one list to read.
    ///
    /// Bounds union the prototype's corners over every placement, not the
    /// prototype alone: framing a ten-thousand-copy scatter on the box at
    /// the origin would put the camera inside one rock.
    #[must_use]
    pub fn from_parts_instanced(
        meshes: Vec<KernelMesh>,
        materials: Vec<Arc<RawMaterialData>>,
        instances: Vec<InstanceXform>,
    ) -> Self {
        let shared = Arc::new(instances);
        let meshes = meshes
            .into_iter()
            .map(|mut mesh| {
                mesh.instances = Some(Arc::clone(&shared));
                mesh
            })
            .collect();
        Self::from_parts(meshes, materials)
    }

    /// Whether any mesh in this set carries placements.
    #[must_use]
    pub fn is_instanced(&self) -> bool {
        self.meshes.iter().any(|m| m.instances.is_some())
    }

    /// The primitive count baking this set would produce, each mesh
    /// measured under its own topology and multiplied by its own placement
    /// count. Saturating, so a runaway count reports the ceiling rather
    /// than wrapping to a small number that passes the check.
    #[must_use]
    pub fn baked_primitive_projection(&self) -> usize {
        self.meshes
            .iter()
            .map(|m| {
                m.primitive_count()
                    .max(m.vertex_count())
                    .saturating_mul(m.instance_count())
            })
            .fold(0, usize::saturating_add)
    }

    /// This set with every placement baked into real geometry, or itself
    /// when no mesh carries any.
    ///
    /// The escape hatch for operations that cannot carry placements
    /// through. Collapsing is slow and can hit the primitive ceiling,
    /// which is the honest outcome: the alternative is an operation that
    /// silently returns one copy where the user placed ten thousand.
    ///
    /// # Errors
    /// Propagates the bake's error, and the ceiling error when the
    /// collapsed output would exceed it.
    pub fn baked(&self) -> Result<std::borrow::Cow<'_, Self>, String> {
        if !self.is_instanced() {
            return Ok(std::borrow::Cow::Borrowed(self));
        }
        let projected = self.baked_primitive_projection();
        if projected > crate::array::MAX_OUTPUT_PRIMITIVES {
            return Err(format!(
                "baking {} instances would produce {projected} primitives (over the {} \
                 ceiling); this operation cannot work on instanced geometry, so either \
                 reduce the copy count or keep the copies as instances and move this \
                 operation before the one that creates them",
                self.instance_count(),
                crate::array::MAX_OUTPUT_PRIMITIVES
            ));
        }

        let mut out: Vec<KernelMesh> = Vec::with_capacity(self.meshes.len());
        for mesh in &self.meshes {
            let Some(placements) = &mesh.instances else {
                out.push(mesh.clone());
                continue;
            };
            // One set per mesh, not per placement: the prototype is built
            // once and each placement bakes a transform of it.
            let mut prototype = mesh.clone();
            prototype.instances = None;
            let one = Self::from_parts(vec![prototype], self.materials.clone());
            for placement in placements.iter() {
                let matrix = cgmath::Matrix4::from(placement.0);
                let baked = crate::transform::bake_transform(&one, &matrix)
                    .map_err(|e: crate::KernelError| e.to_string())?;
                out.extend(baked.meshes);
            }
        }
        Ok(std::borrow::Cow::Owned(Self::from_parts(
            out,
            self.materials.clone(),
        )))
    }

    /// The largest placement count over this set's meshes, or 1 when none
    /// carries any.
    ///
    /// A set can hold meshes placed differently, so there is no single
    /// count for the whole set. This is the "how many copies is this
    /// scatter" number a message quotes, not a value to drive a draw with:
    /// the renderer reads each mesh's own count.
    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.meshes
            .iter()
            .map(KernelMesh::instance_count)
            .max()
            .unwrap_or(1)
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
                        instances: m.instances.clone(),
                    }
                })
                .collect(),
            materials: self.materials.clone(),
            bounds: self.bounds,
        }
    }

    /// Ingests a loader result (the import-node path). Buffers move into
    /// `Arc`s without copying; `polygon_count` is dropped (the set derives
    /// triangle counts from indices). Loader colors lift into the reserved
    /// `color` lane when position-count length (a mismatch is
    /// loader-invalid and dropped).
    #[must_use]
    pub fn from_raw(raw: RawModelData) -> Self {
        let meshes = raw
            .meshes
            .into_iter()
            .map(|m| {
                let mut attributes = AttributeMap::new();
                if let Some(colors) = m.colors
                    && colors.len() == m.positions.len()
                {
                    attributes.insert(
                        reserved::COLOR.to_string(),
                        AttributeData::Vec4(Arc::new(colors)),
                    );
                }
                KernelMesh {
                    name: m.name,
                    positions: Arc::new(m.positions),
                    normals: m.normals.map(Arc::new),
                    tex_coords: m.tex_coords.map(Arc::new),
                    indices: Arc::new(m.indices),
                    material_index: m.material_index,
                    topology: m.topology,
                    attributes,
                    primitive_attributes: AttributeMap::new(),
                    instances: None,
                }
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
                    // The reserved color lane lowers symmetrically with
                    // `from_raw`'s lift, so raw round trips keep colors.
                    colors: match m.attributes.get(reserved::COLOR) {
                        Some(AttributeData::Vec4(v)) if v.len() == m.positions.len() => {
                            Some((**v).clone())
                        }
                        _ => None,
                    },
                })
                .collect(),
            materials: self.materials.iter().map(|m| (**m).clone()).collect(),
            polygon_count: self.triangle_count() as usize,
        }
    }
}

/// Union bounds over the renderable meshes; the `compute_bounds`
/// empty-input fallback when none are.
/// Union the prototype's bounds over every placement.
///
/// Transforms the eight corners of the local box rather than the box's
/// min/max directly: a rotated placement's axis-aligned box is not the
/// rotation of the min and max points, and using those would give a box
/// too small to contain the geometry.
fn instanced_bounds(local: &AABB, instances: &[InstanceXform]) -> AABB {
    if instances.is_empty() {
        return compute_bounds(&[]);
    }
    let corners = [
        [local.min.x, local.min.y, local.min.z],
        [local.max.x, local.min.y, local.min.z],
        [local.min.x, local.max.y, local.min.z],
        [local.max.x, local.max.y, local.min.z],
        [local.min.x, local.min.y, local.max.z],
        [local.max.x, local.min.y, local.max.z],
        [local.min.x, local.max.y, local.max.z],
        [local.max.x, local.max.y, local.max.z],
    ];
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for placement in instances {
        for corner in &corners {
            let p = placement.apply(*corner);
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
    }
    AABB {
        min: min.into(),
        max: max.into(),
    }
}

fn union_bounds(meshes: &[KernelMesh]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut any = false;
    for mesh in meshes {
        if mesh.positions.is_empty() {
            continue;
        }
        any = true;
        let b = mesh.placed_bounds();
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
            roughness_factor: 0.5,
            alpha_cutoff: 0.5,
            ..RawMaterialData::default()
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

    /// The topology contract: topology crosses into the renderer contract, and
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
                colors: None,
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

    /// Loader colors lift into the reserved lane on `from_raw` and
    /// lower back on `to_raw`; a length-mismatched array is dropped.
    #[test]
    fn raw_colors_lift_into_the_lane_and_lower_back() {
        let colors = vec![[0.25, 0.5, 0.75, 1.0]; 3];
        let raw = RawModelData {
            meshes: vec![
                RawMeshData {
                    name: "colored".to_string(),
                    positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    indices: vec![0, 1, 2],
                    normals: None,
                    tex_coords: None,
                    material_index: None,
                    topology: MeshTopology::Triangles,
                    colors: Some(colors.clone()),
                },
                RawMeshData {
                    name: "bad".to_string(),
                    positions: vec![[0.0; 3], [1.0; 3]],
                    indices: vec![],
                    normals: None,
                    tex_coords: None,
                    material_index: None,
                    topology: MeshTopology::Points,
                    colors: Some(vec![[1.0; 4]]),
                },
            ],
            materials: vec![],
            polygon_count: 1,
        };
        let set = GeometrySet::from_raw(raw);
        assert_eq!(
            set.meshes[0].attributes.get(reserved::COLOR),
            Some(&AttributeData::Vec4(Arc::new(colors.clone())))
        );
        assert!(
            !set.meshes[1].attributes.contains_key(reserved::COLOR),
            "length mismatch is loader-invalid and dropped"
        );
        let back = set.to_raw();
        assert_eq!(back.meshes[0].colors, Some(colors));
        assert_eq!(back.meshes[1].colors, None);
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

#[cfg(test)]
mod instance_tests {
    use super::*;

    fn unit_cube() -> GeometrySet {
        GeometrySet::from_mesh(KernelMesh::new(
            "proto",
            vec![
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
            vec![0, 1, 2, 0, 2, 3],
        ))
    }

    fn translation(x: f32, y: f32, z: f32) -> InstanceXform {
        InstanceXform([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [x, y, z, 1.0],
        ])
    }

    /// A multi-mesh prototype stays rigid within each copy because every
    /// one of its meshes reads the SAME list, not merely an equal one.
    ///
    /// This is what the set-level placement bought and what moving it to
    /// the mesh had to keep. Equality would not be enough: two lists that
    /// happen to match today can be edited apart tomorrow, and a prototype
    /// whose meshes disagree on where copy 4,271 sits is a shape that comes
    /// apart as it repeats.
    #[test]
    fn a_multi_mesh_prototype_shares_one_placement_list() {
        let set = GeometrySet::from_parts_instanced(
            vec![
                KernelMesh::new("a", vec![[0.0; 3]], vec![]),
                KernelMesh::new("b", vec![[1.0, 0.0, 0.0]], vec![]),
            ],
            Vec::new(),
            vec![translation(0.0, 0.0, 0.0), translation(5.0, 0.0, 0.0)],
        );
        let a = set.meshes[0].instances.as_ref().expect("placements");
        let b = set.meshes[1].instances.as_ref().expect("placements");
        assert!(
            Arc::ptr_eq(a, b),
            "the prototype's meshes must share one allocation, not two equal ones"
        );
    }

    /// Meshes in one set can be placed differently, which is the property a
    /// single set-level list could not express and the reason merge can
    /// carry instancing through at all.
    #[test]
    fn meshes_in_one_set_can_be_placed_independently() {
        let mut instanced = KernelMesh::new("scattered", vec![[0.0; 3]], vec![]);
        instanced.instances = Some(Arc::new(vec![
            translation(0.0, 0.0, 0.0),
            translation(7.0, 0.0, 0.0),
        ]));
        let plain = KernelMesh::new("ground", vec![[0.0; 3]], vec![]);
        let set = GeometrySet::from_parts(vec![instanced, plain], Vec::new());

        assert_eq!(set.meshes[0].instance_count(), 2);
        assert_eq!(set.meshes[1].instance_count(), 1);
        assert!(set.is_instanced());
        // Bounds reach the far placement without dragging the plain mesh
        // along with it.
        assert!((set.bounds.max.x - 7.0).abs() < 1e-5, "{:?}", set.bounds);
    }

    #[test]
    fn a_set_with_no_instances_still_draws_once() {
        // `None` has to keep meaning exactly one placement, or every
        // existing scene stops rendering.
        assert_eq!(unit_cube().instance_count(), 1);
        assert!(!unit_cube().is_instanced());
    }

    #[test]
    fn instanced_bounds_cover_the_placements_not_the_prototype() {
        // Framing is computed from bounds. A ten-thousand-copy scatter
        // whose bounds described only the prototype would put the camera
        // inside one copy.
        let set = GeometrySet::from_parts_instanced(
            unit_cube().meshes,
            Vec::new(),
            vec![translation(0.0, 0.0, 0.0), translation(10.0, 0.0, 0.0)],
        );
        assert!((set.bounds.min.x - -0.5).abs() < 1e-5, "{:?}", set.bounds);
        assert!((set.bounds.max.x - 10.5).abs() < 1e-5, "{:?}", set.bounds);
        assert_eq!(set.instance_count(), 2);
    }

    #[test]
    fn instanced_bounds_of_a_rotated_placement_contain_the_geometry() {
        // The corners are transformed, not the min and max points: the
        // axis-aligned box of a rotated box is not the rotation of its
        // two extreme corners, and using those gives a box too small.
        let c = std::f32::consts::FRAC_1_SQRT_2;
        let spun = InstanceXform([
            [c, c, 0.0, 0.0],
            [-c, c, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let set = GeometrySet::from_parts_instanced(unit_cube().meshes, Vec::new(), vec![spun]);
        // A unit square spun 45 degrees needs a box of half-diagonal
        // 0.707, not the 0.5 an untransformed min/max would give.
        assert!(set.bounds.max.x > 0.7, "{:?}", set.bounds);
        assert!(set.bounds.max.y > 0.7, "{:?}", set.bounds);
    }

    #[test]
    fn baking_instances_matches_placing_the_prototype_by_hand() {
        // The parity that makes Instance a representation choice rather
        // than a rendering change: the baked result of N instances is the
        // same geometry as N transformed copies merged.
        let places = [translation(1.0, 2.0, 3.0), translation(-4.0, 0.5, 0.0)];
        let instanced =
            GeometrySet::from_parts_instanced(unit_cube().meshes, Vec::new(), places.to_vec());
        let baked = instanced.baked().expect("bake");

        let by_hand: Vec<Arc<GeometrySet>> = places
            .iter()
            .map(|p| {
                let m = cgmath::Matrix4::from(p.0);
                Arc::new(crate::transform::bake_transform(&unit_cube(), &m).expect("bake"))
            })
            .collect();
        let expected = crate::merge::merge(&by_hand);

        let got: Vec<[f32; 3]> = baked
            .meshes
            .iter()
            .flat_map(|m| m.positions.to_vec())
            .collect();
        let want: Vec<[f32; 3]> = expected
            .meshes
            .iter()
            .flat_map(|m| m.positions.to_vec())
            .collect();
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want.iter()) {
            for axis in 0..3 {
                assert!((g[axis] - w[axis]).abs() < 1e-5, "{g:?} vs {w:?}");
            }
        }
    }

    #[test]
    fn baking_an_uninstanced_set_borrows_it_rather_than_copying() {
        // The escape hatch is on the hot path of every operation that
        // cannot carry instances, so the overwhelmingly common case (no
        // instances at all) must not allocate.
        let set = unit_cube();
        let baked = set.baked().expect("bake");
        assert!(matches!(baked, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn baking_past_the_ceiling_names_the_way_out() {
        // G-14: the error has to say what to do, and "lower the point
        // count" is the wrong advice when keeping the copies as instances
        // would have worked.
        let many = vec![translation(0.0, 0.0, 0.0); 5_000_000];
        let set = GeometrySet::from_parts_instanced(unit_cube().meshes, Vec::new(), many);
        let err = set.baked().expect_err("over the ceiling");
        assert!(err.contains("instances"), "{err}");
        assert!(err.contains("ceiling"), "{err}");
    }

    #[test]
    fn instances_survive_the_trip_to_cooked_geometry() {
        // The renderer reads the count off the cooked side; losing the
        // list here would draw one copy and silently discard the rest.
        let set = GeometrySet::from_parts_instanced(
            unit_cube().meshes,
            Vec::new(),
            vec![translation(0.0, 0.0, 0.0), translation(1.0, 0.0, 0.0)],
        );
        let cooked = set.to_cooked();
        assert_eq!(cooked.meshes[0].instance_count(), 2);
        assert_eq!(cooked.bounds.max.x, set.bounds.max.x);
    }
}
