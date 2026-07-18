//! `import_image`: turns a staged encoded image (PNG, JPEG,
//! WebP) into a first-class `Image` wire value for the `material` node's
//! map ports.
//!
//! Source node: no inputs, one Image output, `Mute` bypass. Only the
//! `AssetRef` serializes into a document; decoded pixels are cook
//! artifacts.
//! Natively the cook decodes inline via the shared
//! `solarxy_formats::decode_image_bytes` path (the same decoder the model
//! loaders use); on the web it returns a pending `DecodeImage` job and the
//! import worker decodes via `createImageBitmap` off-thread, with the
//! result landing under the per-node generation guard. Decode failures
//! badge an ERROR while keep-last-good retains the previous image.

use std::sync::Arc;

use super::common::params_with;
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, JobRequest, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::{DataType, Value};
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "import_image",
        version: 1,
        display_name: "Import Image",
        category: Category::Import,
        // Placeable in geometry networks (material map wiring) AND in
        // texture networks (the image-op source).
        contexts: ContextSet::GEO.or(ContextSet::TEX),
        opens: None,
        inputs: vec![],
        outputs: vec![
            PortSpec::single("image", "Image", DataType::Image, false)
                .default_port()
                .doc(
                    "The decoded RGBA image. Empty until a file is staged, \
                     which a material map port reads as 'no map' rather than \
                     as a blank texture.",
                ),
        ],
        params: params_with(
            "Import Image",
            vec![
                ParamSpec::new(
                    "file",
                    "File",
                    "object",
                    ParamType::AssetRef {
                        accept: [".png", ".jpg", ".jpeg", ".webp"]
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                    },
                    ParamValue::Asset(crate::params::AssetId(String::new())),
                )
                .doc(
                    "The staged image file: PNG, JPEG, or WebP. Identity is \
                     the bytes' SHA-256, not the path, so staging the same \
                     file twice costs nothing and a saved `.slxy` embeds a \
                     copy -- the scene still opens once the original is gone. \
                     Only this reference is stored in the document; the \
                     decoded pixels are a cook artifact, rebuilt on load.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "Decodes a PNG, JPEG, or WebP file into an Image value that can \
              drive a material's map ports or feed a texture network.\n\n\
              It is the source end of both image workflows: wire it into a \
              `material` node's base colour, roughness, or normal port to \
              texture a surface, or drop it in a texture network as the input \
              an image operator chain works over. It is one of the few nodes \
              placeable in both geometry and texture networks, for exactly \
              that reason.\n\n\
              With no file staged the node emits no value at all, not a \
              placeholder pixel: a map port wired to it reads as unconnected, \
              so an empty import never silently drives a channel to black. On \
              the web the decode happens off the main thread in the import \
              worker via `createImageBitmap`; a failed decode badges the node \
              and the previous image stays live.",
        search_aliases: &["image", "texture", "png", "jpeg", "webp", "import"],
        glyph: "import_image",
        role: NodeRole::ImageSource,
        cook: cook_import_image,
        migrate: None,
    }
}

fn cook_import_image(
    p: &ResolvedParams,
    _in: &Inputs,
    cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    // No file staged yet: no output value at all. A downstream map port
    // connected to this node gathers an `Absent` slot, which is exactly
    // the "no map" semantics (never a placeholder pixel that would
    // silently drive a channel).
    let Some(asset) = p.asset("file") else {
        return Ok(CookOutcome::Done(Outputs::default()));
    };

    if cx.async_jobs {
        return Ok(CookOutcome::Pending(JobRequest::DecodeImage {
            asset: asset.clone(),
        }));
    }

    let entry = cx.assets.get(asset).ok_or_else(|| CookError::Failed {
        message: "referenced asset is not staged".to_string(),
    })?;
    let image =
        solarxy_formats::decode_image_bytes(&entry.bytes).map_err(|e| CookError::Failed {
            message: e.to_string(),
        })?;
    Ok(CookOutcome::Done(Outputs::single(
        "image",
        Value::Image(Arc::new(image)),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetTable;

    /// A valid 1x1 red PNG, byte-for-byte (the same construction as the
    /// formats fixtures).
    fn tiny_png() -> Vec<u8> {
        // Pre-built: signature + IHDR(1x1 RGBA8) + IDAT(zlib of one red
        // texel row) + IEND. Verified by the decode assertions below.
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99,
            0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    fn resolved_with_file(asset_id: &str) -> ResolvedParams {
        let mut stored = std::collections::BTreeMap::new();
        stored.insert(
            "file".to_string(),
            crate::params::ParamSource::Literal(ParamValue::Asset(crate::params::AssetId(
                asset_id.to_string(),
            ))),
        );
        crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap()
    }

    #[test]
    fn no_file_yields_no_output() {
        let assets = AssetTable::default();
        let resolved = crate::registry::resolve::resolve_params(
            &std::collections::BTreeMap::new(),
            &descriptor().params,
        )
        .unwrap();
        let mut cx = CookCtx::new(&assets, false);
        let outcome = cook_import_image(&resolved, &Inputs::default(), &mut cx).unwrap();
        let CookOutcome::Done(outputs) = outcome else {
            panic!("expected Done");
        };
        assert!(outputs.get("image").is_none(), "no placeholder image");
    }

    #[test]
    fn native_cook_decodes_inline() {
        let mut assets = AssetTable::default();
        let id = assets.stage("red.png".to_string(), "image/png".to_string(), tiny_png());
        let resolved = resolved_with_file(&id.0);
        let mut cx = CookCtx::new(&assets, false);
        let outcome = cook_import_image(&resolved, &Inputs::default(), &mut cx).unwrap();
        let CookOutcome::Done(outputs) = outcome else {
            panic!("expected Done");
        };
        let value = outputs.get("image").expect("image output");
        let img = value.as_image().expect("Image value");
        assert_eq!((img.width, img.height), (1, 1));
        assert_eq!(img.pixels, vec![255, 0, 0, 255]);
        assert_eq!(
            img.hash,
            solarxy_core::RawImageData::content_hash(&img.pixels, 1, 1)
        );
    }

    #[test]
    fn web_cook_returns_a_decode_job() {
        let mut assets = AssetTable::default();
        let id = assets.stage("red.png".to_string(), "image/png".to_string(), tiny_png());
        let resolved = resolved_with_file(&id.0);
        let mut cx = CookCtx::new(&assets, true);
        let outcome = cook_import_image(&resolved, &Inputs::default(), &mut cx).unwrap();
        assert!(matches!(
            outcome,
            CookOutcome::Pending(JobRequest::DecodeImage { asset }) if asset == id
        ));
    }

    #[test]
    fn undecodable_bytes_are_a_cook_error() {
        let mut assets = AssetTable::default();
        let id = assets.stage(
            "junk.png".to_string(),
            "image/png".to_string(),
            b"not an image".to_vec(),
        );
        let resolved = resolved_with_file(&id.0);
        let mut cx = CookCtx::new(&assets, false);
        assert!(matches!(
            cook_import_image(&resolved, &Inputs::default(), &mut cx),
            Err(CookError::Failed { .. })
        ));
    }
}
