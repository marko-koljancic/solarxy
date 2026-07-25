//! Seeded avalanche hashing: the kernel's deterministic random source.
//!
//! The mix replicates the house avalanche hash the imaging crate uses for
//! its noise lattices (the same multiply constants and shift pattern),
//! kept kernel-local because the two crates deliberately do not depend on
//! each other. Every draw is a pure function of `(index, lane, seed)`, so
//! cooks are deterministic and order-independent: sample `i` draws the
//! same values no matter which samples were computed before it, which is
//! what lets a saved scene reproduce exactly.

/// A 64-bit avalanche hash of a sample index, a draw lane (which of the
/// sample's independent random values), and the user seed.
#[must_use]
pub fn hash(index: u64, lane: u32, seed: u32) -> u64 {
    let mut v = index.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ u64::from(lane).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ u64::from(seed).wrapping_mul(0x1656_67B1_9E37_79F9);
    v ^= v >> 33;
    v = v.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    v ^= v >> 33;
    v = v.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    v ^= v >> 33;
    v
}

/// A uniform `f32` in `[0, 1)`: the hash's top 24 bits, a float mantissa's
/// worth, so the distribution is uniform at full `f32` resolution.
#[must_use]
pub fn unit_f32(index: u64, lane: u32, seed: u32) -> f32 {
    (hash(index, lane, seed) >> 40) as f32 / 16_777_216.0
}

/// A uniform `f64` in `[0, 1)`: 53 mantissa bits, for draws that select
/// among millions of alternatives (the scatter triangle pick), where
/// `f32`'s 24 bits would quantize visibly.
#[must_use]
pub fn unit_f64(index: u64, lane: u32, seed: u32) -> f64 {
    (hash(index, lane, seed) >> 11) as f64 / 9_007_199_254_740_992.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_are_deterministic_and_lane_independent() {
        assert_eq!(hash(7, 1, 42), hash(7, 1, 42));
        assert_ne!(hash(7, 1, 42), hash(8, 1, 42), "index decorrelates");
        assert_ne!(hash(7, 1, 42), hash(7, 2, 42), "lane decorrelates");
        assert_ne!(hash(7, 1, 42), hash(7, 1, 43), "seed decorrelates");
    }

    #[test]
    fn unit_draws_stay_in_range_and_spread() {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for i in 0..1000 {
            let v = unit_f32(i, 0, 0);
            assert!((0.0..1.0).contains(&v), "{v} out of [0, 1)");
            min = min.min(v);
            max = max.max(v);
            let w = unit_f64(i, 0, 0);
            assert!((0.0..1.0).contains(&w), "{w} out of [0, 1)");
        }
        assert!(min < 0.05 && max > 0.95, "1000 draws span the range");
    }
}
