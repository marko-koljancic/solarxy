//! What happens to a still's pixels once they arrive: the tile blit, the
//! preview downscale, the floating-point write and the file name.
//!
//! Separated from the job because none of it needs the shell, which is what
//! lets the blit and the preview be tested directly.

use std::path::Path;

use solarxy_host::passes::AovKind;
use solarxy_host::still::{StillPasses, StillTile, TileRect};

use super::PREVIEW_MAX_EDGE;

/// One produced pass as the file it is saved as: always floating point,
/// whatever the beauty is, through the encoders the browser and the headless
/// command write the same pass with, so the three surfaces cannot write one
/// pass three ways.
pub(super) fn pass_exr_bytes(passes: &StillPasses, kind: AovKind) -> Result<Vec<u8>, String> {
    let plane = passes
        .plane(kind)
        .ok_or_else(|| format!("this render produced no {} pass", kind.as_str()))?;
    let (width, height) = (passes.width(), passes.height());
    match kind {
        AovKind::Depth => solarxy_formats::export::encode_exr_depth_bytes(&plane, width, height),
        AovKind::Albedo | AovKind::Normal => solarxy_formats::export::encode_exr_rgb_bytes(
            &solarxy_core::geometry::RawImageHdr::new(plane, width, height),
        ),
    }
    .map_err(|e| format!("the {} pass could not be encoded: {e}", kind.as_str()))
}

/// Writes every produced pass beside the picture, named as the command line
/// names them, and says how many it wrote.
pub(super) fn write_passes_beside(image: &Path, passes: &StillPasses) -> Result<usize, String> {
    let stem = image
        .file_stem()
        .map_or_else(|| "still".to_string(), |s| s.to_string_lossy().into_owned());
    let mut written = 0;
    for kind in [AovKind::Albedo, AovKind::Normal, AovKind::Depth] {
        if !passes.has(kind) {
            continue;
        }
        let bytes = pass_exr_bytes(passes, kind)?;
        let path = image.with_file_name(kind.sibling_name(&stem));
        std::fs::write(&path, bytes)
            .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
        written += 1;
    }
    Ok(written)
}

/// Suggested still file name, `still_<YYYYMMDD-HHMMSS>.png`, matching
/// the screenshot's stamp format.
pub(super) fn still_filename() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let stamp = now
        .format(&time::macros::format_description!(
            "[year][month][day]-[hour][minute][second]"
        ))
        .unwrap_or_default();
    // No extension: the dialog adds the one the chosen format decides, and a
    // name that said `.png` while EXR was selected would be a name arguing with
    // the file beside it.
    format!("still_{stamp}")
}

/// Writes the floating-point still, through the same encoder the browser and
/// the headless command write one with.
pub(super) fn write_exr(
    path: &std::path::Path,
    image: &solarxy_host::still::FloatImage,
) -> Result<(), String> {
    // A matte still goes through the four-channel writer, which premultiplies
    // on the way out; an opaque one keeps writing three channels, because its
    // alpha would be a constant one pretending to be a matte.
    let bytes = if image.has_matte() {
        solarxy_formats::export::encode_exr_rgba_bytes(image.rgba(), image.width(), image.height())
    } else {
        let hdr = solarxy_core::geometry::RawImageHdr::new(
            image.rgb().to_vec(),
            image.width(),
            image.height(),
        );
        solarxy_formats::export::encode_exr_rgb_bytes(&hdr)
    }
    .map_err(|e| format!("the image could not be encoded: {e}"))?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Copy one cropped tile into its place in the assembled picture.
pub(super) fn blit_tile(image: &mut [u8], image_width: u32, tile: &StillTile) {
    blit_rect(image, image_width, tile.rect, &tile.pixels);
}

/// Copies one rectangle of eight-bit pixels into the assembled picture.
///
/// Takes the rect and the bytes rather than a tile, because a mid-render
/// preview covers the same rectangle in the same format and there is no reason
/// for it to travel a second path to the same buffer.
pub(super) fn blit_rect(image: &mut [u8], image_width: u32, rect: TileRect, pixels: &[u8]) {
    let row = image_width as usize * 4;
    let tile_row = rect.width as usize * 4;
    for y in 0..rect.height as usize {
        let dst = (rect.y as usize + y) * row + rect.x as usize * 4;
        let src = y * tile_row;
        image[dst..dst + tile_row].copy_from_slice(&pixels[src..src + tile_row]);
    }
}

/// A nearest-neighbour downscale of the assembled picture for the modal's
/// live preview. Nearest, because it reads only preview-many pixels: a
/// box filter over a large still would cost more than the tile did.
pub(super) fn preview_of(image: &[u8], width: u32, height: u32) -> image::RgbaImage {
    let scale = (PREVIEW_MAX_EDGE as f32 / width.max(height) as f32).min(1.0);
    let pw = ((width as f32 * scale) as u32).max(1);
    let ph = ((height as f32 * scale) as u32).max(1);
    let mut out = image::RgbaImage::new(pw, ph);
    for y in 0..ph {
        let sy = (u64::from(y) * u64::from(height) / u64::from(ph)) as usize;
        for x in 0..pw {
            let sx = (u64::from(x) * u64::from(width) / u64::from(pw)) as usize;
            let i = (sy * width as usize + sx) * 4;
            out.put_pixel(
                x,
                y,
                image::Rgba([image[i], image[i + 1], image[i + 2], image[i + 3]]),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use solarxy_host::still::TileRect;

    use super::*;

    #[test]
    fn a_tile_lands_at_its_own_rect() {
        let mut image = vec![0u8; 4 * 4 * 4];
        let tile = StillTile {
            rect: TileRect {
                x: 2,
                y: 1,
                width: 2,
                height: 2,
            },
            pixels: vec![255u8; 2 * 2 * 4],
            aux: None,
            depth: None,
        };
        blit_tile(&mut image, 4, &tile);
        // Row 0 untouched, rows 1 and 2 filled from column 2.
        assert_eq!(&image[0..16], &[0u8; 16]);
        let row1 = &image[16..32];
        assert_eq!(&row1[0..8], &[0u8; 8]);
        assert_eq!(&row1[8..16], &[255u8; 8]);
    }

    #[test]
    fn the_preview_never_exceeds_its_edge_and_samples_corners() {
        let width = 100u32;
        let height = 50u32;
        let mut image = vec![0u8; width as usize * height as usize * 4];
        // Mark the bottom-right source pixel.
        let last = ((height as usize - 1) * width as usize + (width as usize - 1)) * 4;
        image[last] = 200;
        let p = preview_of(&image, width, height);
        assert!(p.width() <= PREVIEW_MAX_EDGE && p.height() <= PREVIEW_MAX_EDGE);
        assert_eq!(p.width(), 100, "small images pass through unscaled");
        assert_eq!(p.get_pixel(99, 49)[0], 200);
    }
}

#[cfg(test)]
mod pass_file_tests {
    use super::*;
    use solarxy_host::still::StillSpec;

    fn depth_only(width: u32, height: u32) -> StillPasses {
        let spec = StillSpec {
            width,
            height,
            aux: false,
            depth: true,
            ..StillSpec::default()
        };
        let mut passes = StillPasses::new(&spec).expect("depth was asked for");
        let depth: Vec<u8> = (0..width * height)
            .map(|i| i as f32)
            .flat_map(f32::to_le_bytes)
            .collect();
        passes.place(&StillTile {
            rect: TileRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            pixels: Vec::new(),
            aux: None,
            depth: Some(depth),
        });
        passes
    }

    /// A pass file is floating point whatever the beauty is, and a pass the
    /// render did not produce is refused rather than written black.
    #[test]
    fn a_pass_file_is_float_whatever_the_beauty_is() {
        let passes = depth_only(2, 2);
        let bytes = pass_exr_bytes(&passes, AovKind::Depth).expect("encodes");
        assert_eq!(
            &bytes[..4],
            &[0x76, 0x2f, 0x31, 0x01],
            "not an OpenEXR file"
        );
        assert!(pass_exr_bytes(&passes, AovKind::Albedo).is_err());
    }

    /// The siblings land beside the picture under the command line's names.
    #[test]
    fn passes_are_written_beside_the_picture_under_the_command_line_names() {
        let dir = std::env::temp_dir().join(format!("solarxy-passes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let image = dir.join("still_20260909.png");
        let written = write_passes_beside(&image, &depth_only(2, 2)).expect("writes");
        assert_eq!(written, 1);
        assert!(dir.join("still_20260909.depth.exr").exists());
        assert!(!dir.join("still_20260909.albedo.exr").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
