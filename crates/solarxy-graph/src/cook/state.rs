//! Per-node cook state and statistics (the resumable budget loop that
//! consumes them lands in the engine cook driver).

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

/// Per-successful-cook statistics.
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

/// The lean, high-frequency badge state machine. `Ok` carries milliseconds
/// for the badge tooltip.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CookStatus {
    Pending,
    Cooking,
    Ok { ms: f64 },
    Error { message: String },
}

impl CookStatus {
    /// Whether two statuses describe the same badge state, ignoring the
    /// `Ok` timing (which jitters on every cook and must not force a status
    /// event on its own).
    ///
    /// This is [`NodeCookStats::same_shape`]'s discipline applied to the
    /// badge: without it, a node re-cooked every frame during playback
    /// emits a `CookStatus` event every frame purely because 0.41 ms and
    /// 0.43 ms are different numbers, re-rendering the whole canvas. The
    /// stored status still carries the fresh timing, so a pull query reads
    /// a current number; only the *event* is suppressed.
    #[must_use]
    pub fn same_state(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pending, Self::Pending)
            | (Self::Cooking, Self::Cooking)
            | (Self::Ok { .. }, Self::Ok { .. }) => true,
            (Self::Error { message: a }, Self::Error { message: b }) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CookStatus;

    #[test]
    fn ok_timing_alone_is_not_a_state_change() {
        // The playback case: a node re-cooked every frame reports a
        // different duration each time and must not emit for it.
        assert!(CookStatus::Ok { ms: 0.41 }.same_state(&CookStatus::Ok { ms: 0.43 }));
        assert!(CookStatus::Ok { ms: 0.0 }.same_state(&CookStatus::Ok { ms: 128.7 }));
    }

    #[test]
    fn crossing_variants_is_a_state_change() {
        let ok = CookStatus::Ok { ms: 1.0 };
        let err = CookStatus::Error {
            message: "boom".into(),
        };
        assert!(!ok.same_state(&CookStatus::Pending));
        assert!(!ok.same_state(&CookStatus::Cooking));
        assert!(!ok.same_state(&err));
        assert!(!err.same_state(&ok));
        assert!(!CookStatus::Pending.same_state(&CookStatus::Cooking));
    }

    #[test]
    fn a_new_error_message_is_a_state_change() {
        // Two failures in a row with different causes must both reach the
        // badge; only the timing is noise.
        let a = CookStatus::Error {
            message: "line 1: unknown attribute".into(),
        };
        let b = CookStatus::Error {
            message: "line 3: unknown attribute".into(),
        };
        assert!(!a.same_state(&b));
        assert!(a.same_state(&a.clone()));
    }
}
