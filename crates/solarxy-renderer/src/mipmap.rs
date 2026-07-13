//! CPU mip-chain generation for material textures.
//!
//! wgpu has no built-in mip generation. The chain is built CPU-side with a
//! 2x2 box filter and uploaded level by level via `Queue::write_texture`
//! (which has no row-alignment constraint), so it needs no render pipeline,
//! no `RENDER_ATTACHMENT` usage on sRGB textures, and runs identically on
//! native and wasm. Color (sRGB) data is filtered in linear space through a
//! lookup table; linear data (normal maps, ORM) averages bytes directly.
//! Runs once per unique image at upload time, never per frame.

/// Number of mip levels for a full chain down to 1x1.
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// One mip level below the base: pixels plus its dimensions.
pub struct MipLevel {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Build every level below the base (level 0 is the caller's input and is
/// not duplicated here). `srgb` selects linear-space filtering for the RGB
/// channels; alpha always averages linearly.
pub fn build_mip_chain(rgba: &[u8], width: u32, height: u32, srgb: bool) -> Vec<MipLevel> {
    let count = mip_level_count(width, height);
    let mut levels: Vec<MipLevel> = Vec::with_capacity(count.saturating_sub(1) as usize);

    for i in 1..count {
        let (pixels, w, h) = {
            let (src, sw, sh): (&[u8], u32, u32) = match levels.last() {
                None => (rgba, width, height),
                Some(l) => (&l.pixels, l.width, l.height),
            };
            let w = (sw / 2).max(1);
            let h = (sh / 2).max(1);
            (downsample(src, sw, sh, w, h, srgb), w, h)
        };
        levels.push(MipLevel {
            pixels,
            width: w,
            height: h,
        });
        debug_assert_eq!(i as usize, levels.len());
    }
    levels
}

/// 2x2 box filter (clamped at odd edges) from `(sw, sh)` down to `(dw, dh)`.
fn downsample(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32, srgb: bool) -> Vec<u8> {
    let lut = if srgb {
        Some(srgb_to_linear_lut())
    } else {
        None
    };
    let mut dst = vec![0u8; (dw * dh * 4) as usize];

    for y in 0..dh {
        let sy0 = (y * 2).min(sh - 1);
        let sy1 = (y * 2 + 1).min(sh - 1);
        for x in 0..dw {
            let sx0 = (x * 2).min(sw - 1);
            let sx1 = (x * 2 + 1).min(sw - 1);
            let corners = [
                ((sy0 * sw + sx0) * 4) as usize,
                ((sy0 * sw + sx1) * 4) as usize,
                ((sy1 * sw + sx0) * 4) as usize,
                ((sy1 * sw + sx1) * 4) as usize,
            ];
            let di = ((y * dw + x) * 4) as usize;
            for c in 0..3 {
                dst[di + c] = if let Some(lut) = &lut {
                    let sum: f32 = corners.iter().map(|&o| lut[src[o + c] as usize]).sum();
                    linear_to_srgb_u8(sum * 0.25)
                } else {
                    let sum: u32 = corners.iter().map(|&o| u32::from(src[o + c])).sum();
                    (sum / 4) as u8
                };
            }
            let asum: u32 = corners.iter().map(|&o| u32::from(src[o + 3])).sum();
            dst[di + 3] = (asum / 4) as u8;
        }
    }
    dst
}

fn srgb_to_linear_lut() -> [f32; 256] {
    let mut lut = [0.0f32; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        let s = i as f32 / 255.0;
        *v = if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
    }
    lut
}

fn linear_to_srgb_u8(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let s = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_counts() {
        assert_eq!(mip_level_count(1, 1), 1);
        assert_eq!(mip_level_count(2, 2), 2);
        assert_eq!(mip_level_count(4, 4), 3);
        assert_eq!(mip_level_count(4096, 4096), 13);
        assert_eq!(mip_level_count(640, 480), 10);
    }

    #[test]
    fn chain_reaches_one_by_one_for_non_square() {
        let base = vec![255u8; 8 * 2 * 4];
        let levels = build_mip_chain(&base, 8, 2, false);
        let dims: Vec<(u32, u32)> = levels.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(dims, vec![(4, 1), (2, 1), (1, 1)]);
    }

    #[test]
    fn linear_average_is_exact() {
        // 2x2 linear data averaging to one texel.
        let base = vec![
            0, 0, 0, 0, //
            100, 40, 8, 40, //
            100, 40, 8, 40, //
            200, 80, 16, 80,
        ];
        let levels = build_mip_chain(&base, 2, 2, false);
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].pixels, vec![100, 40, 8, 40]);
    }

    #[test]
    fn srgb_average_filters_in_linear_space() {
        // Black and white checker: averaging in linear space then encoding
        // back gives ~188, NOT the naive byte average of 127.
        let base = vec![
            0, 0, 0, 255, //
            255, 255, 255, 255, //
            255, 255, 255, 255, //
            0, 0, 0, 255,
        ];
        let levels = build_mip_chain(&base, 2, 2, true);
        assert_eq!(levels.len(), 1);
        let px = &levels[0].pixels;
        assert_eq!(px[3], 255);
        assert!(
            px[0] >= 186 && px[0] <= 190,
            "expected ~188 (linear-space average), got {}",
            px[0]
        );
        assert_eq!(px[0], px[1]);
        assert_eq!(px[1], px[2]);
    }

    #[test]
    fn odd_dimensions_clamp() {
        let base = vec![10u8; 3 * 3 * 4];
        let levels = build_mip_chain(&base, 3, 3, false);
        let dims: Vec<(u32, u32)> = levels.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(dims, vec![(1, 1)]);
        assert_eq!(levels[0].pixels, vec![10, 10, 10, 10]);
    }
}
