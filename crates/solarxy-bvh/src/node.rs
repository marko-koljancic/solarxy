//! The 32-byte node the GPU reads.

use crate::bounds::Bounds;

/// High bit of [`BvhNode::meta`]: set on a leaf, clear on an interior node.
pub const LEAF_FLAG: u32 = 1 << 31;

/// One node of a BVH2, laid out for the traversal kernel.
///
/// Two packing conventions carry the tree's topology in the 8 bytes that are
/// not bounds, and both are load-bearing for the shader:
///
/// - **The left child is always `self_index + 1`.** The builder emits nodes in
///   depth-first order and finishes a node's left subtree before starting its
///   right one, so the left child needs no stored index. `offset` therefore
///   holds the *right* child on an interior node, and the first primitive on a
///   leaf.
/// - **The high bit of `meta` marks a leaf.** Its low 31 bits are the
///   primitive count on a leaf and the split axis on an interior node. The
///   axis is what lets traversal descend the near child first without
///   recomputing which side the ray came from.
///
/// The layout mirrors the implied node structure of the traversal this crate's
/// WGSL twin ports from, so the shader indexes it without a transcode step.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BvhNode {
    /// Lower corner of the node's bounds.
    pub min: [f32; 3],
    /// Leaf: index of its first primitive in the permutation. Interior: index
    /// of its right child. The left child is `self_index + 1`.
    pub offset: u32,
    /// Upper corner of the node's bounds.
    pub max: [f32; 3],
    /// [`LEAF_FLAG`] in the high bit; primitive count (leaf) or split axis
    /// (interior) in the low 31.
    pub meta: u32,
}

const _: () = assert!(std::mem::size_of::<BvhNode>() == 32);
const _: () = assert!(std::mem::align_of::<BvhNode>() == 4);

impl BvhNode {
    /// A leaf covering `count` primitives starting at `first_prim` in the
    /// permutation.
    #[must_use]
    pub fn leaf(bounds: Bounds, first_prim: u32, count: u32) -> Self {
        debug_assert!(count < LEAF_FLAG, "leaf primitive count overflows 31 bits");
        Self {
            min: bounds.min,
            offset: first_prim,
            max: bounds.max,
            meta: LEAF_FLAG | count,
        }
    }

    /// An interior node split along `axis`, with its right child index left at
    /// zero for the builder to patch once that child is allocated.
    #[must_use]
    pub fn interior(bounds: Bounds, axis: u32) -> Self {
        debug_assert!(axis < 3, "split axis must be 0, 1 or 2");
        Self {
            min: bounds.min,
            offset: 0,
            max: bounds.max,
            meta: axis,
        }
    }

    #[must_use]
    pub fn is_leaf(&self) -> bool {
        self.meta & LEAF_FLAG != 0
    }

    /// Number of primitives, on a leaf. Zero on an interior node.
    #[must_use]
    pub fn prim_count(&self) -> u32 {
        if self.is_leaf() {
            self.meta & !LEAF_FLAG
        } else {
            0
        }
    }

    /// Index of the first primitive in the permutation, on a leaf.
    #[must_use]
    pub fn first_prim(&self) -> u32 {
        self.offset
    }

    /// Split axis, on an interior node.
    #[must_use]
    pub fn axis(&self) -> u32 {
        self.meta & !LEAF_FLAG
    }

    /// Index of the right child, on an interior node.
    #[must_use]
    pub fn right_child(&self) -> u32 {
        self.offset
    }

    #[must_use]
    pub fn bounds(&self) -> Bounds {
        Bounds {
            min: self.min,
            max: self.max,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BvhNode, LEAF_FLAG};
    use crate::bounds::Bounds;

    #[test]
    fn leaf_round_trips_its_count() {
        let n = BvhNode::leaf(Bounds::ZERO, 7, 5);
        assert!(n.is_leaf());
        assert_eq!(n.first_prim(), 7);
        assert_eq!(n.prim_count(), 5);
    }

    #[test]
    fn interior_round_trips_its_axis_and_child() {
        let mut n = BvhNode::interior(Bounds::ZERO, 2);
        n.offset = 41;
        assert!(!n.is_leaf());
        assert_eq!(n.axis(), 2);
        assert_eq!(n.right_child(), 41);
        assert_eq!(n.prim_count(), 0);
    }

    #[test]
    fn the_leaf_flag_is_the_only_thing_separating_the_two() {
        let leaf = BvhNode::leaf(Bounds::ZERO, 0, 2);
        let interior = BvhNode::interior(Bounds::ZERO, 2);
        assert_eq!(leaf.meta & !LEAF_FLAG, interior.meta);
    }

    #[test]
    fn the_gpu_view_is_a_plain_byte_reinterpretation() {
        let nodes = [
            BvhNode::leaf(Bounds::ZERO, 1, 2),
            BvhNode::interior(Bounds::ZERO, 0),
        ];
        let bytes: &[u8] = bytemuck::cast_slice(&nodes);
        assert_eq!(bytes.len(), 64);
    }
}
