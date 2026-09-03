//! How close two images are.
//!
//! One definition, because there were two and they answered different
//! questions. The golden harness counted pixels whose worst channel exceeded a
//! tolerance, which is what a regression gate wants: any pixel that moved is a
//! pixel to look at. The still tests took a mean over the colour lanes, which
//! is what a comparison across two renderers wants: floating-point summation is
//! not associative, so a few least-significant bits are expected and a count of
//! them says nothing.
//!
//! Both readings come out of one pass here, so a caller picks the one its
//! question needs rather than picking an implementation.

/// How two images of the same size differ.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ImageDifference {
    /// Mean absolute difference over the colour lanes, in levels of 255.
    ///
    /// Alpha is excluded deliberately: a still is opaque, so the lane is a
    /// constant and averaging it in would dilute every real difference by a
    /// quarter.
    pub mean_abs: f64,
    /// The worst single channel difference anywhere, alpha included, because a
    /// gate that ignored alpha would miss an image that became transparent.
    pub max_channel: u8,
    /// How many pixels have any channel differing by more than the tolerance.
    pub differing: usize,
    pub total: usize,
}

impl ImageDifference {
    /// Whether every pixel is inside the tolerance the comparison was made at.
    #[must_use]
    pub fn within_tolerance(&self) -> bool {
        self.differing == 0
    }
}

/// Compares two RGBA8 images of the same dimensions.
///
/// `tolerance` is the per-channel difference a pixel is allowed before it is
/// counted as differing; it does not affect the mean or the maximum, which are
/// measurements rather than judgements.
///
/// Compares only as far as the shorter of the two, so a caller that has already
/// checked the dimensions gets an answer and one that has not gets a partial
/// one rather than a panic. Checking them is the caller's job because what to
/// say about a mismatch differs: a gate names the file, a test names the shell.
#[must_use]
pub fn compare_rgba8(a: &[u8], b: &[u8], tolerance: u8) -> ImageDifference {
    let mut total_abs = 0u64;
    let mut lanes = 0u64;
    let mut max_channel = 0u8;
    let mut differing = 0usize;
    let mut total = 0usize;

    for (p, q) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        total += 1;
        let mut pixel_differs = false;
        for c in 0..4 {
            let d = p[c].abs_diff(q[c]);
            max_channel = max_channel.max(d);
            if d > tolerance {
                pixel_differs = true;
            }
            if c < 3 {
                total_abs += u64::from(d);
                lanes += 1;
            }
        }
        if pixel_differs {
            differing += 1;
        }
    }

    ImageDifference {
        #[allow(clippy::cast_precision_loss)]
        mean_abs: total_abs as f64 / lanes.max(1) as f64,
        max_channel,
        differing,
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_differ_by_nothing() {
        let a = vec![7u8; 64];
        let d = compare_rgba8(&a, &a, 0);
        // Bit-exact zero rather than nearly zero: a sum of no differences over a
        // count is exact, and a tolerance here would pass an image that moved.
        assert_eq!(d.mean_abs.to_bits(), 0.0f64.to_bits());
        assert_eq!(d.max_channel, 0);
        assert_eq!(d.differing, 0);
        assert_eq!(d.total, 16);
        assert!(d.within_tolerance());
    }

    /// The mean is over the colour lanes and the maximum is over all four, and
    /// the difference between them is the point: an image that went
    /// transparent has moved, and an image whose opaque alpha never changes
    /// should not have every real difference diluted by a quarter.
    #[test]
    fn alpha_counts_toward_the_maximum_and_not_the_mean() {
        let a = [10u8, 10, 10, 255];
        let b = [10u8, 10, 10, 0];
        let d = compare_rgba8(&a, &b, 0);
        assert_eq!(
            d.mean_abs.to_bits(),
            0.0f64.to_bits(),
            "alpha reached the colour mean"
        );
        assert_eq!(d.max_channel, 255);
        assert_eq!(d.differing, 1);
    }

    #[test]
    fn the_tolerance_decides_only_the_count() {
        let a = [10u8, 10, 10, 255];
        let b = [12u8, 10, 10, 255];
        let loose = compare_rgba8(&a, &b, 2);
        assert_eq!(loose.differing, 0, "a difference inside tolerance counted");
        assert_eq!(loose.max_channel, 2, "the tolerance moved the measurement");
        assert!((loose.mean_abs - 2.0 / 3.0).abs() < 1e-9);

        let strict = compare_rgba8(&a, &b, 1);
        assert_eq!(strict.differing, 1);
        assert_eq!(strict.max_channel, loose.max_channel);
        assert!((strict.mean_abs - loose.mean_abs).abs() < 1e-9);
    }

    #[test]
    fn a_short_side_stops_the_comparison_rather_than_panicking() {
        let a = vec![0u8; 16];
        let b = vec![0u8; 8];
        assert_eq!(compare_rgba8(&a, &b, 0).total, 2);
    }
}
