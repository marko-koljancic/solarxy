//! Shared numeric-conversion primitives. Both coercion systems call these:
//! the wire matrix (`crate::registry::coerce`, gather-time) and the param
//! resolver's JSON-numeric coercion (`crate::registry::resolve`,
//! load/SetParam-time). One definition means one rounding model the user
//! ever observes; the two systems stay separate policies over shared
//! arithmetic.

/// Float to Int: rounds half away from zero (the frozen matrix constant),
/// saturating at the `i64` range.
#[must_use]
pub fn f64_to_i64(v: f64) -> i64 {
    v.round() as i64
}

/// Scalar splat into a fixed-size vector.
#[must_use]
pub fn splat<const N: usize>(v: f64) -> [f64; N] {
    [v; N]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn rounds_half_away_from_zero() {
        assert_eq!(f64_to_i64(0.5), 1);
        assert_eq!(f64_to_i64(-0.5), -1);
        assert_eq!(f64_to_i64(1.4), 1);
        assert_eq!(f64_to_i64(1.5), 2);
        assert_eq!(f64_to_i64(-2.5), -3);
        // Saturating, not wrapping.
        assert_eq!(f64_to_i64(f64::MAX), i64::MAX);
        assert_eq!(f64_to_i64(f64::MIN), i64::MIN);
        assert_eq!(f64_to_i64(f64::NAN), 0);
    }

    #[test]
    fn splat_fills_every_lane() {
        assert_eq!(splat::<2>(1.5), [1.5, 1.5]);
        assert_eq!(splat::<3>(-2.0), [-2.0, -2.0, -2.0]);
        assert_eq!(splat::<4>(0.0), [0.0; 4]);
    }
}
