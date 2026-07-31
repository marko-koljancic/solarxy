//! The `attribute_from_image` modifier: samples an Image input through
//! the mesh UVs into a point-domain attribute lane. The bridge from the
//! texture context onto geometry data: image color into `color` for
//! vertex display, or a channel into a float lane driving `displace`.

use solarxy_imaging::sample::{Filter, WrapMode, sample};

use super::common::{
    geometry_output, params_with, warn_input_lane_type_replaced, warn_reserved_lane_mismatch,
};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::engine::attr_table::resolve_lane;
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};
use solarxy_core::geometry::srgb_to_linear;
use solarxy_kernel::{AttributeData, KernelMesh};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "attribute_from_image",
        version: 1,
        display_name: "Attribute from Image",
        category: Category::Attribute,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry the sampled lane is written onto."),
            PortSpec::single("image", "Image", DataType::Image, false).doc(
                "The image to sample. Wire a `tex_ref` (pointing at a \
                 texture network) or an `import_image`. Unwired, the \
                 geometry passes through with a warning.",
            ),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Attribute from Image",
            vec![
                ParamSpec::new(
                    "attr_name",
                    "Name",
                    "attribute",
                    ParamType::Text,
                    ParamValue::Text("color".into()),
                )
                .doc(
                    "The lane the samples are written under. The default \
                     `color` displays as vertex color immediately; any other \
                     name is free-form data for downstream nodes.",
                ),
                ParamSpec::new(
                    "uv_attr",
                    "UV Attribute",
                    "attribute",
                    ParamType::AttributeName,
                    ParamValue::Text("uv".into()),
                )
                .doc(
                    "The vec2 point lane supplying sample coordinates; `uv` \
                     resolves the mesh's texture coordinates. A mesh without \
                     it passes through with a warning (chain `uv_project` \
                     first).",
                ),
                ParamSpec::new(
                    "channels",
                    "Channels",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("rgba", "RGBA (vec4)"),
                            EnumVariant::new("rgb", "RGB (vec3)"),
                            EnumVariant::new("luminance", "Luminance (float)"),
                            EnumVariant::new("r", "Red (float)"),
                            EnumVariant::new("g", "Green (float)"),
                            EnumVariant::new("b", "Blue (float)"),
                            EnumVariant::new("a", "Alpha (float)"),
                        ],
                    },
                    ParamValue::Enum("rgba".into()),
                )
                .doc(
                    "What lands in the lane: the full color (vec4, the \
                     `color` contract), RGB (vec3), or one scalar channel. \
                     Luminance uses the Rec. 709 weights.",
                ),
                ParamSpec::new(
                    "filter",
                    "Filter",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("bilinear", "Bilinear"),
                            EnumVariant::new("nearest", "Nearest"),
                        ],
                    },
                    ParamValue::Enum("bilinear".into()),
                )
                .doc("Blend the four surrounding texels, or read the nearest one."),
                ParamSpec::new(
                    "wrap",
                    "Wrap",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("repeat", "Repeat"),
                            EnumVariant::new("clamp", "Clamp"),
                        ],
                    },
                    ParamValue::Enum("repeat".into()),
                )
                .doc("How UVs outside 0..1 resolve: tile the image, or extend its edges."),
                ParamSpec::new(
                    "srgb",
                    "Interpret as sRGB",
                    "attribute",
                    ParamType::Bool,
                    ParamValue::Bool(true),
                )
                .doc(
                    "Convert the sampled RGB to linear before writing (alpha \
                     is untouched). Keep it on for color images: the reserved \
                     `color` lane is linear RGBA by contract. Turn it off for \
                     data maps (height, masks) whose bytes are already \
                     linear.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Samples the connected image through each point's UV and writes \
              the result into an attribute lane: full RGBA into `color` for \
              vertex-color display, or one channel into a float lane.\n\n\
              This is the image-to-geometry bridge: build a map in a texture \
              network, `tex_ref` it into the geometry graph, sample it here, \
              and drive `displace` (or anything else that reads lanes) with \
              the result. Sampling matches the renderer's orientation \
              exactly, so the written colors line up with the same image \
              textured on the surface.\n\n\
              Meshes without the UV lane, or an unwired image, pass through \
              with a warning.",
        search_aliases: &["sample", "map", "texture", "image", "bake", "vertex color"],
        glyph: "attribute_from_image",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let input = &super::common::baked_input(input, cx)?;
    let Some(image) = inputs.image("image") else {
        cx.warn("attribute_from_image has no image wired; the input passes through unchanged");
        return Ok(CookOutcome::Done(Outputs::geometry((**input).clone())));
    };
    let name = p.text("attr_name").trim().to_string();
    if name.is_empty() {
        cx.warn("attribute_from_image has no attribute name; the input passes through unchanged");
        return Ok(CookOutcome::Done(Outputs::geometry((**input).clone())));
    }
    let uv_name = p.text("uv_attr").trim().to_string();
    let channels = p.enum_key("channels").to_string();
    let filter = if p.enum_key("filter") == "nearest" {
        Filter::Nearest
    } else {
        Filter::Bilinear
    };
    let wrap = if p.enum_key("wrap") == "clamp" {
        WrapMode::Clamp
    } else {
        WrapMode::Repeat
    };
    let srgb = p.bool("srgb");

    let linearize = |px: [f32; 4]| -> [f32; 4] {
        if srgb {
            [
                srgb_to_linear(px[0]),
                srgb_to_linear(px[1]),
                srgb_to_linear(px[2]),
                px[3],
            ]
        } else {
            px
        }
    };

    let mut missing_uv = false;
    let meshes: Vec<KernelMesh> = input
        .meshes
        .iter()
        .map(|mesh| {
            let mut out = mesh.clone();
            let Some(uv_lane) = resolve_lane(mesh, &uv_name).filter(|l| l.ty() == "vec2") else {
                missing_uv = true;
                return out;
            };
            let count = mesh.positions.len();
            let sample_at = |i: usize| -> [f32; 4] {
                let (u, v) = match uv_lane.components(i) {
                    Some(c) if c.len() >= 2 => (c[0], c[1]),
                    _ => (0.0, 0.0),
                };
                linearize(sample(image, u, v, filter, wrap))
            };
            let lane = match channels.as_str() {
                "rgb" => AttributeData::Vec3(std::sync::Arc::new(
                    (0..count)
                        .map(|i| {
                            let px = sample_at(i);
                            [px[0], px[1], px[2]]
                        })
                        .collect(),
                )),
                "luminance" => AttributeData::Float(std::sync::Arc::new(
                    (0..count)
                        .map(|i| {
                            let px = sample_at(i);
                            0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2]
                        })
                        .collect(),
                )),
                c @ ("r" | "g" | "b" | "a") => {
                    let idx = match c {
                        "r" => 0,
                        "g" => 1,
                        "b" => 2,
                        _ => 3,
                    };
                    AttributeData::Float(std::sync::Arc::new(
                        (0..count).map(|i| sample_at(i)[idx]).collect(),
                    ))
                }
                _ => AttributeData::Vec4(std::sync::Arc::new((0..count).map(sample_at).collect())),
            };
            out.attributes.insert(name.clone(), lane);
            out
        })
        .collect();

    let written_ty = match channels.as_str() {
        "rgba" => "vec4",
        "rgb" => "vec3",
        _ => "float",
    };
    warn_reserved_lane_mismatch(cx, &name, written_ty);
    warn_input_lane_type_replaced(cx, input, &name, written_ty);
    if missing_uv {
        cx.warn(format!(
            "no vec2 `{uv_name}` lane on at least one mesh; those meshes \
             pass through unsampled (chain uv_project first)"
        ));
    }
    Ok(CookOutcome::Done(Outputs::geometry(
        solarxy_kernel::GeometrySet::from_parts(meshes, input.materials.clone()),
    )))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_core::RawImageData;
    use solarxy_kernel::primitives::generate_plane;
    use solarxy_kernel::{GeometrySet, reserved};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// 1 wide, 2 tall: opaque white on top, opaque black below.
    fn test_image() -> RawImageData {
        RawImageData::new(vec![255, 255, 255, 255, 0, 0, 0, 255], 1, 2)
    }

    fn run(
        stored: BTreeMap<String, ParamSource>,
        set: GeometrySet,
        image: Option<RawImageData>,
    ) -> (Arc<GeometrySet>, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(set))),
        );
        if let Some(img) = image {
            slots.insert(
                "image".to_string(),
                InputSlot::Single(Value::Image(Arc::new(img))),
            );
        }
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        (Arc::clone(set), cx.take_warnings())
    }

    fn lit(v: ParamValue) -> ParamSource {
        ParamSource::Literal(v)
    }

    #[test]
    fn samples_white_and_black_by_uv_row_and_reaches_the_renderer() {
        // The plane's UVs span 0..1; its v = 1 corners sample the BOTTOM
        // (black) row and v = 0 corners the top (white) row, matching the
        // renderer's unflipped sampling. Nearest keeps texels exact, and
        // clamp keeps the v = 1.0 edge on the bottom row (repeat would
        // wrap it back to the top, exactly as a GPU sampler does).
        let mut stored = BTreeMap::new();
        stored.insert(
            "filter".to_string(),
            lit(ParamValue::Enum("nearest".into())),
        );
        stored.insert("wrap".to_string(), lit(ParamValue::Enum("clamp".into())));
        stored.insert("srgb".to_string(), lit(ParamValue::Bool(false)));
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let (out, warnings) = run(stored, set, Some(test_image()));
        assert!(warnings.is_empty(), "{warnings:?}");
        let mesh = &out.meshes[0];
        let Some(AttributeData::Vec4(lane)) = mesh.attributes.get(reserved::COLOR) else {
            panic!("vec4 color lane written");
        };
        let uvs = mesh.tex_coords.as_ref().unwrap();
        for (uv, px) in uvs.iter().zip(lane.iter()) {
            let expected = if uv[1] > 0.5 { 0.0 } else { 1.0 };
            assert_eq!(px[0], expected, "uv {uv:?} sampled {px:?}");
            assert_eq!(px[3], 1.0);
        }
        assert!(
            out.to_cooked().meshes[0].colors.is_some(),
            "the color lane crossed the renderer contract"
        );
    }

    #[test]
    fn srgb_linearizes_rgb_and_leaves_alpha() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "filter".to_string(),
            lit(ParamValue::Enum("nearest".into())),
        );
        // Mid-grey sRGB 128 linearizes to ~0.2158.
        let grey = RawImageData::new(vec![128, 128, 128, 128], 1, 1);
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let (out, _) = run(stored, set, Some(grey));
        let Some(AttributeData::Vec4(lane)) = out.meshes[0].attributes.get(reserved::COLOR) else {
            panic!("vec4 lane");
        };
        let px = lane[0];
        assert!((px[0] - 0.2158).abs() < 0.005, "linearized: {px:?}");
        assert!((px[3] - 128.0 / 255.0).abs() < 1e-6, "alpha untouched");
    }

    #[test]
    fn luminance_writes_a_float_lane() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            lit(ParamValue::Text("height".into())),
        );
        stored.insert(
            "channels".to_string(),
            lit(ParamValue::Enum("luminance".into())),
        );
        stored.insert("srgb".to_string(), lit(ParamValue::Bool(false)));
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let (out, warnings) = run(stored, set, Some(test_image()));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(matches!(
            out.meshes[0].attributes.get("height"),
            Some(AttributeData::Float(_))
        ));
    }

    #[test]
    fn an_unwired_image_passes_through_with_a_warning() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let (out, warnings) = run(BTreeMap::new(), set, None);
        assert!(out.meshes[0].attributes.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no image"), "{warnings:?}");
    }

    #[test]
    fn a_mesh_without_uvs_passes_through_with_a_warning() {
        let set = GeometrySet::from_mesh(solarxy_kernel::KernelMesh::points(
            "pts",
            vec![[0.0; 3], [1.0, 0.0, 0.0]],
        ));
        let (out, warnings) = run(BTreeMap::new(), set, Some(test_image()));
        assert!(out.meshes[0].attributes.is_empty());
        assert!(warnings.iter().any(|w| w.contains("uv")), "{warnings:?}");
    }
}
