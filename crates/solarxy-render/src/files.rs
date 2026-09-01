//! What a finished render becomes on disk.
//!
//! One beauty image, plus a sibling file per auxiliary pass that was asked
//! for. Siblings rather than one multilayer file because a sibling is what
//! every compositing package ingests without being told how, and because
//! multilayer brings a channel-naming contract this release does not need to
//! settle.
//!
//! # Why the siblings are always float
//!
//! The beauty follows the path the caller named: eight-bit PNG unless it ends
//! in `.exr`. The passes do not. An eight-bit albedo is a picture of an albedo
//! and an eight-bit normal is useless, so a pass is written as 32-bit float
//! whatever the beauty is, and a run that asks for passes beside a PNG gets
//! exactly that.

use std::path::{Path, PathBuf};

use solarxy_core::geometry::RawImageHdr;

use crate::RenderError;

/// The pass vocabulary and the readers for the planes it names.
///
/// Defined in the shared crate rather than here, because a browser has to name
/// the same passes and read the same planes, and this crate does not build for
/// the web. Re-exported so a caller that only ever writes files keeps reaching
/// for them where the writing lives.
pub use solarxy_host::passes::{AovKind, albedo_from_auxiliary, floats_of, normal_from_auxiliary};

/// Which space a float beauty is written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExrSpace {
    /// Scene-referred light, with no exposure, tone map or grade applied. What
    /// a compositing package wants, because it has not decided anything yet.
    #[default]
    SceneLinear,
    /// The finished look in floating point: the image the screen would show,
    /// without the quantization.
    Display,
}

/// Whether a path names a file this writes as EXR.
#[must_use]
pub fn is_exr(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exr"))
}

/// Where a pass goes, given where the image went.
///
/// `beauty.exr` and `beauty.png` both put albedo at `beauty.albedo.exr`: the
/// stem is what identifies the shot, and a set of passes that changed names
/// with the beauty's format would be a set nobody could glob.
#[must_use]
pub fn sibling(image: &Path, kind: AovKind) -> PathBuf {
    let stem = image.file_stem().map_or_else(
        || "render".to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    image.with_file_name(format!("{stem}.{}.exr", kind.as_str()))
}

/// Drops the alpha lane of an RGBA float readback.
///
/// A still has its background already in it, so the lane is a constant one
/// pretending to be a matte. Writing it would invite a compositor to key on it.
#[must_use]
pub fn rgb_from_rgba(pixels: &[f32]) -> Vec<f32> {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect()
}

/// Encodes one pass and writes it beside the image.
///
/// # Errors
/// The encode or the write failing.
pub fn write_pass(
    path: &Path,
    kind: AovKind,
    plane: &[f32],
    width: u32,
    height: u32,
) -> Result<(), RenderError> {
    let bytes = match kind {
        AovKind::Depth => solarxy_formats::export::encode_exr_depth_bytes(plane, width, height)?,
        AovKind::Albedo | AovKind::Normal => solarxy_formats::export::encode_exr_rgb_bytes(
            &RawImageHdr::new(plane.to_vec(), width, height),
        )?,
    };
    std::fs::write(path, bytes).map_err(|source| RenderError::OutputUnwritable {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pass_lands_beside_the_image_whatever_the_image_is() {
        let png = sibling(Path::new("/shots/beauty.png"), AovKind::Albedo);
        assert_eq!(png, Path::new("/shots/beauty.albedo.exr"));
        let exr = sibling(Path::new("/shots/beauty.exr"), AovKind::Albedo);
        assert_eq!(exr, Path::new("/shots/beauty.albedo.exr"));
        assert_eq!(
            sibling(Path::new("beauty.exr"), AovKind::Depth),
            Path::new("beauty.depth.exr")
        );
    }

    #[test]
    fn only_an_exr_extension_reads_as_one() {
        assert!(is_exr(Path::new("a.exr")));
        assert!(is_exr(Path::new("a.EXR")));
        assert!(!is_exr(Path::new("a.png")));
        assert!(!is_exr(Path::new("exr")));
    }

    /// The two passes come out of one store, and they read different lanes.
    ///
    /// The fourth lane is packed rather than stored, so a reader that forgets
    /// to unpack it gets a plausible number rather than an error. What is
    /// asserted here is that it went through the unpacker at all: the result is
    /// a unit vector, which a raw lane value is not. That the unpacker agrees
    /// with the shader's packer is pinned on the GPU, in the renderer's own
    /// camera test, which is the only place both halves exist.
    #[test]
    fn the_normal_lane_is_unpacked_and_the_albedo_lane_is_not() {
        let aux = vec![0.25, 0.5, 0.75, 2_100_000.0, 0.1, 0.2, 0.3, 12.0];
        assert_eq!(
            albedo_from_auxiliary(&aux),
            vec![0.25, 0.5, 0.75, 0.1, 0.2, 0.3]
        );
        let normals = normal_from_auxiliary(&aux);
        assert_eq!(normals.len(), 6);
        for n in normals.as_chunks::<3>().0 {
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((length - 1.0).abs() < 1e-4, "not a direction: {n:?}");
        }
    }

    #[test]
    fn a_float_plane_reads_back_as_the_floats_that_were_written() {
        let mut bytes = Vec::new();
        for v in [1.5_f32, -2.0, 0.25, 8.0] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(floats_of(&bytes), vec![1.5, -2.0, 0.25, 8.0]);
        assert_eq!(rgb_from_rgba(&floats_of(&bytes)), vec![1.5, -2.0, 0.25]);
    }
}
