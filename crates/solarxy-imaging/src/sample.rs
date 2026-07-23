//! CPU texture sampling over [`RawImageData`]: single-texel reads for
//! operators that pull an image through arbitrary coordinates (the
//! `attribute_from_image` node samples through mesh UVs).
//!
//! Coordinate convention, pinned by test: `(u, v) = (0, 0)` reads the
//! image's TOP-LEFT texel, matching the renderer exactly (the main shader
//! samples `tex_coords` unflipped against pixel data whose first row is
//! the image top), so a value sampled here agrees with the texture
//! rendered under the same UV. Values return raw (no color-space
//! conversion) in 0..=1, the crate's stored-bytes policy.

use solarxy_core::RawImageData;

/// How out-of-range coordinates resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    /// Tile: the coordinate wraps modulo 1.
    Repeat,
    /// Extend: the edge texel repeats outward.
    Clamp,
}

/// The reconstruction filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Bilinear,
}

/// One texel by integer coordinate, wrapped into range.
fn fetch(img: &RawImageData, x: i64, y: i64, wrap: WrapMode) -> [f32; 4] {
    let (w, h) = (i64::from(img.width), i64::from(img.height));
    let (x, y) = match wrap {
        WrapMode::Repeat => (x.rem_euclid(w), y.rem_euclid(h)),
        WrapMode::Clamp => (x.clamp(0, w - 1), y.clamp(0, h - 1)),
    };
    let i = usize::try_from((y * w + x) * 4).unwrap_or(0);
    match img.pixels.get(i..i + 4) {
        Some(px) => [
            f32::from(px[0]) / 255.0,
            f32::from(px[1]) / 255.0,
            f32::from(px[2]) / 255.0,
            f32::from(px[3]) / 255.0,
        ],
        None => [0.0, 0.0, 0.0, 0.0],
    }
}

/// Samples the image at `(u, v)` (see the module doc for orientation),
/// returning raw RGBA in 0..=1. An empty image samples transparent black.
#[must_use]
pub fn sample(img: &RawImageData, u: f32, v: f32, filter: Filter, wrap: WrapMode) -> [f32; 4] {
    if img.width == 0 || img.height == 0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let (w, h) = (img.width as f32, img.height as f32);
    match filter {
        Filter::Nearest => {
            let x = (u * w).floor() as i64;
            let y = (v * h).floor() as i64;
            fetch(img, x, y, wrap)
        }
        Filter::Bilinear => {
            // Texel centers sit at integer + 0.5; the -0.5 shift makes the
            // interpolation weights come out of the fractional part.
            let fx = u * w - 0.5;
            let fy = v * h - 0.5;
            let x0 = fx.floor();
            let y0 = fy.floor();
            let tx = fx - x0;
            let ty = fy - y0;
            let (x0, y0) = (x0 as i64, y0 as i64);
            let c00 = fetch(img, x0, y0, wrap);
            let c10 = fetch(img, x0 + 1, y0, wrap);
            let c01 = fetch(img, x0, y0 + 1, wrap);
            let c11 = fetch(img, x0 + 1, y0 + 1, wrap);
            std::array::from_fn(|c| {
                let top = c00[c] + (c10[c] - c00[c]) * tx;
                let bottom = c01[c] + (c11[c] - c01[c]) * tx;
                top + (bottom - top) * ty
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    /// RGBA texels in row-major order, row 0 first (the image top).
    fn image(width: u32, height: u32, texels: &[[u8; 4]]) -> RawImageData {
        assert_eq!(texels.len(), (width * height) as usize);
        RawImageData::new(texels.iter().flatten().copied().collect(), width, height)
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    #[test]
    fn v_zero_is_the_image_top() {
        // 1 wide, 2 tall: red on top, blue below. The renderer convention
        // this pins is load-bearing for attribute_from_image parity.
        let img = image(1, 2, &[RED, BLUE]);
        let top = sample(&img, 0.5, 0.25, Filter::Nearest, WrapMode::Clamp);
        let bottom = sample(&img, 0.5, 0.75, Filter::Nearest, WrapMode::Clamp);
        assert_eq!(top, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(bottom, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn nearest_reads_exact_texels() {
        let img = image(2, 2, &[RED, BLUE, WHITE, BLACK]);
        assert_eq!(
            sample(&img, 0.25, 0.25, Filter::Nearest, WrapMode::Clamp),
            [1.0, 0.0, 0.0, 1.0]
        );
        assert_eq!(
            sample(&img, 0.75, 0.75, Filter::Nearest, WrapMode::Clamp),
            [0.0, 0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn bilinear_blends_at_the_texel_seam() {
        let img = image(2, 1, &[BLACK, WHITE]);
        // The midpoint between the two texel centers.
        let mid = sample(&img, 0.5, 0.5, Filter::Bilinear, WrapMode::Clamp);
        assert!((mid[0] - 0.5).abs() < 1e-6, "{mid:?}");
        // At a texel center the blend collapses to that texel.
        let left = sample(&img, 0.25, 0.5, Filter::Bilinear, WrapMode::Clamp);
        assert_eq!(left[0], 0.0);
    }

    #[test]
    fn repeat_tiles_and_clamp_extends() {
        let img = image(2, 1, &[BLACK, WHITE]);
        // Just past the right edge: repeat wraps to the left (black)
        // texel, clamp stays on the right (white) one.
        assert_eq!(
            sample(&img, 1.1, 0.5, Filter::Nearest, WrapMode::Repeat)[0],
            0.0
        );
        assert_eq!(
            sample(&img, 1.1, 0.5, Filter::Nearest, WrapMode::Clamp)[0],
            1.0
        );
        // Just below zero mirrors the same pair.
        assert_eq!(
            sample(&img, -0.1, 0.5, Filter::Nearest, WrapMode::Repeat)[0],
            1.0
        );
        assert_eq!(
            sample(&img, -0.1, 0.5, Filter::Nearest, WrapMode::Clamp)[0],
            0.0
        );
    }

    #[test]
    fn an_empty_image_samples_transparent_black() {
        let img = RawImageData::new(Vec::new(), 0, 0);
        assert_eq!(
            sample(&img, 0.5, 0.5, Filter::Bilinear, WrapMode::Repeat),
            [0.0, 0.0, 0.0, 0.0]
        );
    }
}
