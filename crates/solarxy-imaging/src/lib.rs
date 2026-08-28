//! Pure-CPU image operators for the Solarxy texture context
//! .
//!
//! Every operator maps `&RawImageData` (RGBA8) plus parameters to a new
//! `RawImageData`, synchronously and single-threaded: the engine crate
//! never links wgpu (architecture invariant 1), and the web build has no
//! threads (COEP is off until wasm-threads land, at which point these
//! per-pixel loops adopt rayon without API changes). Operators are
//! deterministic: `noise` is hash-based with an explicit seed, so a cook
//! is reproducible across shells and sessions.
//!
//! Color-space policy: operators work directly on the stored (sRGB-
//! encoded) bytes, the convention of 2D image editors, so results match
//! what a texture artist expects from Photoshop-style tools. Alpha is
//! carried through untouched unless an operator documents otherwise.

// Allow list kept consistent with `solarxy-kernel`, this crate's closest
// sibling (pure-CPU, wasm-clean, no fs or GPU): operator errors are described
// on the `ImagingError` variants rather than repeated in each `# Errors`
// section.
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::many_single_char_names
)]

use solarxy_core::RawImageData;

pub mod sample;

/// A parameter failure (dimension bounds, empty inputs). Cook bodies map
/// these onto `CookError::Failed`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImagingError {
    #[error("image dimensions must be 1..=8192, got {width}x{height}")]
    BadDimensions { width: u32, height: u32 },
}

/// The hard ceiling any generator or resize accepts.
pub const MAX_EDGE: u32 = 8192;

fn check_dims(width: u32, height: u32) -> Result<(), ImagingError> {
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
        return Err(ImagingError::BadDimensions { width, height });
    }
    Ok(())
}

/// Per-pixel map over RGB, alpha untouched.
fn map_rgb(src: &RawImageData, mut f: impl FnMut(u8) -> u8) -> RawImageData {
    let mut px = src.pixels.clone();
    for chunk in px.as_chunks_mut::<4>().0 {
        chunk[0] = f(chunk[0]);
        chunk[1] = f(chunk[1]);
        chunk[2] = f(chunk[2]);
    }
    RawImageData::new(px, src.width, src.height)
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

// ---- Adjust ----

/// Photoshop-style levels: input black/white points remap to 0..1, a
/// midtone gamma applies, then the range maps onto the output points.
#[must_use]
pub fn levels(
    src: &RawImageData,
    in_black: f32,
    in_white: f32,
    gamma: f32,
    out_black: f32,
    out_white: f32,
) -> RawImageData {
    let lo = in_black.clamp(0.0, 1.0);
    let hi = in_white.clamp(0.0, 1.0).max(lo + 1e-4);
    let g = gamma.max(1e-3);
    let (ob, ow) = (out_black.clamp(0.0, 1.0), out_white.clamp(0.0, 1.0));
    let lut: Vec<u8> = (0..=255u32)
        .map(|v| {
            let x = ((v as f32 / 255.0 - lo) / (hi - lo)).clamp(0.0, 1.0);
            let y = x.powf(1.0 / g);
            to_u8(ob + y * (ow - ob))
        })
        .collect();
    map_rgb(src, |v| lut[v as usize])
}

/// Brightness in -1..1 (additive), contrast in -1..1 (pivot 0.5).
#[must_use]
pub fn brightness_contrast(src: &RawImageData, brightness: f32, contrast: f32) -> RawImageData {
    let b = brightness.clamp(-1.0, 1.0);
    let slope = (1.0 + contrast.clamp(-1.0, 1.0)).max(0.0);
    let lut: Vec<u8> = (0..=255u32)
        .map(|v| to_u8((v as f32 / 255.0 - 0.5) * slope + 0.5 + b))
        .collect();
    map_rgb(src, |v| lut[v as usize])
}

/// Hue shift in degrees, saturation and lightness as multipliers (1 =
/// identity). Works in HSL per pixel.
#[must_use]
pub fn hue_saturation(
    src: &RawImageData,
    hue_deg: f32,
    saturation: f32,
    lightness: f32,
) -> RawImageData {
    let mut px = src.pixels.clone();
    for chunk in px.as_chunks_mut::<4>().0 {
        let (h, s, l) = rgb_to_hsl(
            f32::from(chunk[0]) / 255.0,
            f32::from(chunk[1]) / 255.0,
            f32::from(chunk[2]) / 255.0,
        );
        let h = (h + hue_deg / 360.0).rem_euclid(1.0);
        let s = (s * saturation.max(0.0)).clamp(0.0, 1.0);
        let l = (l * lightness.max(0.0)).clamp(0.0, 1.0);
        let (r, g, b) = hsl_to_rgb(h, s, l);
        chunk[0] = to_u8(r);
        chunk[1] = to_u8(g);
        chunk[2] = to_u8(b);
    }
    RawImageData::new(px, src.width, src.height)
}

/// RGB inversion (alpha untouched).
#[must_use]
pub fn invert(src: &RawImageData) -> RawImageData {
    map_rgb(src, |v| 255 - v)
}

/// Plain gamma curve (1 = identity).
#[must_use]
pub fn gamma(src: &RawImageData, gamma: f32) -> RawImageData {
    let g = gamma.max(1e-3);
    let lut: Vec<u8> = (0..=255u32)
        .map(|v| to_u8((v as f32 / 255.0).powf(1.0 / g)))
        .collect();
    map_rgb(src, |v| lut[v as usize])
}

// ---- Composite ----

/// Two-input blend modes. `Over` is source-over alpha compositing (B over
/// A is NOT commutative: the SECOND input composites over the first);
/// everything else blends RGB by `factor` with A's alpha kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Over,
    Multiply,
    Add,
    Screen,
    Overlay,
}

/// Blends `b` onto `a`. The output takes `a`'s dimensions; `b` is
/// nearest-sampled to fit when the sizes differ. `factor` 0..1 fades the
/// effect (0 = a untouched).
#[must_use]
pub fn mix(a: &RawImageData, b: &RawImageData, mode: BlendMode, factor: f32) -> RawImageData {
    let f = factor.clamp(0.0, 1.0);
    let (w, h) = (a.width, a.height);
    let mut out = a.pixels.clone();
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            // Nearest sample of b at the same normalized position.
            let bx = (u64::from(x) * u64::from(b.width) / u64::from(w.max(1))) as u32;
            let by = (y * b.height / h.max(1)).min(b.height - 1);
            let j = ((by * b.width + bx.min(b.width - 1)) * 4) as usize;

            let (ar, ag, ab_, aa) = (
                f32::from(a.pixels[i]) / 255.0,
                f32::from(a.pixels[i + 1]) / 255.0,
                f32::from(a.pixels[i + 2]) / 255.0,
                f32::from(a.pixels[i + 3]) / 255.0,
            );
            let (br, bg, bb, ba) = (
                f32::from(b.pixels[j]) / 255.0,
                f32::from(b.pixels[j + 1]) / 255.0,
                f32::from(b.pixels[j + 2]) / 255.0,
                f32::from(b.pixels[j + 3]) / 255.0,
            );
            let blend = |x: f32, y: f32| -> f32 {
                match mode {
                    BlendMode::Normal | BlendMode::Over => y,
                    BlendMode::Multiply => x * y,
                    BlendMode::Add => (x + y).min(1.0),
                    BlendMode::Screen => 1.0 - (1.0 - x) * (1.0 - y),
                    BlendMode::Overlay => {
                        if x < 0.5 {
                            2.0 * x * y
                        } else {
                            1.0 - 2.0 * (1.0 - x) * (1.0 - y)
                        }
                    }
                }
            };
            if mode == BlendMode::Over {
                // Source-over with b's own alpha scaled by the factor.
                let sa = ba * f;
                let oa = sa + aa * (1.0 - sa);
                let comp = |s: f32, d: f32| {
                    if oa > 0.0 {
                        (s * sa + d * aa * (1.0 - sa)) / oa
                    } else {
                        0.0
                    }
                };
                out[i] = to_u8(comp(br, ar));
                out[i + 1] = to_u8(comp(bg, ag));
                out[i + 2] = to_u8(comp(bb, ab_));
                out[i + 3] = to_u8(oa);
            } else {
                out[i] = to_u8(ar + (blend(ar, br) - ar) * f);
                out[i + 1] = to_u8(ag + (blend(ag, bg) - ag) * f);
                out[i + 2] = to_u8(ab_ + (blend(ab_, bb) - ab_) * f);
            }
        }
    }
    RawImageData::new(out, w, h)
}

// ---- PBR utility ----

/// Packs the glTF ORM layout the renderer consumes: R = occlusion,
/// G = roughness, B = metallic, each taken from the corresponding
/// input's red channel (a missing input holds its constant fallback).
/// The output takes the dimensions of the first present input (or
/// 1x1 when none is connected).
#[must_use]
pub fn pack_orm(
    occlusion: Option<&RawImageData>,
    roughness: Option<&RawImageData>,
    metallic: Option<&RawImageData>,
    fallback_occlusion: f32,
    fallback_roughness: f32,
    fallback_metallic: f32,
) -> RawImageData {
    let (w, h) = [occlusion, roughness, metallic]
        .iter()
        .flatten()
        .next()
        .map_or((1, 1), |img| (img.width, img.height));
    let sample = |src: Option<&RawImageData>, fallback: f32, x: u32, y: u32| -> u8 {
        match src {
            None => to_u8(fallback),
            Some(img) => {
                let sx = (u64::from(x) * u64::from(img.width) / u64::from(w.max(1))) as u32;
                let sy = (y * img.height / h.max(1)).min(img.height - 1);
                img.pixels[(((sy * img.width) + sx.min(img.width - 1)) * 4) as usize]
            }
        }
    };
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.push(sample(occlusion, fallback_occlusion, x, y));
            px.push(sample(roughness, fallback_roughness, x, y));
            px.push(sample(metallic, fallback_metallic, x, y));
            px.push(255);
        }
    }
    RawImageData::new(px, w, h)
}

/// Sobel height-to-normal: the input's red channel is the height field;
/// `strength` scales the slope. Output is a tangent-space normal map
/// (0.5, 0.5, 1.0 = flat), edge pixels clamp.
#[must_use]
pub fn height_to_normal(src: &RawImageData, strength: f32) -> RawImageData {
    let (w, h) = (i64::from(src.width), i64::from(src.height));
    let height_at = |x: i64, y: i64| -> f32 {
        let cx = x.clamp(0, w - 1);
        let cy = y.clamp(0, h - 1);
        f32::from(src.pixels[((cy * w + cx) * 4) as usize]) / 255.0
    };
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let tl = height_at(x - 1, y - 1);
            let l = height_at(x - 1, y);
            let bl = height_at(x - 1, y + 1);
            let tr = height_at(x + 1, y - 1);
            let r = height_at(x + 1, y);
            let br = height_at(x + 1, y + 1);
            let t = height_at(x, y - 1);
            let b = height_at(x, y + 1);
            let dx = (tr + 2.0 * r + br) - (tl + 2.0 * l + bl);
            let dy = (bl + 2.0 * b + br) - (tl + 2.0 * t + tr);
            let n = normalize3(-dx * strength, -dy * strength, 1.0);
            px.push(to_u8(n.0 * 0.5 + 0.5));
            px.push(to_u8(n.1 * 0.5 + 0.5));
            px.push(to_u8(n.2 * 0.5 + 0.5));
            px.push(255);
        }
    }
    RawImageData::new(px, src.width, src.height)
}

// ---- Generate + filter ----

/// A solid color (RGBA 0..1).
pub fn constant(width: u32, height: u32, color: [f32; 4]) -> Result<RawImageData, ImagingError> {
    check_dims(width, height)?;
    let texel = [
        to_u8(color[0]),
        to_u8(color[1]),
        to_u8(color[2]),
        to_u8(color[3]),
    ];
    Ok(RawImageData::new(
        texel.repeat((width * height) as usize),
        width,
        height,
    ))
}

/// Ramp direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampDirection {
    Horizontal,
    Vertical,
    Radial,
}

/// A two-color gradient.
pub fn ramp(
    width: u32,
    height: u32,
    direction: RampDirection,
    from: [f32; 4],
    to: [f32; 4],
) -> Result<RawImageData, ImagingError> {
    check_dims(width, height)?;
    let mut px = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let t = match direction {
                RampDirection::Horizontal => x as f32 / (width - 1).max(1) as f32,
                RampDirection::Vertical => y as f32 / (height - 1).max(1) as f32,
                RampDirection::Radial => {
                    let cx = x as f32 / (width - 1).max(1) as f32 - 0.5;
                    let cy = y as f32 / (height - 1).max(1) as f32 - 0.5;
                    (cx * cx + cy * cy).sqrt() * 2.0
                }
            }
            .clamp(0.0, 1.0);
            for c in 0..4 {
                px.push(to_u8(from[c] + (to[c] - from[c]) * t));
            }
        }
    }
    Ok(RawImageData::new(px, width, height))
}

/// Deterministic value noise: `scale` is the feature cell count across
/// the width; same seed, same image, everywhere.
pub fn noise(width: u32, height: u32, scale: f32, seed: u32) -> Result<RawImageData, ImagingError> {
    check_dims(width, height)?;
    let cells = scale.max(1.0);
    let lattice = |ix: i64, iy: i64| -> f32 {
        // A small avalanche hash over the lattice point and seed.
        let mut v = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ u64::from(seed).wrapping_mul(0x1656_67B1_9E37_79F9);
        v ^= v >> 33;
        v = v.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        v ^= v >> 33;
        (v & 0xFFFF) as f32 / 65535.0
    };
    let mut px = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let fx = x as f32 / width as f32 * cells;
            let fy = y as f32 / height as f32 * cells;
            let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
            let (tx, ty) = (fx - fx.floor(), fy - fy.floor());
            // Smoothstep-interpolated bilinear lattice noise.
            let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
            let v = lerp(
                lerp(lattice(ix, iy), lattice(ix + 1, iy), sx),
                lerp(lattice(ix, iy + 1), lattice(ix + 1, iy + 1), sx),
                sy,
            );
            let g = to_u8(v);
            px.extend_from_slice(&[g, g, g, 255]);
        }
    }
    Ok(RawImageData::new(px, width, height))
}

/// Distance metric for [`voronoi`] cell membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoronoiMetric {
    Euclidean,
    Manhattan,
    Chebyshev,
}

/// What [`voronoi`] writes per texel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoronoiPattern {
    /// Distance to the nearest feature point (F1).
    Distance,
    /// A per-cell hashed value, flat within each cell.
    CellId,
    /// Bright lines along the cell boundaries (F2 minus F1).
    Edges,
}

/// A 64-bit avalanche hash of a lattice cell and seed (the `noise` hash with
/// one extra mixing round, so both value lanes are well distributed).
fn cell_hash(ix: i64, iy: i64, seed: u32) -> u64 {
    let mut v = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ u64::from(seed).wrapping_mul(0x1656_67B1_9E37_79F9);
    v ^= v >> 33;
    v = v.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    v ^= v >> 33;
    v = v.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    v ^= v >> 33;
    v
}

/// Worley / Voronoi cellular noise: scatters one jittered feature point per
/// lattice cell and, per texel, reports the nearest-feature distance, the
/// owning cell's hashed value, or the cell edges. Deterministic in `seed`.
#[allow(clippy::too_many_arguments)]
pub fn voronoi(
    width: u32,
    height: u32,
    scale: f32,
    seed: u32,
    jitter: f32,
    metric: VoronoiMetric,
    pattern: VoronoiPattern,
) -> Result<RawImageData, ImagingError> {
    check_dims(width, height)?;
    let cells = scale.max(0.5);
    let jitter = jitter.clamp(0.0, 1.0);
    let dist = |dx: f32, dy: f32| match metric {
        VoronoiMetric::Euclidean => (dx * dx + dy * dy).sqrt(),
        VoronoiMetric::Manhattan => dx.abs() + dy.abs(),
        VoronoiMetric::Chebyshev => dx.abs().max(dy.abs()),
    };
    let mut px = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let fx = x as f32 / width as f32 * cells;
            let fy = y as f32 / height as f32 * cells;
            let (cx, cy) = (fx.floor() as i64, fy.floor() as i64);
            let mut f1 = f32::MAX;
            let mut f2 = f32::MAX;
            let mut best_cell = (cx, cy);
            for oy in -1..=1 {
                for ox in -1..=1 {
                    let (gx, gy) = (cx + ox, cy + oy);
                    let h = cell_hash(gx, gy, seed);
                    let rx = (h & 0xFFFF) as f32 / 65535.0;
                    let ry = ((h >> 16) & 0xFFFF) as f32 / 65535.0;
                    // Feature point: cell centre, displaced by jitter.
                    let feat_x = gx as f32 + 0.5 + jitter * (rx - 0.5);
                    let feat_y = gy as f32 + 0.5 + jitter * (ry - 0.5);
                    let d = dist(fx - feat_x, fy - feat_y);
                    if d < f1 {
                        f2 = f1;
                        f1 = d;
                        best_cell = (gx, gy);
                    } else if d < f2 {
                        f2 = d;
                    }
                }
            }
            let v = match pattern {
                VoronoiPattern::Distance => f1.min(1.0),
                VoronoiPattern::CellId => {
                    let h = cell_hash(best_cell.0, best_cell.1, seed ^ 0x5A5A_5A5A);
                    (h & 0xFFFF) as f32 / 65535.0
                }
                VoronoiPattern::Edges => (1.0 - (f2 - f1) * 4.0).clamp(0.0, 1.0),
            };
            let g = to_u8(v);
            px.extend_from_slice(&[g, g, g, 255]);
        }
    }
    Ok(RawImageData::new(px, width, height))
}

/// How [`gradient`] measures its blend factor around the centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientMode {
    /// Left-to-right, with the centre's x as the midpoint.
    Linear,
    /// Outward from the centre.
    Radial,
    /// A conic sweep of the angle around the centre.
    Angular,
    /// Manhattan distance from the centre (a rotated square).
    Diamond,
}

/// A two-color gradient with a movable centre and four falloff modes. Richer
/// than `ramp`: it adds the angular (conic) and diamond falloffs and a
/// configurable centre that `ramp`'s fixed horizontal / vertical / radial
/// lacks.
pub fn gradient(
    width: u32,
    height: u32,
    mode: GradientMode,
    color_a: [f32; 4],
    color_b: [f32; 4],
    center: [f32; 2],
) -> Result<RawImageData, ImagingError> {
    check_dims(width, height)?;
    let (cx, cy) = (center[0], center[1]);
    let mut px = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let nx = x as f32 / (width - 1).max(1) as f32;
            let ny = y as f32 / (height - 1).max(1) as f32;
            let (dx, dy) = (nx - cx, ny - cy);
            let t = match mode {
                GradientMode::Linear => 0.5 + (nx - cx),
                GradientMode::Radial => (dx * dx + dy * dy).sqrt() * 2.0,
                GradientMode::Angular => dy.atan2(dx) / std::f32::consts::TAU + 0.5,
                GradientMode::Diamond => (dx.abs() + dy.abs()) * 2.0,
            }
            .clamp(0.0, 1.0);
            for c in 0..4 {
                px.push(to_u8(color_a[c] + (color_b[c] - color_a[c]) * t));
            }
        }
    }
    Ok(RawImageData::new(px, width, height))
}

/// A two-color checkerboard of `tiles_x` by `tiles_y` cells. Tiles map to a
/// normalized grid, so a non-square image gets stretched cells.
pub fn checker(
    width: u32,
    height: u32,
    color_a: [f32; 4],
    color_b: [f32; 4],
    tiles_x: u32,
    tiles_y: u32,
) -> Result<RawImageData, ImagingError> {
    check_dims(width, height)?;
    let tx = u64::from(tiles_x.max(1));
    let ty = u64::from(tiles_y.max(1));
    let a = [
        to_u8(color_a[0]),
        to_u8(color_a[1]),
        to_u8(color_a[2]),
        to_u8(color_a[3]),
    ];
    let b = [
        to_u8(color_b[0]),
        to_u8(color_b[1]),
        to_u8(color_b[2]),
        to_u8(color_b[3]),
    ];
    let mut px = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let cy = u64::from(y) * ty / u64::from(height);
        for x in 0..width {
            let cx = u64::from(x) * tx / u64::from(width);
            px.extend_from_slice(if (cx + cy) % 2 == 0 { &a } else { &b });
        }
    }
    Ok(RawImageData::new(px, width, height))
}

/// A running-bond brick wall: `columns` by `rows` bricks separated by mortar,
/// with alternate rows shifted by `row_offset`. `mortar` is the mortar width
/// as a fraction of a cell (0..0.5).
#[allow(clippy::too_many_arguments)]
pub fn brick(
    width: u32,
    height: u32,
    brick_color: [f32; 4],
    mortar_color: [f32; 4],
    columns: u32,
    rows: u32,
    mortar: f32,
    row_offset: f32,
) -> Result<RawImageData, ImagingError> {
    check_dims(width, height)?;
    let cols = columns.max(1) as f32;
    let rows_f = rows.max(1) as f32;
    let m = mortar.clamp(0.0, 0.5);
    let offset = row_offset.clamp(0.0, 1.0);
    let brick = [
        to_u8(brick_color[0]),
        to_u8(brick_color[1]),
        to_u8(brick_color[2]),
        to_u8(brick_color[3]),
    ];
    let mortar_c = [
        to_u8(mortar_color[0]),
        to_u8(mortar_color[1]),
        to_u8(mortar_color[2]),
        to_u8(mortar_color[3]),
    ];
    let mut px = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let v = y as f32 / height as f32 * rows_f;
        let row = v.floor();
        let row_v = v - row;
        let shift = if (row as i64) % 2 != 0 { offset } else { 0.0 };
        for x in 0..width {
            let u = x as f32 / width as f32 * cols + shift;
            let col_u = u - u.floor();
            let is_mortar = row_v < m || row_v > 1.0 - m || col_u < m || col_u > 1.0 - m;
            px.extend_from_slice(if is_mortar { &mortar_c } else { &brick });
        }
    }
    Ok(RawImageData::new(px, width, height))
}

/// Separable Gaussian blur; `radius` in pixels (0 = identity clone).
#[must_use]
pub fn blur(src: &RawImageData, radius: f32) -> RawImageData {
    let r = radius.clamp(0.0, 64.0);
    if r < 0.01 {
        return RawImageData::new(src.pixels.clone(), src.width, src.height);
    }
    let sigma = r / 2.0;
    let taps = r.ceil() as i64;
    let kernel: Vec<f32> = (-taps..=taps)
        .map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let norm: f32 = kernel.iter().sum();
    let (w, h) = (i64::from(src.width), i64::from(src.height));

    let pass = |input: &[u8], horizontal: bool| -> Vec<u8> {
        let mut out = vec![0u8; input.len()];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                // Zip the same range the kernel was built from, so the tap
                // offset is an i64 by construction.
                for (o, weight) in (-taps..=taps).zip(kernel.iter()) {
                    let (sx, sy) = if horizontal {
                        ((x + o).clamp(0, w - 1), y)
                    } else {
                        (x, (y + o).clamp(0, h - 1))
                    };
                    let idx = ((sy * w + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += f32::from(input[idx + c]) * weight;
                    }
                }
                let idx = ((y * w + x) * 4) as usize;
                for c in 0..4 {
                    out[idx + c] = (acc[c] / norm + 0.5) as u8;
                }
            }
        }
        out
    };
    let horizontal = pass(&src.pixels, true);
    RawImageData::new(pass(&horizontal, false), src.width, src.height)
}

/// Unsharp-mask sharpen: `amount` 0..4 scales the high-pass added back.
#[must_use]
pub fn sharpen(src: &RawImageData, amount: f32) -> RawImageData {
    let a = amount.clamp(0.0, 4.0);
    if a < 0.01 {
        return RawImageData::new(src.pixels.clone(), src.width, src.height);
    }
    let soft = blur(src, 1.5);
    let mut px = src.pixels.clone();
    for (i, (p, s)) in px.iter_mut().zip(soft.pixels.iter()).enumerate() {
        if i % 4 == 3 {
            continue; // alpha untouched
        }
        let v = f32::from(*p) + (f32::from(*p) - f32::from(*s)) * a;
        *p = to_u8(v / 255.0);
    }
    RawImageData::new(px, src.width, src.height)
}

/// Nearest-neighbour downscale so the long edge fits `max_edge` (the
/// working-resolution cap); returns a clone when already within bounds.
#[must_use]
pub fn clamp_to_edge(src: &RawImageData, max_edge: u32) -> RawImageData {
    let long = src.width.max(src.height);
    if long <= max_edge || max_edge == 0 {
        return RawImageData::new(src.pixels.clone(), src.width, src.height);
    }
    let scale = f64::from(max_edge) / f64::from(long);
    let w = ((f64::from(src.width) * scale).round() as u32).max(1);
    let h = ((f64::from(src.height) * scale).round() as u32).max(1);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let sx = (u64::from(x) * u64::from(src.width) / u64::from(w)) as u32;
            let sy = (u64::from(y) * u64::from(src.height) / u64::from(h)) as u32;
            let idx = ((sy.min(src.height - 1) * src.width + sx.min(src.width - 1)) * 4) as usize;
            px.extend_from_slice(&src.pixels[idx..idx + 4]);
        }
    }
    RawImageData::new(px, w, h)
}

// ---- helpers ----

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn normalize3(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let len = (x * x + y * y + z * z).sqrt().max(1e-6);
    (x / len, y / len, z / len)
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = f32::midpoint(max, min);
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < 1e-6 {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < 1e-6 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s < 1e-6 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| -> f32 {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swatch_2x2() -> RawImageData {
        // 2x2: black, white / white, black.
        RawImageData::new(
            vec![
                0, 0, 0, 255, 255, 255, 255, 255, //
                255, 255, 255, 255, 0, 0, 0, 255,
            ],
            2,
            2,
        )
    }

    #[test]
    fn invert_is_involutive() {
        let img = swatch_2x2();
        assert_eq!(invert(&invert(&img)).pixels, img.pixels);
    }

    #[test]
    fn identity_params_are_identities() {
        let img = swatch_2x2();
        assert_eq!(levels(&img, 0.0, 1.0, 1.0, 0.0, 1.0).pixels, img.pixels);
        assert_eq!(brightness_contrast(&img, 0.0, 0.0).pixels, img.pixels);
        assert_eq!(gamma(&img, 1.0).pixels, img.pixels);
        assert_eq!(blur(&img, 0.0).pixels, img.pixels);
        assert_eq!(sharpen(&img, 0.0).pixels, img.pixels);
    }

    #[test]
    fn constant_and_ramp_generate() {
        let c = constant(4, 2, [1.0, 0.0, 0.0, 1.0]).unwrap();
        assert_eq!((c.width, c.height), (4, 2));
        assert_eq!(&c.pixels[0..4], &[255, 0, 0, 255]);
        let r = ramp(8, 1, RampDirection::Horizontal, [0.0; 4], [1.0; 4]).unwrap();
        assert!(r.pixels[0] < r.pixels[4 * 7]);
        assert!(constant(0, 4, [0.0; 4]).is_err());
        assert!(constant(9000, 4, [0.0; 4]).is_err());
    }

    #[test]
    fn noise_is_deterministic_and_seed_sensitive() {
        let a = noise(16, 16, 4.0, 7).unwrap();
        let b = noise(16, 16, 4.0, 7).unwrap();
        let c = noise(16, 16, 4.0, 8).unwrap();
        assert_eq!(a.pixels, b.pixels);
        assert_ne!(a.pixels, c.pixels);
    }

    #[test]
    fn voronoi_is_deterministic_and_seed_sensitive() {
        let opts = (VoronoiMetric::Euclidean, VoronoiPattern::Distance);
        let a = voronoi(24, 24, 6.0, 3, 1.0, opts.0, opts.1).unwrap();
        let b = voronoi(24, 24, 6.0, 3, 1.0, opts.0, opts.1).unwrap();
        let c = voronoi(24, 24, 6.0, 4, 1.0, opts.0, opts.1).unwrap();
        assert_eq!((a.width, a.height), (24, 24));
        assert_eq!(a.pixels, b.pixels, "same seed reproduces");
        assert_ne!(a.pixels, c.pixels, "a new seed changes the pattern");
        assert!(voronoi(0, 4, 6.0, 0, 1.0, opts.0, opts.1).is_err());
        assert!(voronoi(9000, 4, 6.0, 0, 1.0, opts.0, opts.1).is_err());
    }

    #[test]
    fn gradient_runs_between_its_two_colors() {
        let g = gradient(
            16,
            1,
            GradientMode::Linear,
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [0.5, 0.5],
        )
        .unwrap();
        assert_eq!((g.width, g.height), (16, 1));
        assert!(
            g.pixels[0] < g.pixels[4 * 15],
            "dark at left, bright at right"
        );
        assert!(gradient(0, 4, GradientMode::Radial, [0.0; 4], [1.0; 4], [0.5, 0.5]).is_err());
    }

    #[test]
    fn checker_alternates_two_colors() {
        let a = [1.0, 0.0, 0.0, 1.0];
        let b = [0.0, 0.0, 1.0, 1.0];
        let img = checker(4, 4, a, b, 2, 2).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        // Cell (0,0) is color_a; the second column cell is color_b.
        assert_eq!(&img.pixels[0..4], &[255, 0, 0, 255]);
        assert_eq!(&img.pixels[8..12], &[0, 0, 255, 255]);
        assert!(checker(9000, 4, a, b, 2, 2).is_err());
    }

    #[test]
    fn brick_has_mortar_and_brick_texels() {
        let brick_c = [0.6, 0.2, 0.1, 1.0]; // -> [153, 51, 26, 255]
        let mortar_c = [0.8, 0.8, 0.8, 1.0]; // -> [204, 204, 204, 255]
        let img = brick(64, 64, brick_c, mortar_c, 4, 8, 0.1, 0.5).unwrap();
        assert_eq!((img.width, img.height), (64, 64));
        // y=0 sits in the horizontal mortar band (row_v < 0.1).
        assert_eq!(&img.pixels[0..4], &[204, 204, 204, 255]);
        assert!(
            img.pixels.as_chunks::<4>().0.contains(&[153, 51, 26, 255]),
            "brick texels present"
        );
        assert!(brick(0, 4, brick_c, mortar_c, 4, 8, 0.1, 0.5).is_err());
    }

    #[test]
    fn mix_multiply_with_black_is_black() {
        let img = swatch_2x2();
        let black = constant(2, 2, [0.0, 0.0, 0.0, 1.0]).unwrap();
        let out = mix(&img, &black, BlendMode::Multiply, 1.0);
        assert!(
            out.pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|c| c[0] == 0 && c[1] == 0 && c[2] == 0)
        );
    }

    #[test]
    fn pack_orm_places_channels() {
        let white = constant(2, 2, [1.0; 4]).unwrap();
        let out = pack_orm(None, Some(&white), None, 1.0, 0.5, 0.0);
        assert_eq!((out.width, out.height), (2, 2));
        // R = occlusion fallback 1.0, G = roughness map 1.0, B = metallic 0.
        assert_eq!(&out.pixels[0..4], &[255, 255, 0, 255]);
    }

    #[test]
    fn height_to_normal_flat_input_is_flat_normal() {
        let flat = constant(4, 4, [0.5, 0.5, 0.5, 1.0]).unwrap();
        let n = height_to_normal(&flat, 4.0);
        // (0.5, 0.5, 1.0) encoded.
        assert_eq!(&n.pixels[0..4], &[128, 128, 255, 255]);
    }

    #[test]
    fn clamp_to_edge_downscales_preserving_aspect() {
        let img = constant(400, 100, [1.0; 4]).unwrap();
        let capped = clamp_to_edge(&img, 200);
        assert_eq!((capped.width, capped.height), (200, 50));
        let unchanged = clamp_to_edge(&img, 512);
        assert_eq!((unchanged.width, unchanged.height), (400, 100));
    }

    #[test]
    fn blur_spreads_energy() {
        let mut px = vec![0u8; 8 * 8 * 4];
        // One white pixel in the middle, opaque alpha everywhere.
        for c in px.as_chunks_mut::<4>().0 {
            c[3] = 255;
        }
        let center = ((4 * 8 + 4) * 4) as usize;
        px[center] = 255;
        px[center + 1] = 255;
        px[center + 2] = 255;
        let img = RawImageData::new(px, 8, 8);
        let out = blur(&img, 3.0);
        let neighbor = ((4 * 8 + 5) * 4) as usize;
        assert!(out.pixels[neighbor] > 0, "energy spread to neighbours");
        assert!(out.pixels[center] < 255, "peak flattened");
    }
}
