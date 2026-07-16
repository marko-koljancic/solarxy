//! Per-node cook state and statistics (the resumable budget loop that
//! consumes them lands in the engine cook driver, task G4).

use solarxy_core::AABB;

/// One node's cook lifecycle. The cook set is modeled as this persistent
/// per-node state plus the memoized topological order, never a consumable
/// queue with a cursor: re-dirtying an already-passed node simply re-marks
/// it `Dirty` for the next budget slice, so resume is correct by
/// construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CookState {
    /// Committed and current.
    Clean,
    /// Needs a (re)cook on the next pass. A freshly added node starts
    /// here, dirty until first cooked.
    #[default]
    Dirty,
    /// Cooking asynchronously; carries the generation token the result
    /// must match. Never re-cooked by the sync loop (that would re-spawn a
    /// duplicate job); resurrected by `submit_job_result`.
    Pending(u64),
}

/// Per-successful-cook statistics (node catalog part I, section 9).
/// Delivered as coalesced `NodeStats` events, emitted only for nodes whose
/// stats changed. `AABB` is not `PartialEq`, so change detection compares
/// via [`NodeCookStats::same_shape`] (bit-exact bounds), not `derive`.
#[derive(Debug, Clone, Copy)]
pub struct NodeCookStats {
    pub duration_us: u64,
    /// Vertex count over the output geometry set.
    pub points: u64,
    /// Triangle count.
    pub prims: u64,
    pub meshes: u32,
    pub bounds: Option<AABB>,
    /// `(width, height)` of the default image output, for nodes whose
    /// default output is an image rather than geometry (the geometry
    /// fields stay zero for those).
    pub image: Option<(u32, u32)>,
}

impl NodeCookStats {
    /// Whether two stats describe the same output shape (ignoring
    /// `duration_us`, which changes every cook and must not force a stats
    /// event on its own). Bounds compare bit-exact.
    #[must_use]
    pub fn same_shape(&self, other: &Self) -> bool {
        self.points == other.points
            && self.prims == other.prims
            && self.meshes == other.meshes
            && self.image == other.image
            && bounds_eq(self.bounds, other.bounds)
    }
}

fn bounds_eq(a: Option<AABB>, b: Option<AABB>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.min.x.to_bits() == b.min.x.to_bits()
                && a.min.y.to_bits() == b.min.y.to_bits()
                && a.min.z.to_bits() == b.min.z.to_bits()
                && a.max.x.to_bits() == b.max.x.to_bits()
                && a.max.y.to_bits() == b.max.y.to_bits()
                && a.max.z.to_bits() == b.max.z.to_bits()
        }
        _ => false,
    }
}

/// The lean, high-frequency badge state machine (node catalog part I,
/// section 9). `Ok` carries milliseconds for the badge tooltip.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CookStatus {
    Pending,
    Cooking,
    Ok { ms: f64 },
    Error { message: String },
}
