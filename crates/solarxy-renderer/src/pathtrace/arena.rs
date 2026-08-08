//! The traced scene's buffer packing: many meshes and placements concatenated
//! into the handful of storage buffers the kernel binds.
//!
//! This is the packing layer only. Ingesting a `SceneDelta`, caching a
//! hierarchy per object and deciding when to rebuild are a separate concern
//! that sits on top of this one, so that the arithmetic every shader depends on
//! has somewhere to be tested without a scene graph in the way.
//!
//! # Why the node indices stay relative
//!
//! Each hierarchy's node indices are left exactly as its builder emitted them,
//! relative to its own first node, and the kernel adds the instance's
//! [`Instance::bvh_root`] at the point it fetches a node. Rebasing them at pack
//! time would save that one integer add per node visit, and it is the wrong
//! trade: a hierarchy is built once per mesh and cached, while the arena is
//! repacked whenever anything moves. Rebased nodes would have to be rewritten
//! on every repack, which is the cache the two-level structure exists to keep.
//! The same argument covers a leaf's primitive offset, which the kernel resolves
//! against [`Instance::prim_base`].

use bytemuck::{Pod, Zeroable};
use solarxy_bvh::{Bvh, BvhNode};

use super::material::TracedMaterial;

/// Bit 0 of [`Instance::flags`]: the instance is drawn.
pub const INSTANCE_VISIBLE: u32 = 1 << 0;
/// Bit 1 of [`Instance::flags`]: the instance blocks shadow rays.
pub const INSTANCE_CAST_SHADOW: u32 = 1 << 1;

/// One placement of one mesh, as the kernel reads it.
///
/// The three bases are what make a shared hierarchy work: several instances
/// name the same `bvh_root`, `prim_base`, `index_base` and `vertex_base` and
/// differ only in their transform and material.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct Instance {
    /// Object to world, column-major.
    pub world: [[f32; 4]; 4],
    /// World to object, column-major. Assumed affine.
    pub inv_world: [[f32; 4]; 4],
    /// First node of this instance's hierarchy in the node buffer.
    pub bvh_root: u32,
    /// Where this hierarchy's primitive permutation starts in `prim_indices`.
    pub prim_base: u32,
    /// Where this mesh's triangle index triples start in `prim_indices`.
    pub index_base: u32,
    /// Where this mesh's vertices start in `vertex_pos` and `vertex_attr`.
    pub vertex_base: u32,
    /// First material of this instance in the material buffer.
    pub material_base: u32,
    /// [`INSTANCE_VISIBLE`] and [`INSTANCE_CAST_SHADOW`].
    pub flags: u32,
    pub _pad: [u32; 2],
}

const _: () = assert!(std::mem::size_of::<Instance>() == 160);

/// Per-vertex shading attributes, 48 bytes.
///
/// A zero normal means the source mesh carried none; the kernel falls back to
/// the triangle's geometric normal rather than shading a surface as if it faced
/// nowhere.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct VertexAttr {
    /// Object-space shading normal in `xyz`; `w` unused.
    pub normal: [f32; 4],
    /// Object-space tangent in `xyz`, handedness in `w`.
    pub tangent: [f32; 4],
    /// `uv0` in `xy`, `uv1` in `zw`.
    pub uv: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<VertexAttr>() == 48);

/// A mesh and the hierarchy already built over it.
///
/// The hierarchy is borrowed rather than built here because it is the expensive
/// half and it outlives any one packing: several instances share one, and a
/// mesh that has not changed keeps the one it had.
#[derive(Debug, Clone, Copy)]
pub struct ArenaMesh<'a> {
    pub bvh: &'a Bvh,
    pub positions: &'a [[f32; 3]],
    pub indices: &'a [u32],
    /// Object-space shading normals, one per position, when the source has
    /// them.
    pub normals: Option<&'a [[f32; 3]]>,
    /// `uv0`, one per position, when the source has it.
    pub uv0: Option<&'a [[f32; 2]]>,
}

/// Where a mesh goes.
#[derive(Debug, Clone, Copy)]
pub struct ArenaPlacement {
    /// Index into the mesh slice.
    pub mesh: u32,
    /// Object to world, column-major.
    pub world: [[f32; 4]; 4],
    /// World to object, column-major.
    pub inv_world: [[f32; 4]; 4],
    pub material_base: u32,
    pub flags: u32,
}

/// The packed scene: one buffer per binding of the kernel's scene group.
#[derive(Debug, Default, Clone)]
pub struct TraceArena {
    nodes: Vec<BvhNode>,
    prim_indices: Vec<u32>,
    vertex_pos: Vec<[f32; 4]>,
    vertex_attr: Vec<VertexAttr>,
    instances: Vec<Instance>,
    materials: Vec<TracedMaterial>,
}

impl TraceArena {
    /// Packs a top-level hierarchy over `placements` together with each mesh's
    /// own hierarchy.
    ///
    /// `tlas` must have been built by [`Bvh::build_tlas`] over the placements'
    /// world-space boxes, in placement order, so its leaves name entries of
    /// `placements`. It goes first in both the node and permutation buffers,
    /// which is what lets the kernel start at node zero with no offset.
    ///
    /// A placement naming a mesh the slice does not carry is dropped, for the
    /// same reason the traversal tolerates a bad index: this packs whatever a
    /// scene contained.
    #[must_use]
    pub fn build(tlas: &Bvh, meshes: &[ArenaMesh<'_>], placements: &[ArenaPlacement]) -> Self {
        let tlas_arrays = tlas.to_gpu_arrays();
        let mut arena = Self {
            nodes: tlas.nodes().to_vec(),
            prim_indices: tlas.prim_indices().to_vec(),
            vertex_pos: Vec::new(),
            vertex_attr: Vec::new(),
            instances: Vec::with_capacity(placements.len()),
            materials: Vec::new(),
        };
        debug_assert_eq!(tlas_arrays.nodes.len(), arena.nodes.len() * 32);

        // Pack each mesh once, recording where its four pieces landed. A mesh
        // no placement names is still packed, because a placement's `mesh`
        // field indexes this slice and skipping would shift every later index.
        let mut bases = Vec::with_capacity(meshes.len());
        for mesh in meshes {
            let base = MeshBase {
                bvh_root: u32::try_from(arena.nodes.len()).unwrap_or(u32::MAX),
                prim_base: u32::try_from(arena.prim_indices.len()).unwrap_or(u32::MAX),
                vertex_base: u32::try_from(arena.vertex_pos.len()).unwrap_or(u32::MAX),
                index_base: 0,
            };
            arena.nodes.extend_from_slice(mesh.bvh.nodes());
            arena
                .prim_indices
                .extend_from_slice(mesh.bvh.prim_indices());

            // The triangle indices ride the same buffer as the permutation,
            // after it. That is what keeps the scene group at seven storage
            // buffers instead of eight, and it costs one more base per mesh.
            let index_base = u32::try_from(arena.prim_indices.len()).unwrap_or(u32::MAX);
            arena.prim_indices.extend_from_slice(mesh.indices);

            for (i, p) in mesh.positions.iter().enumerate() {
                // `w` carries the packed vertex colour once the scene path
                // produces one. Zero reads as "none" and is not a colour, so a
                // consumer added later cannot mistake a placeholder for data.
                arena.vertex_pos.push([p[0], p[1], p[2], 0.0]);
                let normal = mesh.normals.and_then(|n| n.get(i)).copied();
                let uv = mesh.uv0.and_then(|u| u.get(i)).copied().unwrap_or([0.0; 2]);
                arena.vertex_attr.push(VertexAttr {
                    normal: normal.map_or([0.0; 4], |n| [n[0], n[1], n[2], 0.0]),
                    tangent: [0.0; 4],
                    uv: [uv[0], uv[1], 0.0, 0.0],
                });
            }

            bases.push(MeshBase { index_base, ..base });
        }

        for placement in placements {
            let Some(base) = bases.get(placement.mesh as usize) else {
                continue;
            };
            arena.instances.push(Instance {
                world: placement.world,
                inv_world: placement.inv_world,
                bvh_root: base.bvh_root,
                prim_base: base.prim_base,
                index_base: base.index_base,
                vertex_base: base.vertex_base,
                material_base: placement.material_base,
                flags: placement.flags,
                _pad: [0; 2],
            });
        }

        arena
    }

    /// Attaches the material pool an [`Instance::material_base`] indexes.
    ///
    /// A builder rather than a fourth parameter to [`TraceArena::build`], and
    /// deliberately: the records are built by the ingestion above this file
    /// from the scene's materials and its atlas arrangement, neither of which
    /// [`TraceArena::build`] is given or should be. Taking them as a parameter
    /// would also change every one of `build`'s call sites, all of which pack
    /// geometry and have no materials to hand it.
    ///
    /// Materials are per material slot rather than per mesh or per placement,
    /// so nothing here indexes or reorders them; they ride along because they
    /// are one buffer of the kernel's scene group, which is what this type is.
    #[must_use]
    pub fn with_materials(mut self, materials: Vec<TracedMaterial>) -> Self {
        self.materials = materials;
        self
    }

    /// The concatenated node buffer: the top-level hierarchy, then one per mesh.
    #[must_use]
    pub fn nodes(&self) -> &[BvhNode] {
        &self.nodes
    }

    /// The concatenated permutation and triangle-index buffer.
    #[must_use]
    pub fn prim_indices(&self) -> &[u32] {
        &self.prim_indices
    }

    /// Object-space positions; `w` is the packed vertex colour slot.
    #[must_use]
    pub fn vertex_pos(&self) -> &[[f32; 4]] {
        &self.vertex_pos
    }

    /// Per-vertex shading attributes, parallel to [`TraceArena::vertex_pos`].
    #[must_use]
    pub fn vertex_attr(&self) -> &[VertexAttr] {
        &self.vertex_attr
    }

    /// The placements, in the order the top-level hierarchy names them.
    #[must_use]
    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    /// The material pool, indexed by [`Instance::material_base`].
    ///
    /// Empty when nothing attached one, which is every caller that packs
    /// geometry alone. An empty pool uploads as one zeroed record, and a zeroed
    /// record is a wrong material rather than an absent one, so the ingestion
    /// that does attach a pool always emits at least one entry. See
    /// `TracedMaterial::fallback`.
    #[must_use]
    pub fn materials(&self) -> &[TracedMaterial] {
        &self.materials
    }

    /// Whether the arena holds nothing worth dispatching over.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty() || self.vertex_pos.is_empty()
    }
}

/// Where one mesh's four pieces landed in the arena.
#[derive(Debug, Clone, Copy)]
struct MeshBase {
    bvh_root: u32,
    prim_base: u32,
    index_base: u32,
    vertex_base: u32,
}

#[cfg(test)]
mod tests {
    use super::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, Instance, TraceArena, VertexAttr};
    use solarxy_bvh::{Bvh, corpus};

    fn identity() -> [[f32; 4]; 4] {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    fn placement(mesh: u32) -> ArenaPlacement {
        ArenaPlacement {
            mesh,
            world: identity(),
            inv_world: identity(),
            material_base: 0,
            flags: INSTANCE_VISIBLE,
        }
    }

    #[test]
    fn the_records_are_the_sizes_the_shader_declares() {
        assert_eq!(std::mem::size_of::<Instance>(), 160);
        assert_eq!(std::mem::size_of::<VertexAttr>(), 48);
    }

    #[test]
    fn the_top_level_hierarchy_starts_at_node_zero() {
        let (positions, indices) = corpus::sphere(8, 4);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let mesh = ArenaMesh {
            bvh: &bvh,
            positions: &positions,
            indices: &indices,
            normals: None,
            uv0: None,
        };
        let boxes = [solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-1.0, -1.0, -1.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        }];
        let tlas = Bvh::build_tlas(&boxes);
        let arena = TraceArena::build(&tlas, &[mesh], &[placement(0)]);

        assert_eq!(&arena.nodes()[..tlas.nodes().len()], tlas.nodes());
        assert_eq!(arena.instances()[0].bvh_root as usize, tlas.nodes().len());
        assert_eq!(
            arena.instances()[0].prim_base as usize,
            tlas.prim_indices().len()
        );
        assert_eq!(arena.instances()[0].vertex_base, 0);
    }

    #[test]
    fn the_hierarchy_nodes_are_copied_unrebased() {
        // The cache the two-level structure exists for only works if a packed
        // hierarchy is byte-identical to the one the builder emitted.
        let (positions, indices) = corpus::sphere(8, 4);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let mesh = ArenaMesh {
            bvh: &bvh,
            positions: &positions,
            indices: &indices,
            normals: None,
            uv0: None,
        };
        let boxes = [solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-1.0, -1.0, -1.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        }];
        let tlas = Bvh::build_tlas(&boxes);
        let arena = TraceArena::build(&tlas, &[mesh], &[placement(0), placement(0)]);

        let root = arena.instances()[0].bvh_root as usize;
        assert_eq!(&arena.nodes()[root..root + bvh.nodes().len()], bvh.nodes());
        // Two placements of one mesh share every base: that is the whole
        // point of instancing.
        assert_eq!(arena.instances()[0].bvh_root, arena.instances()[1].bvh_root);
        assert_eq!(
            arena.instances()[0].vertex_base,
            arena.instances()[1].vertex_base
        );
        assert_eq!(arena.vertex_pos().len(), positions.len());
    }

    #[test]
    fn the_triangle_indices_follow_the_permutation_in_one_buffer() {
        let (positions, indices) = corpus::sphere(8, 4);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let mesh = ArenaMesh {
            bvh: &bvh,
            positions: &positions,
            indices: &indices,
            normals: None,
            uv0: None,
        };
        let boxes = [solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-1.0, -1.0, -1.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        }];
        let tlas = Bvh::build_tlas(&boxes);
        let arena = TraceArena::build(&tlas, &[mesh], &[placement(0)]);

        let inst = arena.instances()[0];
        let prim = &arena.prim_indices()
            [inst.prim_base as usize..inst.prim_base as usize + bvh.prim_indices().len()];
        assert_eq!(prim, bvh.prim_indices());
        let idx = &arena.prim_indices()
            [inst.index_base as usize..inst.index_base as usize + indices.len()];
        assert_eq!(idx, indices.as_slice());
    }

    #[test]
    fn a_placement_naming_a_mesh_that_is_not_there_is_dropped() {
        let (positions, indices) = corpus::sphere(8, 4);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let mesh = ArenaMesh {
            bvh: &bvh,
            positions: &positions,
            indices: &indices,
            normals: None,
            uv0: None,
        };
        let boxes = [solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-1.0, -1.0, -1.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        }];
        let tlas = Bvh::build_tlas(&boxes);
        let arena = TraceArena::build(&tlas, &[mesh], &[placement(0), placement(7)]);
        assert_eq!(arena.instances().len(), 1);
    }

    #[test]
    fn a_mesh_without_normals_packs_a_zero_normal() {
        let (positions, indices) = corpus::sphere(8, 4);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let mesh = ArenaMesh {
            bvh: &bvh,
            positions: &positions,
            indices: &indices,
            normals: None,
            uv0: None,
        };
        let boxes = [solarxy_core::aabb::AABB {
            min: cgmath::Point3::new(-1.0, -1.0, -1.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        }];
        let tlas = Bvh::build_tlas(&boxes);
        let arena = TraceArena::build(&tlas, &[mesh], &[placement(0)]);
        // Compared by bits rather than by value: the sentinel the kernel tests
        // for is exactly zero, so that is what this has to assert.
        assert!(
            arena
                .vertex_attr()
                .iter()
                .all(|a| a.normal.iter().all(|v| v.to_bits() == 0))
        );
    }
}
