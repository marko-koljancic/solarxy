//! `environment`: the scene's lighting environment as a node, so the HDRI
//! you light with lives in the document and survives a save and a reload.
//!
//! Root-context and wireless, exactly like the light and camera nodes: the
//! scene builder reads it off the node rather than passing a value down a
//! chain. Unlike them it does real cook work, because an HDRI has to be
//! decoded. Natively the cook decodes inline through the shared
//! `solarxy_formats::hdr` path; on the web it returns a pending
//! `DecodeHdrImage` job and the import worker decodes off-thread, with the
//! result landing under the per-node generation guard.
//!
//! The decoded image travels on the `CookCtx` environment side-channel
//! rather than as an output value, for the reason recorded there: `Value`
//! has no float-image variant and adding one would mean a new `DataType`,
//! which is a deliberate frontend change. An environment has no wire to
//! travel on anyway.

use std::sync::Arc;

use super::common::params_with;
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, JobRequest, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

/// `background` param keys, shared with the scene lowering so the string
/// exists once.
pub const BACKGROUND_KEEP: &str = "keep";
pub const BACKGROUND_HDRI_SKY: &str = "hdri_sky";

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "environment",
        version: 1,
        display_name: "Environment",
        category: Category::Lights,
        contexts: ContextSet::OBJ,
        opens: None,
        inputs: vec![],
        outputs: vec![],
        params: params_with(
            "Environment",
            vec![
                ParamSpec::new(
                    "hdri",
                    "HDRI",
                    "environment",
                    ParamType::AssetRef {
                        accept: [".hdr", ".exr"].iter().map(ToString::to_string).collect(),
                    },
                    ParamValue::Asset(crate::params::AssetId(String::new())),
                )
                .doc(
                    "The high-dynamic-range image that lights the scene: a \
                     Radiance `.hdr` or an `OpenEXR` `.exr`. It supplies \
                     both the ambient light and the reflections, so loading \
                     one changes the look of every material at once. \
                     Identity is the bytes' SHA-256 rather than the path, so \
                     a saved scene embeds a copy and still opens once the \
                     original file is gone. With no file staged the node \
                     asserts no environment at all, which leaves whatever \
                     background and procedural sky the viewport already had \
                     rather than going black.",
                ),
                ParamSpec::new(
                    "rotation",
                    "Rotation",
                    "environment",
                    ParamType::Float,
                    ParamValue::Float(0.0),
                )
                .doc(
                    "Spins the environment around the vertical axis, in \
                     degrees. Moves the visible sky and the lighting it \
                     casts together, so it is how you place a highlight \
                     without moving a light.",
                ),
                ParamSpec::new(
                    "intensity",
                    "Intensity",
                    "environment",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .doc(
                    "Scales how much light the environment casts, leaving \
                     the visible sky alone. Use it to keep a backdrop \
                     readable while dialling the key it throws up or down. \
                     1 is the image as it was authored.",
                ),
                ParamSpec::new(
                    "background",
                    "Background",
                    "environment",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new(BACKGROUND_KEEP, "Keep"),
                            EnumVariant::new(BACKGROUND_HDRI_SKY, "HDRI Sky"),
                        ],
                    },
                    ParamValue::Enum(BACKGROUND_KEEP.to_string()),
                )
                .doc(
                    "Whether the environment also claims the backdrop. \
                     **Keep** lights from the HDRI but leaves each \
                     viewport's own background alone, which is what you \
                     want for a product shot on white. **HDRI Sky** draws \
                     the image itself behind the scene. Solid and gradient \
                     backdrops stay per-viewport, so this never fights the \
                     background control on the pane toolbar.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "Makes the lighting environment part of the scene rather than \
              part of the application, so the HDRI you light with is saved \
              in the file and comes back when you reopen it.\n\n\
              Drop it in the root graph beside your `geo` containers. It \
              takes no wires: the scene builder reads it straight off the \
              node, the same way it reads the light and camera nodes. Point \
              it at a `.hdr` or `.exr`, then use Rotation to place the \
              highlights and Intensity to balance the environment against \
              your lights.\n\n\
              There is exactly one environment, so if a graph holds more \
              than one of these the first in document order wins and the \
              rest are ignored. An environment set here also takes \
              precedence over one loaded through the viewport's own HDRI \
              control, which stays available for scenes that have no \
              environment node.",
        search_aliases: &["hdri", "ibl", "sky", "lighting", "environment"],
        glyph: "hemisphere",
        role: NodeRole::Light,
        cook: cook_environment,
        migrate: None,
    }
}

fn cook_environment(
    p: &ResolvedParams,
    _in: &Inputs,
    cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    // No file staged: no environment. Deliberately not an error and not a
    // black image; the host keeps the background it already had.
    let Some(asset) = p.asset("hdri") else {
        return Ok(CookOutcome::Done(Outputs::empty()));
    };

    if cx.async_jobs {
        return Ok(CookOutcome::Pending(JobRequest::DecodeHdrImage {
            asset: asset.clone(),
        }));
    }

    let entry = cx.assets.get(asset).ok_or_else(|| CookError::Failed {
        message: "referenced asset is not staged".to_string(),
    })?;
    // An empty extension sniffs the container magic, which is the case a
    // scene-file reload lands in: it retains content-addressed bytes with
    // no filename to read an extension from.
    let ext = std::path::Path::new(&entry.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let image = solarxy_formats::hdr::decode_hdr_image_bytes(&entry.bytes, &ext).map_err(|e| {
        CookError::Failed {
            message: e.to_string(),
        }
    })?;
    cx.set_environment(Arc::new(image));
    Ok(CookOutcome::Done(Outputs::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetTable;

    /// A tiny synthetic 4x2 Radiance HDR file (RLE-free), the same fixture
    /// shape the formats decoder tests use.
    fn tiny_hdr() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n");
        out.extend_from_slice(b"-Y 2 +X 4\n");
        for _ in 0..8 {
            out.extend_from_slice(&[128, 64, 32, 128]);
        }
        out
    }

    fn resolved_with_hdri(asset_id: &crate::params::AssetId) -> ResolvedParams {
        let mut stored = std::collections::BTreeMap::new();
        stored.insert(
            "hdri".to_string(),
            crate::params::ParamSource::Literal(ParamValue::Asset(asset_id.clone())),
        );
        crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap()
    }

    fn resolved_empty() -> ResolvedParams {
        crate::registry::resolve::resolve_params(
            &std::collections::BTreeMap::new(),
            &descriptor().params,
        )
        .unwrap()
    }

    #[test]
    fn no_staged_file_is_no_environment_rather_than_an_error() {
        // The empty state has to be quiet: an environment node someone
        // dropped but has not pointed anywhere yet must not badge an error
        // and must not black out the viewport.
        let assets = AssetTable::default();
        let mut cx = CookCtx::new(&assets, false);
        let out = cook_environment(&resolved_empty(), &Inputs::default(), &mut cx)
            .expect("empty is not an error");
        assert!(matches!(out, CookOutcome::Done(_)));
        assert!(cx.take_environment().is_none());
    }

    #[test]
    fn a_staged_hdri_decodes_inline_on_the_native_path() {
        let mut assets = AssetTable::default();
        let id = assets.stage("sky.hdr", "image/vnd.radiance", tiny_hdr());
        let mut cx = CookCtx::new(&assets, false);
        let out = cook_environment(&resolved_with_hdri(&id), &Inputs::default(), &mut cx)
            .expect("decode");
        assert!(matches!(out, CookOutcome::Done(_)));
        let image = cx
            .take_environment()
            .expect("environment on the side-channel");
        assert_eq!((image.width, image.height), (4, 2));
        assert_eq!(image.pixels.len(), 4 * 2 * 3);
    }

    #[test]
    fn the_web_path_defers_to_a_job_instead_of_decoding_inline() {
        // Decoding a 4K equirect on the browser's main thread would stall
        // the frame; the worker does it.
        let mut assets = AssetTable::default();
        let id = assets.stage("sky.hdr", "image/vnd.radiance", tiny_hdr());
        let mut cx = CookCtx::new(&assets, true);
        let out =
            cook_environment(&resolved_with_hdri(&id), &Inputs::default(), &mut cx).expect("job");
        match out {
            CookOutcome::Pending(JobRequest::DecodeHdrImage { asset }) => {
                assert_eq!(asset, id);
            }
            other => panic!("expected a decode job, got {other:?}"),
        }
        assert!(cx.take_environment().is_none());
    }

    #[test]
    fn an_unstaged_reference_fails_with_a_diagnostic() {
        let assets = AssetTable::default();
        let mut cx = CookCtx::new(&assets, false);
        let missing = crate::params::AssetId("deadbeef".to_string());
        let err = cook_environment(&resolved_with_hdri(&missing), &Inputs::default(), &mut cx)
            .expect_err("dangling reference");
        assert!(format!("{err:?}").contains("not staged"), "{err:?}");
    }

    #[test]
    fn a_file_that_is_not_an_hdri_is_a_diagnostic_and_not_a_panic() {
        // Staged assets are user files and the accept list is advisory, so
        // the decoder is untrusted-input territory.
        let mut assets = AssetTable::default();
        let id = assets.stage("sky.hdr", "image/vnd.radiance", vec![0u8; 32]);
        let mut cx = CookCtx::new(&assets, false);
        assert!(cook_environment(&resolved_with_hdri(&id), &Inputs::default(), &mut cx).is_err());
    }
}
