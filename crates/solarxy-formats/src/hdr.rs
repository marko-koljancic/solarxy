//! High-dynamic-range image decode: Radiance `.hdr` and `OpenEXR` `.exr`
//! into [`RawImageHdr`].
//!
//! These decoders lived inside the renderer's IBL module and were reachable
//! only from the lighting path. They are format loaders like every other
//! module here, so any consumer can now read a float image: the environment
//! node, the scene file, and the lighting path alike.
//!
//! HDRIs arrive from the same untrusted places models do, so both decoders
//! report diagnostics rather than panicking, and neither trusts a
//! dimension or a channel count it was handed.

use crate::{FormatsError, RawImageHdr};

/// The two high-dynamic-range containers this crate reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HdrContainer {
    /// Radiance RGBE, conventionally `.hdr`.
    Radiance,
    /// `OpenEXR`, conventionally `.exr`.
    OpenExr,
}

impl HdrContainer {
    /// Resolve a (lowercase, dot-free) extension, or `None` for anything
    /// this crate does not read.
    fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "hdr" => Some(Self::Radiance),
            "exr" => Some(Self::OpenExr),
            _ => None,
        }
    }

    /// Sniff the container magic. The `.slxy` reload path retains only the
    /// content-addressed bytes and has no filename left to read an
    /// extension from, so identity has to come out of the bytes.
    fn from_magic(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"#?") {
            Some(Self::Radiance)
        } else if bytes.starts_with(&[0x76, 0x2f, 0x31, 0x01]) {
            Some(Self::OpenExr)
        } else {
            None
        }
    }
}

/// Decode a high-dynamic-range image, dispatching on the (lowercase,
/// dot-free) extension exactly as [`crate::load_model_bytes`] does for
/// models. **An empty `ext` sniffs the container magic instead**; a
/// non-empty one is taken at its word, so a mislabeled file is an error
/// rather than a silent reinterpretation.
pub fn decode_hdr_image_bytes(bytes: &[u8], ext: &str) -> Result<RawImageHdr, FormatsError> {
    let container = if ext.is_empty() {
        HdrContainer::from_magic(bytes).ok_or_else(|| {
            FormatsError::Invalid(
                "unrecognized HDRI container: the bytes are neither Radiance .hdr nor OpenEXR"
                    .to_string(),
            )
        })?
    } else {
        HdrContainer::from_extension(ext)
            .ok_or_else(|| FormatsError::Invalid(format!("unsupported HDRI format: .{ext}")))?
    };

    match container {
        HdrContainer::Radiance => decode_hdr_bytes(bytes),
        HdrContainer::OpenExr => decode_exr_bytes(bytes),
    }
}

/// Decode Radiance `.hdr` bytes.
pub fn decode_hdr_bytes(bytes: &[u8]) -> Result<RawImageHdr, FormatsError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| FormatsError::Invalid(format!("HDR decode failed: {e}")))?;
    // `Rgb32FImage`'s backing container is already the flat row-major RGB
    // buffer `RawImageHdr` wants, so this hands the allocation straight
    // over rather than reshaping it.
    let rgb32f = img.to_rgb32f();
    let (width, height) = (rgb32f.width(), rgb32f.height());
    Ok(RawImageHdr::new(rgb32f.into_raw(), width, height))
}

/// Decode `OpenEXR` `.exr` bytes.
pub fn decode_exr_bytes(bytes: &[u8]) -> Result<RawImageHdr, FormatsError> {
    use exr::prelude::{ReadChannels, ReadLayers};

    // The set-pixel closure only receives the absolute pixel position, so
    // the row stride (image width) is carried in the pixel-storage tuple.
    // `Vec2::width()` aliases `.x()` in the `exr` crate — indexing the
    // buffer with it instead of the real width scrambles every row but the
    // first, which is the historical "broken EXR background" bug.
    // This is the same reader chain as the crate's
    // `read_first_rgba_layer_from_file` convenience, fed from a buffer.
    let image = exr::prelude::read()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            |resolution, _| {
                let width = resolution.width();
                (width, vec![0.0_f32; width * resolution.height() * 3])
            },
            |(width, pixels): &mut (usize, Vec<f32>), pos, (r, g, b, _a): (f32, f32, f32, f32)| {
                let o = (pos.y() * *width + pos.x()) * 3;
                pixels[o] = r;
                pixels[o + 1] = g;
                pixels[o + 2] = b;
            },
        )
        .first_valid_layer()
        .all_attributes()
        .from_buffered(std::io::Cursor::new(bytes))
        .map_err(|e| FormatsError::Invalid(format!("EXR decode failed: {e}")))?;

    let width = image.layer_data.size.width() as u32;
    let height = image.layer_data.size.height() as u32;
    let (_, pixels) = image.layer_data.channel_data.pixels;
    Ok(RawImageHdr::new(pixels, width, height))
}

/// Load a high-dynamic-range image from a filesystem path, dispatching on
/// its extension. The byte-first entry points above carry the wasm target,
/// which has no filesystem.
///
/// Takes a `Path` rather than the `&str` the model loaders take, because
/// the caller (the desktop HDRI picker) holds a `PathBuf` from a native
/// file dialog. Converting it to a string to open it would go through
/// `Path::display`, which is lossy, and a path that does not round-trip
/// through UTF-8 would stop opening. The error message is allowed to be
/// lossy; the `open` call is not.
#[cfg(feature = "std-fs")]
pub fn load_hdr_image(path: &std::path::Path) -> Result<RawImageHdr, FormatsError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = std::fs::read(path).map_err(|source| FormatsError::Io {
        path: path.display().to_string(),
        source,
    })?;
    decode_hdr_image_bytes(&bytes, &ext)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values decoded from a flat fixture

    use super::*;

    /// A tiny synthetic 4x2 Radiance HDR file (RLE-free), enough to
    /// exercise decode deterministically without committing a fixture.
    fn tiny_hdr() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n");
        out.extend_from_slice(b"-Y 2 +X 4\n");
        // 8 flat RGBE pixels (r=128, g=64, b=32, e=128 -> mid grey-ish).
        for _ in 0..8 {
            out.extend_from_slice(&[128, 64, 32, 128]);
        }
        out
    }

    /// Write an EXR where each pixel encodes its own coordinates, and hand
    /// back the bytes. `tag` names the scratch file: the test binary runs
    /// its tests in parallel threads sharing one temp directory, so two
    /// tests asking for the same dimensions would otherwise race on one
    /// path and one would read a file the other had already deleted.
    fn coordinate_exr(tag: &str, w: usize, h: usize) -> Vec<u8> {
        let path = std::env::temp_dir().join(format!("solarxy_hdr_{tag}_{w}x{h}.exr"));
        exr::prelude::write_rgba_file(&path, w, h, |x, y| (x as f32, y as f32, 0.5_f32, 1.0_f32))
            .expect("write test exr");
        let bytes = std::fs::read(&path).expect("read test exr");
        let _ = std::fs::remove_file(&path);
        bytes
    }

    #[test]
    fn hdr_decode_produces_three_floats_per_pixel() {
        let img = decode_hdr_bytes(&tiny_hdr()).expect("decode");
        assert_eq!((img.width, img.height), (4, 2));
        assert_eq!(img.pixels.len(), 4 * 2 * 3);
        // The fixture is flat, so every pixel carries the same triple.
        let first = &img.pixels[..3];
        for px in img.pixels.as_chunks::<3>().0 {
            assert_eq!(px, first);
        }
    }

    #[test]
    fn hdr_decode_is_deterministic_and_stamps_a_stable_hash() {
        // The hash is a texture-cache and dedup key, so decoding the same
        // bytes twice has to produce the same identity.
        let a = decode_hdr_bytes(&tiny_hdr()).expect("decode");
        let b = decode_hdr_bytes(&tiny_hdr()).expect("decode");
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.pixels, b.pixels);
        assert_eq!(
            a.hash,
            RawImageHdr::content_hash(&a.pixels, a.width, a.height)
        );
    }

    #[test]
    fn exr_decode_preserves_row_layout() {
        // A 4x3 EXR where each pixel encodes its own coordinates. A
        // mis-strided decode (the historical `pos.width()` bug) scrambles
        // every row but the first, so this catches a regression.
        let (w, h) = (4usize, 3usize);
        let img = decode_exr_bytes(&coordinate_exr("rowlayout", w, h)).expect("decode test exr");

        assert_eq!((img.width, img.height), (w as u32, h as u32));
        assert_eq!(img.pixels.len(), w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let px = &img.pixels[(y * w + x) * 3..][..3];
                assert!((px[0] - x as f32).abs() < 1e-3, "x mismatch at ({x},{y})");
                assert!((px[1] - y as f32).abs() < 1e-3, "y mismatch at ({x},{y})");
            }
        }
    }

    #[test]
    fn dispatch_honors_the_extension_it_was_given() {
        let by_ext = decode_hdr_image_bytes(&tiny_hdr(), "hdr").expect("decode");
        assert_eq!((by_ext.width, by_ext.height), (4, 2));

        let exr_bytes = coordinate_exr("by-extension", 2, 2);
        let by_ext = decode_hdr_image_bytes(&exr_bytes, "exr").expect("decode");
        assert_eq!((by_ext.width, by_ext.height), (2, 2));
    }

    #[test]
    fn an_empty_extension_sniffs_the_container_magic() {
        // The scene-file reload path keeps content-addressed bytes with no
        // filename, so identity has to come out of the header.
        let sniffed = decode_hdr_image_bytes(&tiny_hdr(), "").expect("sniff hdr");
        assert_eq!((sniffed.width, sniffed.height), (4, 2));

        let sniffed =
            decode_hdr_image_bytes(&coordinate_exr("by-magic", 2, 2), "").expect("sniff exr");
        assert_eq!((sniffed.width, sniffed.height), (2, 2));
    }

    #[test]
    fn unreadable_input_is_a_diagnostic_and_never_a_panic() {
        // A named format this crate does not read.
        let err = decode_hdr_image_bytes(&[0u8; 16], "png").expect_err("unsupported");
        assert!(err.to_string().contains("png"), "{err}");

        // Bytes that match no container magic.
        let err = decode_hdr_image_bytes(&[0u8; 16], "").expect_err("unrecognized");
        assert!(err.to_string().contains("unrecognized"), "{err}");

        // The right extension over bytes that are not that format.
        assert!(decode_hdr_bytes(&[0u8; 16]).is_err());
        assert!(decode_exr_bytes(&[0u8; 16]).is_err());

        // Truncated mid-header, the shape a partial upload takes.
        let truncated = &tiny_hdr()[..12];
        assert!(decode_hdr_image_bytes(truncated, "hdr").is_err());
    }
}
