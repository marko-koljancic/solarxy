//! Bakes the attribute-label glyph atlas: a single-channel SDF over the
//! numeric charset the GPU label channel draws (digits, punctuation, and
//! the NaN/Infinity letters), rasterized from the same Lilex face the
//! desktop GUI bundles. The committed blob is
//! `src/shaders/label_atlas.r8`; regenerate it with:
//!
//! ```bash
//! cargo run -p solarxy-renderer --example gen_glyph_atlas -- \
//!     crates/solarxy-renderer/src/shaders/label_atlas.r8
//! ```
//!
//! The cell layout and charset order are the contract with
//! `solarxy_renderer::labels` (index = position in [`CHARSET`]); the
//! metrics printed at the end are pinned there as consts. One SDF bake
//! plus fwidth-based AA in the shader covers the whole 9px-to-27px
//! on-screen range, so there is exactly one size.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

// The contract with labels.rs; keep byte-for-byte identical.
const CHARSET: &str = "0123456789.,-: e+NaInfity";
const CELL_W: usize = 48;
const CELL_H: usize = 64;
const GRID_COLS: usize = 5;
const GRID_ROWS: usize = 5;
const ATLAS_W: usize = CELL_W * GRID_COLS;
const ATLAS_H: usize = CELL_H * GRID_ROWS;
/// Font size at bake time; shader metrics scale from it.
const EM_PX: f32 = 40.0;
/// Pen origin inside a cell (left padding, baseline from the cell top).
const PEN_X: f32 = 4.0;
const BASELINE_Y: f32 = 44.0;
/// SDF half-range in texels: distances clamp here, and the shader decodes
/// 0.5 as the outline.
const SPREAD: f32 = 6.0;

fn main() -> anyhow::Result<()> {
    let out_path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: gen_glyph_atlas <output.r8>"))?;

    let font_bytes = include_bytes!("../../../res/Lilex/static/Lilex-Medium.ttf");
    let font = FontRef::try_from_slice(font_bytes)?;
    let scale = PxScale::from(EM_PX);
    let scaled = font.as_scaled(scale);

    let mut atlas = vec![0u8; ATLAS_W * ATLAS_H];
    for (i, ch) in CHARSET.chars().enumerate() {
        let cell_x = (i % GRID_COLS) * CELL_W;
        let cell_y = (i / GRID_COLS) * CELL_H;

        // Rasterize coverage into the cell-local grid.
        let mut cov = vec![0.0f32; CELL_W * CELL_H];
        let glyph = font
            .glyph_id(ch)
            .with_scale_and_position(scale, ab_glyph::point(PEN_X, BASELINE_Y));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|x, y, c| {
                let px = bounds.min.x as i32 + x as i32;
                let py = bounds.min.y as i32 + y as i32;
                if (0..CELL_W as i32).contains(&px) && (0..CELL_H as i32).contains(&py) {
                    cov[py as usize * CELL_W + px as usize] = c;
                }
            });
        }

        // Signed distance per texel: partial-coverage texels sit on the
        // outline and use their coverage directly as a subtexel offset;
        // solid texels search a small window for the nearest opposite
        // texel. The window only needs to reach the clamp radius.
        let window = SPREAD as i32 + 2;
        for ty in 0..CELL_H {
            for tx in 0..CELL_W {
                let c = cov[ty * CELL_W + tx];
                let signed = if c > 0.01 && c < 0.99 {
                    c - 0.5
                } else {
                    let inside = c >= 0.5;
                    let mut best = SPREAD * SPREAD;
                    for wy in -window..=window {
                        for wx in -window..=window {
                            let nx = tx as i32 + wx;
                            let ny = ty as i32 + wy;
                            if !(0..CELL_W as i32).contains(&nx)
                                || !(0..CELL_H as i32).contains(&ny)
                            {
                                continue;
                            }
                            let n = cov[ny as usize * CELL_W + nx as usize];
                            if (n >= 0.5) != inside {
                                let d2 = (wx * wx + wy * wy) as f32;
                                if d2 < best {
                                    best = d2;
                                }
                            }
                        }
                    }
                    let d = (best.sqrt() - 0.5).max(0.0);
                    if inside { d } else { -d }
                };
                let norm = (signed / SPREAD).clamp(-1.0, 1.0) * 0.5 + 0.5;
                atlas[(cell_y + ty) * ATLAS_W + (cell_x + tx)] = (norm * 255.0).round() as u8;
            }
        }
    }

    std::fs::write(&out_path, &atlas)?;

    let advance = scaled.h_advance(font.glyph_id('0'));
    println!(
        "wrote {out_path}: {ATLAS_W}x{ATLAS_H} R8, {} bytes",
        atlas.len()
    );
    println!("charset: {CHARSET:?} ({} glyphs)", CHARSET.chars().count());
    println!("advance ratio (advance/em): {:.6}", advance / EM_PX);
    Ok(())
}
