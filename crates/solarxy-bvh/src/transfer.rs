//! A compact binary codec for moving a built hierarchy across a worker
//! boundary.
//!
//! The build is the expensive half of ingestion, and on web it has to happen in
//! the import worker, which is a second wasm instance with its own heap. So a
//! finished [`Bvh`] has to cross as bytes: [`pack`] writes one little-endian
//! blob, the blob transfers, and [`unpack`] reconstructs the hierarchy with one
//! memcpy per buffer.
//!
//! The shape follows `solarxy_kernel::transfer`, which already moves geometry
//! the same way, with one difference worth stating. That codec is versionless
//! because both sides are the same wasm build; this one carries a magic word
//! and a version anyway, because a hierarchy is the one thing here that could
//! plausibly be cached to storage later and read back by a different build. It
//! costs eight bytes against a payload measured in megabytes.
//!
//! **No error crate.** This crate depends on `solarxy-core` and `bytemuck` and
//! nothing else, which is what lets the import worker build one inside a
//! GPU-free wasm instance without dragging a tree behind it. A three-variant
//! error with a hand-written `Display` is a smaller price than widening that.

use bytemuck::{Zeroable, cast_slice, cast_slice_mut};

use crate::build::{Bvh, BvhStats};
use crate::node::BvhNode;

/// `SXBV`, so a blob that is not one fails on its first four bytes rather than
/// being read as a node count.
const MAGIC: u32 = 0x5358_4256;

/// The wire format's version. Bump it when the header or `BvhNode` changes
/// shape, never for a builder change: the same nodes from a better heuristic
/// are still these bytes.
const VERSION: u32 = 1;

/// Words before the node array: magic, version, the two counts, and the seven
/// `BvhStats` fields.
const HEADER_WORDS: usize = 4 + 7;
const HEADER_BYTES: usize = HEADER_WORDS * 4;

/// A malformed or truncated hierarchy blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferError {
    /// The first four bytes are not the format's magic word.
    NotAHierarchy,
    /// The blob was written by a different wire format.
    Version(u32),
    /// The blob is shorter than its own header says it should be.
    Truncated { wanted: usize, got: usize },
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAHierarchy => write!(f, "not a hierarchy blob"),
            Self::Version(v) => write!(f, "hierarchy blob version {v} is not {VERSION}"),
            Self::Truncated { wanted, got } => {
                write!(
                    f,
                    "hierarchy blob truncated: wanted {wanted} bytes, got {got}"
                )
            }
        }
    }
}

impl std::error::Error for TransferError {}

/// Serializes a hierarchy into one transferable blob.
///
/// The node array is copied rather than viewed, because the destination is a
/// `Vec<u8>` whose alignment nobody controls; `BvhNode` needs four-byte
/// alignment and the header is a whole number of words, so the offsets are
/// sound and only the base pointer is in question.
#[must_use]
pub fn pack(bvh: &Bvh) -> Vec<u8> {
    let stats = bvh.stats();
    let nodes = bvh.nodes();
    let prims = bvh.prim_indices();

    let mut out = Vec::with_capacity(HEADER_BYTES + nodes.len() * 32 + prims.len() * 4);
    for word in [
        MAGIC,
        VERSION,
        u32::try_from(nodes.len()).unwrap_or(u32::MAX),
        u32::try_from(prims.len()).unwrap_or(u32::MAX),
        stats.prim_count,
        stats.skipped_prims,
        stats.node_count,
        stats.leaf_count,
        stats.max_depth,
        stats.max_leaf_size,
        stats.depth_capped_leaves,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(cast_slice(nodes));
    out.extend_from_slice(cast_slice(prims));
    out
}

/// Reconstructs a hierarchy from a blob [`pack`] wrote.
///
/// Every length is checked against what is actually there before anything is
/// read, because this runs over bytes that crossed a boundary and a truncated
/// transfer is a plausible event rather than a logic error.
pub fn unpack(bytes: &[u8]) -> Result<Bvh, TransferError> {
    let header = read_words(bytes, 0, HEADER_WORDS)?;
    if header[0] != MAGIC {
        return Err(TransferError::NotAHierarchy);
    }
    if header[1] != VERSION {
        return Err(TransferError::Version(header[1]));
    }
    let node_count = header[2] as usize;
    let prim_count = header[3] as usize;
    let stats = BvhStats {
        prim_count: header[4],
        skipped_prims: header[5],
        node_count: header[6],
        leaf_count: header[7],
        max_depth: header[8],
        max_leaf_size: header[9],
        depth_capped_leaves: header[10],
    };

    let node_bytes = node_count * std::mem::size_of::<BvhNode>();
    let prim_bytes = prim_count * 4;
    let wanted = HEADER_BYTES + node_bytes + prim_bytes;
    if bytes.len() < wanted {
        return Err(TransferError::Truncated {
            wanted,
            got: bytes.len(),
        });
    }

    // Copy into aligned destinations rather than casting the input in place:
    // a `&[u8]` that arrived from anywhere carries no alignment guarantee, and
    // `cast_slice` over a misaligned base is a panic rather than a slow path.
    let mut nodes = vec![BvhNode::zeroed(); node_count];
    cast_slice_mut::<BvhNode, u8>(&mut nodes)
        .copy_from_slice(&bytes[HEADER_BYTES..HEADER_BYTES + node_bytes]);
    let mut prim_indices = vec![0u32; prim_count];
    cast_slice_mut::<u32, u8>(&mut prim_indices)
        .copy_from_slice(&bytes[HEADER_BYTES + node_bytes..HEADER_BYTES + node_bytes + prim_bytes]);

    Ok(Bvh::from_parts(nodes, prim_indices, stats))
}

fn read_words(bytes: &[u8], at: usize, count: usize) -> Result<Vec<u32>, TransferError> {
    let wanted = at + count * 4;
    if bytes.len() < wanted {
        return Err(TransferError::Truncated {
            wanted,
            got: bytes.len(),
        });
    }
    Ok((0..count)
        .map(|i| {
            let o = at + i * 4;
            u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{MAGIC, TransferError, VERSION, pack, unpack};
    use crate::build::Bvh;
    use crate::corpus;

    fn built() -> Bvh {
        let (positions, indices) = corpus::sphere(16, 12);
        Bvh::build_triangles(&positions, &indices)
    }

    #[test]
    fn a_hierarchy_survives_the_round_trip_byte_for_byte() {
        // Byte-identical rather than merely equivalent, because the cache the
        // two-level structure exists for depends on a packed hierarchy being
        // exactly what its builder emitted.
        let original = built();
        let back = unpack(&pack(&original)).expect("round trip");
        assert_eq!(back.nodes(), original.nodes());
        assert_eq!(back.prim_indices(), original.prim_indices());
        assert_eq!(back.stats(), original.stats());
    }

    #[test]
    fn a_round_tripped_hierarchy_answers_the_same_queries() {
        // The arrays matching is necessary and not sufficient: the stats carry
        // the primitive count, and a traversal reading a wrong one finds
        // nothing while every array comparison passes.
        let (positions, indices) = corpus::sphere(16, 12);
        let original = Bvh::build_triangles(&positions, &indices);
        let back = unpack(&pack(&original)).expect("round trip");
        let mut hits = 0;
        for ray in corpus::rays(7, 64) {
            let a =
                original.intersect_triangles(ray.origin, ray.direction, 1e30, &positions, &indices);
            let b = back.intersect_triangles(ray.origin, ray.direction, 1e30, &positions, &indices);
            assert_eq!(a.is_some(), b.is_some(), "ray {}", ray.index);
            if let (Some(a), Some(b)) = (a, b) {
                assert_eq!(a.prim, b.prim);
                assert_eq!(a.t.to_bits(), b.t.to_bits());
                hits += 1;
            }
        }
        assert!(hits > 0, "the corpus missed everything and proved nothing");
    }

    #[test]
    fn an_empty_hierarchy_round_trips() {
        // A scene with nothing in it is a state reached in the ordinary course
        // of editing, and the builder still emits a root.
        let empty = Bvh::build_triangles(&[], &[]);
        let back = unpack(&pack(&empty)).expect("round trip");
        assert_eq!(back.nodes(), empty.nodes());
        assert_eq!(back.prim_indices(), empty.prim_indices());
    }

    #[test]
    fn something_that_is_not_a_hierarchy_is_refused_on_its_first_word() {
        let mut blob = pack(&built());
        blob[0] ^= 0xFF;
        assert_eq!(unpack(&blob).unwrap_err(), TransferError::NotAHierarchy);
    }

    #[test]
    fn a_blob_from_another_wire_format_is_refused_rather_than_read() {
        let mut blob = pack(&built());
        blob[4..8].copy_from_slice(&(VERSION + 1).to_le_bytes());
        assert_eq!(
            unpack(&blob).unwrap_err(),
            TransferError::Version(VERSION + 1)
        );
    }

    #[test]
    fn a_truncated_blob_reports_what_it_wanted() {
        let blob = pack(&built());
        let cut = blob.len() - 16;
        match unpack(&blob[..cut]) {
            Err(TransferError::Truncated { wanted, got }) => {
                assert_eq!(got, cut);
                assert_eq!(wanted, blob.len());
            }
            other => panic!("expected a truncation, got {other:?}"),
        }
    }

    #[test]
    fn a_blob_shorter_than_its_own_header_is_refused() {
        assert!(matches!(
            unpack(&MAGIC.to_le_bytes()),
            Err(TransferError::Truncated { .. })
        ));
        assert!(matches!(unpack(&[]), Err(TransferError::Truncated { .. })));
    }
}
