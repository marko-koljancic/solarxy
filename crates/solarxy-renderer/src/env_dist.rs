//! The environment's sampling distribution: which way to look for light.
//!
//! GPU-free, and deliberately so. It is built on the CPU in the same worker job
//! that already prepares an HDRI for image-based lighting, rides the same
//! transfer, and is uploaded as two lookup textures the kernel searches.
//!
//! # Why an environment needs one at all
//!
//! An outdoor HDRI is mostly sky and a little sun, and the sun is a thousand
//! times brighter than what surrounds it while occupying a ten-thousandth of the
//! image. A path tracer that draws directions uniformly finds it about once in
//! ten thousand tries, and the tries that do find it come back carrying ten
//! thousand times the average. That is not slow convergence, it is an image made
//! of white specks that never settles. Drawing directions in proportion to how
//! bright the environment is there finds the sun on nearly every sample and
//! charges it a correspondingly high density, so the two cancel and the estimate
//! is quiet.
//!
//! # The one line the source material gets wrong
//!
//! An equirectangular image is not an equal-area projection: a row near the pole
//! covers a sliver of sky and a row at the equator covers a band, and the pixel
//! count is the same. Weighting by luminance alone therefore treats a bright
//! patch of pole as if it were as large as an equally bright patch of horizon,
//! and oversamples it by the ratio of their solid angles, which at the top row of
//! a 1024-tall image is about three hundred to one.
//!
//! The source records this as a known bug of its own and does not fix it. Here
//! each weight carries the `sin(theta)` its row subtends, which is one
//! multiplication at build time and the difference between a sky that converges
//! and one that stipples at the poles.
//!
//! # Why the tables are cumulative rather than inverted
//!
//! The obvious representation is the *inverse* function: a table that takes a
//! uniform number straight to a row, read with hardware interpolation in one tap.
//! That is what the source does, and it is an approximation, because linearly
//! interpolating an inverse produces a sampler whose real density is not the
//! piecewise-constant one the shader then reports. This stage exists to establish
//! that densities describe their samplers, so introducing a new place where one
//! does not would be an odd way to spend it.
//!
//! These are the cumulative distributions themselves, and the kernel binary
//! searches them. That is exact: the direction drawn is distributed exactly as
//! the density claims, and the cost is about twenty texture reads against a
//! hierarchy traversal that costs far more.

/// A two-dimensional piecewise-constant distribution over an equirectangular
/// image, weighted by luminance and by the solid angle each pixel subtends.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvDistribution {
    width: u32,
    height: u32,
    /// Cumulative row weights, normalized so the last entry is exactly one.
    /// `height` entries.
    marginal: Vec<f32>,
    /// Per row, the cumulative pixel weights within that row, each row
    /// normalized so its last entry is exactly one. `width * height` entries,
    /// row-major.
    conditional: Vec<f32>,
    /// The sum of every weight before normalization, which is the denominator
    /// the density needs. Zero means the image is black.
    total_weight: f32,
}

impl EnvDistribution {
    /// Builds the distribution over a sanitized linear-RGB equirectangular
    /// image, three floats per pixel, row-major.
    ///
    /// A black image, or one whose dimensions do not match the pixel count,
    /// yields an empty distribution whose `total_weight` is zero; a caller reads
    /// that as "there is nothing here worth sampling" rather than as an error,
    /// because a scene with an unlit environment is an ordinary scene.
    #[must_use]
    pub fn build(width: u32, height: u32, pixels: &[f32]) -> Self {
        let (w, h) = (width as usize, height as usize);
        let expected = w.saturating_mul(h).saturating_mul(3);
        if w == 0 || h == 0 || pixels.len() < expected {
            return Self::empty();
        }

        let mut conditional = vec![0.0f32; w * h];
        let mut marginal = vec![0.0f32; h];
        let mut row_sums = vec![0.0f64; h];
        let mut total = 0.0f64;

        for y in 0..h {
            // The row's own solid-angle factor, taken at the row centre. `v`
            // runs from the top of the image to the bottom and `theta` with it,
            // matching `sample_equirect`: `v = acos(dir.y) / PI`.
            #[allow(clippy::cast_precision_loss)]
            let theta = (y as f64 + 0.5) / h as f64 * std::f64::consts::PI;
            let solid_angle = theta.sin();

            let mut running = 0.0f64;
            for x in 0..w {
                let i = (y * w + x) * 3;
                let weight = luminance(pixels[i], pixels[i + 1], pixels[i + 2]) * solid_angle;
                running += weight;
                // The cumulative sum lands here unnormalized and is divided
                // through below, once the row's total is known.
                #[allow(clippy::cast_possible_truncation)]
                {
                    conditional[y * w + x] = running as f32;
                }
            }
            row_sums[y] = running;
            total += running;
        }

        if total <= 0.0 {
            return Self::empty();
        }

        let mut running = 0.0f64;
        for y in 0..h {
            let row = row_sums[y];
            if row > 0.0 {
                for x in 0..w {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        conditional[y * w + x] =
                            (f64::from(conditional[y * w + x]) / row).min(1.0) as f32;
                    }
                }
                // Exactly one at the end of the row, so a variate arbitrarily
                // close to one cannot search past the last column and fall off
                // the table.
                conditional[y * w + w - 1] = 1.0;
            } else {
                // An all-black row is never selected by the marginal, because
                // its own weight is zero, but its conditional still has to be a
                // valid non-decreasing table reaching one: a variate that lands
                // here through a rounding error must find a column rather than
                // search forever.
                #[allow(clippy::cast_precision_loss)]
                for x in 0..w {
                    conditional[y * w + x] = (x as f32 + 1.0) / w as f32;
                }
            }
            running += row;
            #[allow(clippy::cast_possible_truncation)]
            {
                marginal[y] = (running / total).min(1.0) as f32;
            }
        }
        marginal[h - 1] = 1.0;

        #[allow(clippy::cast_possible_truncation)]
        Self {
            width,
            height,
            marginal,
            conditional,
            total_weight: total as f32,
        }
    }

    /// Reassembles a distribution that crossed the worker boundary.
    ///
    /// The tables are trusted rather than re-validated, because the only
    /// producer is [`EnvDistribution::build`] on the other side of one transfer
    /// this codebase owns both ends of. What is *not* trusted is their length:
    /// a short table would make the kernel's binary search read past the
    /// texture it was uploaded into, so a blob whose dimensions and data
    /// disagree collapses to an empty distribution and the environment falls
    /// back to its constant.
    #[must_use]
    pub fn from_parts(
        width: u32,
        height: u32,
        marginal: Vec<f32>,
        conditional: Vec<f32>,
        total_weight: f32,
    ) -> Self {
        let cells = (width as usize).saturating_mul(height as usize);
        if width == 0
            || height == 0
            || total_weight <= 0.0
            || marginal.len() != height as usize
            || conditional.len() != cells
        {
            return Self::empty();
        }
        Self {
            width,
            height,
            marginal,
            conditional,
            total_weight,
        }
    }

    /// The distribution of an environment there is nothing to sample in.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            marginal: Vec::new(),
            conditional: Vec::new(),
            total_weight: 0.0,
        }
    }

    /// Whether there is anything here worth drawing a direction toward.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total_weight <= 0.0 || self.marginal.is_empty()
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Cumulative row weights, `height` entries, ending at exactly one.
    #[must_use]
    pub fn marginal(&self) -> &[f32] {
        &self.marginal
    }

    /// Per-row cumulative pixel weights, row-major, each row ending at exactly
    /// one.
    #[must_use]
    pub fn conditional(&self) -> &[f32] {
        &self.conditional
    }

    /// The sum of every weight, which is the density's denominator.
    #[must_use]
    pub fn total_weight(&self) -> f32 {
        self.total_weight
    }

    /// The solid-angle density this distribution assigns to a point in the
    /// image, matching what the kernel computes.
    ///
    /// Here for the tests rather than for the renderer, and that is the point:
    /// a density written twice, once in Rust over the pixels and once in WGSL
    /// over the textures, is two independent statements of one claim.
    ///
    /// `u` and `v` are in `[0, 1)`, `v` measured down from the top.
    #[must_use]
    pub fn pdf(&self, u: f32, v: f32, pixels: &[f32]) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let x = ((u.clamp(0.0, 1.0) * self.width as f32) as usize).min(w - 1);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let y = ((v.clamp(0.0, 1.0) * self.height as f32) as usize).min(h - 1);
        let i = (y * w + x) * 3;
        #[allow(clippy::cast_precision_loss)]
        let theta_cell = (y as f64 + 0.5) / h as f64 * std::f64::consts::PI;
        let theta = f64::from(v.clamp(0.0, 1.0)) * std::f64::consts::PI;
        let sin_theta = theta.sin();
        if sin_theta <= 0.0 {
            return 0.0;
        }
        let weight = luminance(pixels[i], pixels[i + 1], pixels[i + 2]) * theta_cell.sin();
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        {
            (weight * (w * h) as f64
                / (f64::from(self.total_weight) * 2.0 * std::f64::consts::PI.powi(2) * sin_theta))
                as f32
        }
    }
}

/// Rec. 709 luminance, the same weighting the kernel's `luminance` uses.
fn luminance(r: f32, g: f32, b: f32) -> f64 {
    0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact zeros, and a CDF normalized to end at exactly 1.0

    use super::*;

    /// A grey image, where every pixel has the same luminance and the only
    /// thing distinguishing rows is the solid angle they subtend.
    fn flat(width: u32, height: u32, value: f32) -> Vec<f32> {
        vec![value; (width * height * 3) as usize]
    }

    #[test]
    fn a_black_image_has_nothing_to_sample() {
        let dist = EnvDistribution::build(8, 4, &flat(8, 4, 0.0));
        assert!(dist.is_empty());
        assert_eq!(dist.total_weight(), 0.0);
    }

    #[test]
    fn a_malformed_image_is_empty_rather_than_a_panic() {
        // A short pixel buffer is not a state any producer here reaches, and
        // indexing past it would be a crash inside the lighting path.
        assert!(EnvDistribution::build(8, 4, &[0.0; 12]).is_empty());
        assert!(EnvDistribution::build(0, 0, &[]).is_empty());
    }

    #[test]
    fn every_table_is_non_decreasing_and_ends_at_one() {
        // The binary search in the kernel assumes both, and the failure if
        // either is false is a search that returns a column it was not looking
        // for, which reads as a sky sampled through a kaleidoscope.
        let (w, h) = (16u32, 8u32);
        let mut pixels = flat(w, h, 0.0);
        for (i, p) in pixels.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let v = (i % 7) as f32;
            p[0] = v;
            p[1] = v;
            p[2] = v;
        }
        let dist = EnvDistribution::build(w, h, &pixels);
        assert!(!dist.is_empty());

        let marginal = dist.marginal();
        assert_eq!(marginal.len(), h as usize);
        for pair in marginal.windows(2) {
            assert!(pair[1] >= pair[0], "the marginal table went backwards");
        }
        assert_eq!(marginal[marginal.len() - 1], 1.0);

        let conditional = dist.conditional();
        assert_eq!(conditional.len(), (w * h) as usize);
        for row in conditional.chunks_exact(w as usize) {
            for pair in row.windows(2) {
                assert!(pair[1] >= pair[0], "a conditional row went backwards");
            }
            assert_eq!(row[row.len() - 1], 1.0);
        }
    }

    #[test]
    fn an_all_black_row_still_carries_a_searchable_table() {
        // It is never chosen, because its marginal weight is zero, but a
        // variate that reaches it through a rounding error has to find a column.
        let (w, h) = (8u32, 4u32);
        let black_row = 1usize;
        let mut pixels = flat(w, h, 1.0);
        for x in 0..w as usize {
            for c in 0..3 {
                pixels[(black_row * w as usize + x) * 3 + c] = 0.0;
            }
        }
        let dist = EnvDistribution::build(w, h, &pixels);
        let row = &dist.conditional()[w as usize..2 * w as usize];
        for pair in row.windows(2) {
            assert!(pair[1] > pair[0]);
        }
        assert_eq!(row[row.len() - 1], 1.0);
    }

    #[test]
    fn the_solid_angle_correction_stops_the_poles_being_oversampled() {
        // The whole point, and the thing the source material leaves undone. On
        // a uniformly bright image the rows are equally bright and are NOT
        // equally likely: a row at the pole subtends almost no sky.
        let (w, h) = (4u32, 16u32);
        let dist = EnvDistribution::build(w, h, &flat(w, h, 1.0));

        // The marginal is cumulative, so a row's own share is its difference.
        let share = |y: usize| {
            let m = dist.marginal();
            if y == 0 { m[0] } else { m[y] - m[y - 1] }
        };
        let pole = share(0);
        let equator = share(h as usize / 2);
        assert!(
            equator > pole * 5.0,
            "an equatorial row subtends far more sky than a polar one, so it \
             has to be far more likely: pole {pole}, equator {equator}"
        );

        // And the shares still sum to one, which is what says the correction
        // redistributed the probability rather than losing some of it.
        let total: f32 = (0..h as usize).map(share).sum();
        assert!((total - 1.0).abs() < 1e-5, "shares sum to {total}");
    }

    #[test]
    fn a_uniform_image_has_a_uniform_solid_angle_density_at_every_latitude() {
        // The sharpest statement of the correction being right. Under a
        // genuinely uniform environment every direction is equally likely per
        // unit solid angle, so the density is the constant `1 / (4 * PI)` at
        // the pole as much as at the equator. Without the `sin(theta)` in the
        // weights it comes out hundreds of times too large at the top row.
        //
        // Evaluated at row centres, and that is not a convenience. The sampler
        // draws uniformly in `uv` **within** a cell while solid angle does not
        // vary uniformly with `v`, so the exact density genuinely changes
        // across one cell, by six percent across the polar cell of a 32-row
        // image. That is the density describing its sampler rather than an
        // error in it, and it vanishes with resolution; at a row's centre the
        // cell's own `sin(theta)` and the point's are the same number and the
        // constant is exact.
        let (w, h) = (64u32, 32u32);
        let pixels = flat(w, h, 0.5);
        let dist = EnvDistribution::build(w, h, &pixels);
        let uniform = 1.0 / (4.0 * std::f32::consts::PI);
        for y in [0u32, 1, 8, 16, 24, h - 1] {
            #[allow(clippy::cast_precision_loss)]
            let v = (y as f32 + 0.5) / h as f32;
            let pdf = dist.pdf(0.37, v, &pixels);
            let error = (pdf - uniform).abs() / uniform;
            assert!(
                error < 0.01,
                "at row {y} the density is {pdf} where a uniform environment \
                 requires {uniform}"
            );
        }
    }

    #[test]
    fn the_density_integrates_to_one_over_the_sphere() {
        // What it means for a density to be a density, and the one check that
        // exercises the weights, the normalization and the density formula
        // together: get any of the three wrong by a factor and this is off by
        // that factor.
        //
        // Integrated in the image's own parameterization, where the solid angle
        // element is `2 * PI^2 * sin(theta) du dv`, over cell centres. A
        // non-uniform image, because a flat one would pass a construction that
        // ignored the pixels entirely.
        let (w, h) = (48u32, 24u32);
        let mut pixels = flat(w, h, 0.0);
        for (i, p) in pixels.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let value = 0.05 + ((i * 37) % 23) as f32;
            p[0] = value;
            p[1] = value * 0.6;
            p[2] = value * 0.3;
        }
        let dist = EnvDistribution::build(w, h, &pixels);

        let mut integral = 0.0f64;
        for y in 0..h {
            for x in 0..w {
                #[allow(clippy::cast_precision_loss)]
                let u = (x as f32 + 0.5) / w as f32;
                #[allow(clippy::cast_precision_loss)]
                let v = (y as f32 + 0.5) / h as f32;
                let theta = f64::from(v) * std::f64::consts::PI;
                let jacobian = 2.0 * std::f64::consts::PI.powi(2) * theta.sin();
                #[allow(clippy::cast_precision_loss)]
                let cell = 1.0 / f64::from(w * h);
                integral += f64::from(dist.pdf(u, v, &pixels)) * jacobian * cell;
            }
        }
        assert!(
            (integral - 1.0).abs() < 1e-3,
            "a probability density has to integrate to one over the sphere, \
             got {integral}"
        );
    }

    #[test]
    fn a_bright_patch_is_more_likely_in_proportion_to_how_bright_it_is() {
        let (w, h) = (32u32, 16u32);
        let mut pixels = flat(w, h, 1.0);
        // Ten times brighter in one pixel on the equator.
        let y = h as usize / 2;
        let x = 7usize;
        for c in 0..3 {
            pixels[(y * w as usize + x) * 3 + c] = 10.0;
        }
        let dist = EnvDistribution::build(w, h, &pixels);
        #[allow(clippy::cast_precision_loss)]
        let u = (x as f32 + 0.5) / w as f32;
        #[allow(clippy::cast_precision_loss)]
        let v = (y as f32 + 0.5) / h as f32;
        let bright = dist.pdf(u, v, &pixels);
        let ordinary = dist.pdf(u, v + 2.0 / h as f32, &pixels);
        let ratio = bright / ordinary;
        assert!(
            (ratio - 10.0).abs() < 0.5,
            "a pixel ten times brighter should be about ten times as likely, \
             got {ratio}"
        );
    }
}
