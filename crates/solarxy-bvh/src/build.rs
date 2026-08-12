//! The binned-SAH BVH2 builder.
//!
//! One builder serves both levels of the hierarchy. Triangles and instance
//! boxes differ only in how a `PrimRef` is derived from them, so the split
//! search, the partition and the node emission are written once and the
//! bottom level gets no better treatment than the top.

use solarxy_core::aabb::AABB;

use crate::bounds::Bounds;
use crate::node::BvhNode;

/// Primitives per leaf the builder aims for.
///
/// Five rather than one. Larger leaves cut the node count by roughly the same
/// factor, trading a little more primitive testing for a much smaller node
/// buffer, and the buffer is what the payload budget cares about.
pub const TARGET_LEAF_SIZE: u32 = 5;

/// The deepest node's depth index stays below this.
///
/// The traversal stack is a fixed 64 entries and a descent pushes at most one
/// entry per level, so 32 levels leaves the stack at half capacity in the
/// worst case. The builder does not assert this and then panic on pathological
/// input: it *enforces* it by emitting a leaf at the cap, because a hierarchy
/// that overflows a shader's stack is a wrong image and a crash-free builder
/// is worth more than a tight tree on input nothing produces.
pub const MAX_DEPTH: u32 = 32;

/// A leaf never holds more than this, even when the surface-area heuristic
/// says one big leaf is cheaper.
///
/// The heuristic is right about cost and wrong about variance: a 900-primitive
/// leaf makes one ray in the corner of the screen cost as much as the rest of
/// the frame. The cap only ever binds on degenerate input, since real geometry
/// never makes a leaf that large look cheap.
pub const MAX_LEAF_SIZE: u32 = 32;

/// Bins per axis in the split search.
const BIN_COUNT: usize = 12;

/// Cost of visiting an interior node, in units of one primitive test.
const TRAVERSAL_COST: f32 = 1.0;

/// One primitive as the builder sees it: where it is and how big it is.
#[derive(Clone, Copy)]
struct PrimRef {
    bounds: Bounds,
    centroid: [f32; 3],
}

impl PrimRef {
    fn from_bounds(bounds: Bounds) -> Self {
        Self {
            centroid: bounds.centre(),
            bounds,
        }
    }
}

/// What a build cost and what it produced.
///
/// These are the numbers a gate review reads, so they are part of the API
/// rather than a debug print: node count and maximum depth decide whether the
/// traversal stack is sized right, and `depth_capped_leaves` being non-zero is
/// the one signal that says the input defeated the heuristic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BvhStats {
    /// Primitives that made it into the hierarchy.
    pub prim_count: u32,
    /// Primitives rejected before the build, currently only triangles whose
    /// indices point outside the position buffer.
    pub skipped_prims: u32,
    pub node_count: u32,
    pub leaf_count: u32,
    /// Depth index of the deepest node. The root is zero.
    pub max_depth: u32,
    pub max_leaf_size: u32,
    /// Leaves the depth cap forced, which would otherwise have been split.
    /// Non-zero means the tree is worse than the heuristic wanted.
    pub depth_capped_leaves: u32,
}

/// A built hierarchy: the node array and the primitive permutation it indexes.
///
/// A leaf names a contiguous run of [`Bvh::prim_indices`], and each entry there
/// is an index into whatever the caller built from. For a triangle hierarchy
/// that is the triangle index, so vertex `k` of primitive `p` lives at
/// `indices[prim_indices[p] * 3 + k]`. For a top-level hierarchy it is the
/// instance index.
#[derive(Debug, Clone)]
pub struct Bvh {
    nodes: Vec<BvhNode>,
    prim_indices: Vec<u32>,
    stats: BvhStats,
}

/// The two buffers the traversal kernel binds, as bytes.
#[derive(Debug, Clone, Copy)]
pub struct GpuArrays<'a> {
    pub nodes: &'a [u8],
    pub prim_indices: &'a [u8],
}

impl Bvh {
    /// Build over triangles.
    ///
    /// `indices` is read as consecutive triples. A trailing partial triple and
    /// any triple pointing outside `positions` are dropped and counted in
    /// [`BvhStats::skipped_prims`] rather than panicking: this runs inside a
    /// Web Worker over whatever a file contained, and a malformed index buffer
    /// should cost the caller a triangle, not the session.
    #[must_use]
    pub fn build_triangles(positions: &[[f32; 3]], indices: &[u32]) -> Self {
        let tri_count = indices.len() / 3;
        let mut prims = Vec::with_capacity(tri_count);
        let mut source = Vec::with_capacity(tri_count);
        let mut skipped = 0u32;

        for (tri, corner) in indices.chunks_exact(3).enumerate() {
            let (Some(&a), Some(&b), Some(&c)) = (
                positions.get(corner[0] as usize),
                positions.get(corner[1] as usize),
                positions.get(corner[2] as usize),
            ) else {
                skipped += 1;
                continue;
            };
            let mut bounds = Bounds::from_point(a);
            bounds.expand(b);
            bounds.expand(c);
            prims.push(PrimRef::from_bounds(bounds));
            source.push(tri as u32);
        }

        let mut bvh = build(&prims);
        for slot in &mut bvh.prim_indices {
            // The builder permutes its own slots; map them back to the
            // triangle indices the caller knows, which differ whenever
            // anything was skipped.
            *slot = source[*slot as usize];
        }
        bvh.stats.skipped_prims = skipped;
        bvh
    }

    /// Build a top-level hierarchy over instance bounds, already in world
    /// space. Leaves name instance indices.
    #[must_use]
    pub fn build_tlas(instances: &[AABB]) -> Self {
        let prims: Vec<PrimRef> = instances
            .iter()
            .map(|a| PrimRef::from_bounds(Bounds::from(a)))
            .collect();
        build(&prims)
    }

    /// Reassembles a hierarchy from arrays a builder already produced.
    ///
    /// The one way to get a [`Bvh`] without building one, and it exists for the
    /// worker boundary: the build happens in a second wasm instance and the
    /// result crosses as bytes, so something on this side has to put the pieces
    /// back together. See [`crate::transfer`].
    ///
    /// Nothing is validated. A caller assembling arbitrary arrays gets an
    /// arbitrary hierarchy, which the traversal tolerates the way it tolerates
    /// a bad index: it may miss, it will not read out of bounds. Checking the
    /// tree's shape here would cost a walk on every transfer to defend against
    /// a caller that does not exist.
    #[must_use]
    pub fn from_parts(nodes: Vec<BvhNode>, prim_indices: Vec<u32>, stats: BvhStats) -> Self {
        Self {
            nodes,
            prim_indices,
            stats,
        }
    }

    #[must_use]
    pub fn nodes(&self) -> &[BvhNode] {
        &self.nodes
    }

    #[must_use]
    pub fn prim_indices(&self) -> &[u32] {
        &self.prim_indices
    }

    #[must_use]
    pub fn stats(&self) -> BvhStats {
        self.stats
    }

    /// Bounds of the root node, which covers everything in the hierarchy.
    #[must_use]
    pub fn root_bounds(&self) -> Bounds {
        self.nodes.first().map_or(Bounds::ZERO, BvhNode::bounds)
    }

    /// The node and permutation buffers as bytes, ready to upload.
    ///
    /// Zero copy: [`BvhNode`] is `repr(C)` and 32 bytes with no padding, which
    /// is what makes the GPU layout and the CPU layout the same bytes rather
    /// than a transcode.
    #[must_use]
    pub fn to_gpu_arrays(&self) -> GpuArrays<'_> {
        GpuArrays {
            nodes: bytemuck::cast_slice(&self.nodes),
            prim_indices: bytemuck::cast_slice(&self.prim_indices),
        }
    }
}

/// One task in the depth-first build.
struct Task {
    start: u32,
    count: u32,
    depth: u32,
    /// Node whose right-child index this task must fill in once it knows where
    /// it landed. `None` for the root and for every left child, which the
    /// left-child-is-next convention makes implicit.
    patch: Option<u32>,
}

/// The chosen split: which axis, and how many primitives went left.
struct Partition {
    axis: usize,
    left_count: u32,
}

fn build(prims: &[PrimRef]) -> Bvh {
    let mut nodes: Vec<BvhNode> = Vec::new();
    let mut order: Vec<u32> = (0..prims.len() as u32).collect();
    let mut stats = BvhStats {
        prim_count: prims.len() as u32,
        ..BvhStats::default()
    };

    if prims.is_empty() {
        // A hierarchy with a root is easier to consume than one without: the
        // kernel tests the root's bounds, misses, and is done. An empty node
        // array would need a guard at every call site instead.
        nodes.push(BvhNode::leaf(Bounds::ZERO, 0, 0));
        stats.node_count = 1;
        stats.leaf_count = 1;
        return Bvh {
            nodes,
            prim_indices: order,
            stats,
        };
    }

    // Two nodes per leaf is the shape of a full binary tree over
    // `prims / TARGET_LEAF_SIZE` leaves; over-reserving costs nothing next to
    // regrowing a million-node vector.
    nodes.reserve(2 * (prims.len() / TARGET_LEAF_SIZE as usize + 1));

    let mut stack = vec![Task {
        start: 0,
        count: prims.len() as u32,
        depth: 0,
        patch: None,
    }];

    while let Some(task) = stack.pop() {
        let self_idx = nodes.len() as u32;
        if let Some(parent) = task.patch {
            nodes[parent as usize].offset = self_idx;
        }

        let range = task.start as usize..(task.start + task.count) as usize;
        let mut node_bounds = Bounds::EMPTY;
        let mut centroid_bounds = Bounds::EMPTY;
        for &slot in &order[range.clone()] {
            node_bounds.union(&prims[slot as usize].bounds);
            centroid_bounds.expand(prims[slot as usize].centroid);
        }
        stats.max_depth = stats.max_depth.max(task.depth);

        let at_depth_cap = task.depth + 1 >= MAX_DEPTH;
        let partition = if task.count <= TARGET_LEAF_SIZE || at_depth_cap {
            None
        } else {
            choose_partition(
                prims,
                &mut order[range],
                &node_bounds,
                &centroid_bounds,
                task.count,
            )
        };

        let Some(p) = partition else {
            nodes.push(BvhNode::leaf(node_bounds, task.start, task.count));
            stats.leaf_count += 1;
            stats.max_leaf_size = stats.max_leaf_size.max(task.count);
            if at_depth_cap && task.count > TARGET_LEAF_SIZE {
                stats.depth_capped_leaves += 1;
            }
            continue;
        };

        nodes.push(BvhNode::interior(node_bounds, p.axis as u32));
        // Right first, so the left child pops next and lands at `self_idx + 1`
        // with the whole left subtree emitted before the right one starts.
        // That ordering is the left-child-is-next convention, and it is the
        // only thing keeping the shader from needing a second child index.
        stack.push(Task {
            start: task.start + p.left_count,
            count: task.count - p.left_count,
            depth: task.depth + 1,
            patch: Some(self_idx),
        });
        stack.push(Task {
            start: task.start,
            count: p.left_count,
            depth: task.depth + 1,
            patch: None,
        });
    }

    stats.node_count = nodes.len() as u32;
    Bvh {
        nodes,
        prim_indices: order,
        stats,
    }
}

/// Decide whether to split, and partition `order` in place if so.
///
/// Returns `None` when a leaf is the better answer. The order of the three
/// questions matters: the heuristic is consulted first, its verdict is
/// overridden only when the leaf would be oversized, and the median split is
/// the last resort for input the binning cannot separate at all.
fn choose_partition(
    prims: &[PrimRef],
    order: &mut [u32],
    node_bounds: &Bounds,
    centroid_bounds: &Bounds,
    count: u32,
) -> Option<Partition> {
    let parent_area = node_bounds.surface_area();
    let sah = if parent_area > 0.0 {
        find_split(prims, order, centroid_bounds, parent_area)
    } else {
        // Every primitive sits at one point, so no split can reduce area and
        // the heuristic has nothing to say.
        None
    };

    let oversized = count > MAX_LEAF_SIZE;
    let mut axis;
    let mut left_count;
    match sah {
        Some(split) if split.cost < count as f32 || oversized => {
            axis = split.axis;
            left_count = partition_by_bin(prims, order, split.axis, centroid_bounds, split.bin);
        }
        _ if oversized => {
            axis = centroid_bounds.largest_axis();
            left_count = 0;
        }
        _ => return None,
    }

    if left_count == 0 || left_count == count {
        // Binning put everything on one side, which happens when the
        // centroids cluster inside a single bin. An equal-count split down the
        // widest axis is worse than a good SAH split and strictly better than
        // not descending at all.
        axis = centroid_bounds.largest_axis();
        left_count = partition_by_median(prims, order, axis);
    }
    Some(Partition { axis, left_count })
}

/// The best binned split found, or `None` if no axis separates anything.
struct Split {
    axis: usize,
    /// Primitives in bins `0..=bin` go left.
    bin: usize,
    cost: f32,
}

fn find_split(
    prims: &[PrimRef],
    order: &[u32],
    centroid_bounds: &Bounds,
    parent_area: f32,
) -> Option<Split> {
    let extent = centroid_bounds.extent();
    let mut best: Option<Split> = None;

    for axis in 0..3 {
        if extent[axis] <= 0.0 {
            continue;
        }
        let scale = BIN_COUNT as f32 / extent[axis];

        let mut bin_bounds = [Bounds::EMPTY; BIN_COUNT];
        let mut bin_counts = [0u32; BIN_COUNT];
        for &slot in order {
            let prim = &prims[slot as usize];
            let bin = bin_of(prim.centroid[axis], centroid_bounds.min[axis], scale);
            bin_bounds[bin].union(&prim.bounds);
            bin_counts[bin] += 1;
        }

        // Prefix sweeps: `left[i]` describes bins `0..=i`, `right[i]`
        // describes bins `i..BIN_COUNT`. Computing both once turns the
        // eleven candidate splits into eleven additions.
        let mut left_area = [0.0f32; BIN_COUNT];
        let mut left_count = [0u32; BIN_COUNT];
        let mut acc = Bounds::EMPTY;
        let mut running = 0u32;
        for i in 0..BIN_COUNT {
            acc.union(&bin_bounds[i]);
            running += bin_counts[i];
            left_area[i] = acc.surface_area();
            left_count[i] = running;
        }

        let mut right_area = [0.0f32; BIN_COUNT];
        let mut right_count = [0u32; BIN_COUNT];
        acc = Bounds::EMPTY;
        running = 0;
        for i in (0..BIN_COUNT).rev() {
            acc.union(&bin_bounds[i]);
            running += bin_counts[i];
            right_area[i] = acc.surface_area();
            right_count[i] = running;
        }

        for i in 0..BIN_COUNT - 1 {
            if left_count[i] == 0 || right_count[i + 1] == 0 {
                continue;
            }
            let cost = TRAVERSAL_COST
                + (left_area[i].mul_add(
                    left_count[i] as f32,
                    right_area[i + 1] * right_count[i + 1] as f32,
                ) / parent_area);
            if best.as_ref().is_none_or(|b| cost < b.cost) {
                best = Some(Split { axis, bin: i, cost });
            }
        }
    }

    best
}

/// Which bin a centroid coordinate falls in.
///
/// Clamped at the top because a centroid exactly on the upper bound maps to
/// `BIN_COUNT` and there is no such bin.
fn bin_of(coord: f32, origin: f32, scale: f32) -> usize {
    let raw = ((coord - origin) * scale) as i32;
    (raw.max(0) as usize).min(BIN_COUNT - 1)
}

/// Move every primitive in bins `0..=split_bin` to the front. Returns how many
/// ended up there.
fn partition_by_bin(
    prims: &[PrimRef],
    order: &mut [u32],
    axis: usize,
    centroid_bounds: &Bounds,
    split_bin: usize,
) -> u32 {
    let extent = centroid_bounds.extent()[axis];
    if extent <= 0.0 {
        return 0;
    }
    let scale = BIN_COUNT as f32 / extent;
    let origin = centroid_bounds.min[axis];

    let mut left = 0usize;
    for i in 0..order.len() {
        let bin = bin_of(prims[order[i] as usize].centroid[axis], origin, scale);
        if bin <= split_bin {
            order.swap(left, i);
            left += 1;
        }
    }
    left as u32
}

/// Split into two equal halves by centroid rank along `axis`.
///
/// `select_nth_unstable_by` partitions in linear time without sorting, and the
/// return is a count rather than a value because a run of equal centroids
/// still has to divide somewhere.
fn partition_by_median(prims: &[PrimRef], order: &mut [u32], axis: usize) -> u32 {
    let mid = order.len() / 2;
    order.select_nth_unstable_by(mid, |a, b| {
        prims[*a as usize].centroid[axis].total_cmp(&prims[*b as usize].centroid[axis])
    });
    mid as u32
}

#[cfg(test)]
mod tests {
    use super::{Bvh, MAX_DEPTH, MAX_LEAF_SIZE, TARGET_LEAF_SIZE};
    use crate::corpus::{grid, sphere};

    #[test]
    fn an_empty_input_still_has_a_root() {
        let bvh = Bvh::build_triangles(&[], &[]);
        assert_eq!(bvh.nodes().len(), 1);
        assert!(bvh.nodes()[0].is_leaf());
        assert_eq!(bvh.nodes()[0].prim_count(), 0);
    }

    #[test]
    fn one_triangle_is_one_leaf() {
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let bvh = Bvh::build_triangles(&positions, &[0, 1, 2]);
        assert_eq!(bvh.nodes().len(), 1);
        assert_eq!(bvh.nodes()[0].prim_count(), 1);
        assert_eq!(bvh.prim_indices(), &[0]);
    }

    #[test]
    fn every_primitive_appears_exactly_once() {
        let (positions, indices) = grid(24);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let mut seen = bvh.prim_indices().to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), indices.len() / 3);
        assert_eq!(bvh.stats().prim_count, indices.len() as u32 / 3);
    }

    #[test]
    fn the_tree_is_well_formed() {
        let (positions, indices) = grid(24);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let nodes = bvh.nodes();

        let mut covered = 0u32;
        for (i, node) in nodes.iter().enumerate() {
            if node.is_leaf() {
                covered += node.prim_count();
                assert!(node.prim_count() <= MAX_LEAF_SIZE);
                continue;
            }
            // The left child is implicit and the right child is stored; both
            // must be real nodes ahead of their parent.
            assert!(i + 1 < nodes.len());
            assert!(node.right_child() as usize > i + 1);
            assert!((node.right_child() as usize) < nodes.len());
            assert!(node.axis() < 3);
        }
        assert_eq!(covered, bvh.stats().prim_count);
        assert_eq!(bvh.stats().node_count, nodes.len() as u32);
    }

    #[test]
    fn child_bounds_sit_inside_their_parent() {
        let (positions, indices) = grid(16);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let nodes = bvh.nodes();
        for (i, node) in nodes.iter().enumerate() {
            if node.is_leaf() {
                continue;
            }
            for child in [i as u32 + 1, node.right_child()] {
                let c = nodes[child as usize].bounds();
                let p = node.bounds();
                for axis in 0..3 {
                    assert!(c.min[axis] >= p.min[axis] - 1e-5);
                    assert!(c.max[axis] <= p.max[axis] + 1e-5);
                }
            }
        }
    }

    #[test]
    fn depth_stays_inside_the_traversal_stack() {
        let (positions, indices) = grid(64);
        let bvh = Bvh::build_triangles(&positions, &indices);
        assert!(bvh.stats().max_depth < MAX_DEPTH, "{:?}", bvh.stats());
        assert_eq!(bvh.stats().depth_capped_leaves, 0);
    }

    #[test]
    fn coincident_primitives_do_not_defeat_the_builder() {
        // Every triangle is the same triangle, so no split reduces area and
        // the centroid bounds are a point. The median fallback is the only
        // thing that keeps this from recursing forever or emitting one
        // enormous leaf.
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let indices: Vec<u32> = std::iter::repeat_n([0u32, 1, 2], 500).flatten().collect();
        let bvh = Bvh::build_triangles(&positions, &indices);
        assert_eq!(bvh.stats().prim_count, 500);
        assert!(bvh.stats().max_leaf_size <= MAX_LEAF_SIZE);
        assert!(bvh.stats().max_depth < MAX_DEPTH);
    }

    #[test]
    fn the_same_geometry_builds_the_same_tree_twice() {
        // Two things rest on this. A built hierarchy is cached across repacks
        // and reused rather than rebuilt, so a second build that disagreed
        // with the first would make a scene's rays depend on when the cache
        // happened to miss. And a seeded render promises the same image
        // twice, which it cannot keep if the tree underneath it moved.
        //
        // The coincident case is the one at risk: no split reduces area, so
        // the median fallback runs, and it selects by an unstable order.
        // Curved geometry with degenerate pole quads is the ordinary case
        // beside it.
        for (positions, indices) in [
            sphere(24, 16),
            (
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                std::iter::repeat_n([0u32, 1, 2], 500).flatten().collect(),
            ),
        ] {
            let first = Bvh::build_triangles(&positions, &indices);
            let second = Bvh::build_triangles(&positions, &indices);
            assert_eq!(first.nodes(), second.nodes());
            assert_eq!(first.prim_indices(), second.prim_indices());
        }
    }

    #[test]
    fn malformed_indices_cost_a_triangle_not_the_build() {
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        // One good triangle, one pointing past the end, one partial triple.
        let bvh = Bvh::build_triangles(&positions, &[0, 1, 2, 0, 1, 99, 0, 1]);
        assert_eq!(bvh.stats().prim_count, 1);
        assert_eq!(bvh.stats().skipped_prims, 1);
        assert_eq!(bvh.prim_indices(), &[0]);
    }

    #[test]
    fn leaves_hit_the_target_size_on_ordinary_geometry() {
        let (positions, indices) = grid(32);
        let bvh = Bvh::build_triangles(&positions, &indices);
        assert!(bvh.stats().max_leaf_size <= MAX_LEAF_SIZE);
        // A binned-SAH tree over a regular grid should be leaf-dominated at
        // roughly the target size, not one triangle per leaf.
        let avg = bvh.stats().prim_count as f32 / bvh.stats().leaf_count as f32;
        assert!(avg > 1.0, "average leaf size {avg}");
        assert!(avg <= TARGET_LEAF_SIZE as f32, "average leaf size {avg}");
    }

    #[test]
    fn the_top_level_hierarchy_indexes_instances() {
        use cgmath::Point3;
        use solarxy_core::aabb::AABB;

        let instances: Vec<AABB> = (0..40)
            .map(|i| {
                let x = i as f32 * 3.0;
                AABB {
                    min: Point3::new(x, 0.0, 0.0),
                    max: Point3::new(x + 1.0, 1.0, 1.0),
                }
            })
            .collect();
        let bvh = Bvh::build_tlas(&instances);
        let mut seen = bvh.prim_indices().to_vec();
        seen.sort_unstable();
        assert_eq!(seen, (0..40u32).collect::<Vec<_>>());
        let root = bvh.root_bounds();
        assert!((root.min[0] - 0.0).abs() < 1e-6);
        assert!((root.max[0] - 118.0).abs() < 1e-6);
    }

    #[test]
    fn the_gpu_arrays_are_the_node_and_permutation_buffers() {
        let (positions, indices) = grid(8);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let gpu = bvh.to_gpu_arrays();
        assert_eq!(gpu.nodes.len(), bvh.nodes().len() * 32);
        assert_eq!(gpu.prim_indices.len(), bvh.prim_indices().len() * 4);
    }
}
