//! The texture-context composite, filter, and PBR-utility nodes
//! (phase 19): `mix`, `blur`, `sharpen`, `pack_orm`, and
//! `height_to_normal`.

use super::common::params_with;
use super::image_support::{image_in, image_out, image_outputs, require_image, working_image};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

fn tex_node(
    type_id: &'static str,
    display_name: &'static str,
    inputs: Vec<PortSpec>,
    params: Vec<ParamSpec>,
    bypass: BypassBehavior,
    doc: &'static str,
    search_aliases: &'static [&'static str],
    role: NodeRole,
    cook: crate::cook::CookFn,
) -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id,
        version: 1,
        display_name,
        category: Category::Modifiers,
        contexts: ContextSet::TEX,
        opens: None,
        inputs,
        outputs: vec![image_out()],
        params: params_with(display_name, params),
        bypass,
        doc,
        search_aliases,
        glyph: type_id,
        role,
        cook,
        migrate: None,
    }
}

#[must_use]
pub fn mix_descriptor() -> NodeTypeDescriptor {
    tex_node(
        "mix",
        "Mix",
        vec![
            image_in(true),
            PortSpec::single("blend", "Blend", DataType::Image, false),
        ],
        vec![
            ParamSpec::new(
                "mode",
                "Mode",
                "mix",
                ParamType::Enum {
                    variants: vec![
                        EnumVariant::new("normal", "Normal"),
                        EnumVariant::new("over", "Over"),
                        EnumVariant::new("multiply", "Multiply"),
                        EnumVariant::new("add", "Add"),
                        EnumVariant::new("screen", "Screen"),
                        EnumVariant::new("overlay", "Overlay"),
                    ],
                },
                ParamValue::Enum("normal".to_string()),
            ),
            ParamSpec::new(
                "factor",
                "Factor",
                "mix",
                ParamType::Float,
                ParamValue::Float(1.0),
            )
            .hard(0.0, 1.0)
            .step(0.01),
        ],
        BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        "Blends the second image onto the first (the output keeps the first input's size).",
        &["mix", "blend", "composite", "over", "multiply", "screen"],
        NodeRole::Gather,
        cook_mix,
    )
}

fn cook_mix(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let a = require_image(inputs)?;
    // An unconnected blend input passes the base through untouched.
    let Some(b) = working_image(inputs, "blend") else {
        return Ok(CookOutcome::Done(image_outputs((*a).clone())));
    };
    let mode = match p.enum_key("mode") {
        "over" => solarxy_imaging::BlendMode::Over,
        "multiply" => solarxy_imaging::BlendMode::Multiply,
        "add" => solarxy_imaging::BlendMode::Add,
        "screen" => solarxy_imaging::BlendMode::Screen,
        "overlay" => solarxy_imaging::BlendMode::Overlay,
        _ => solarxy_imaging::BlendMode::Normal,
    };
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::mix(
        &a,
        &b,
        mode,
        p.f32("factor"),
    ))))
}

#[must_use]
pub fn blur_descriptor() -> NodeTypeDescriptor {
    tex_node(
        "blur",
        "Blur",
        vec![image_in(true)],
        vec![
            ParamSpec::new(
                "radius",
                "Radius",
                "filter",
                ParamType::Float,
                ParamValue::Float(4.0),
            )
            .hard(0.0, 64.0)
            .soft(0.0, 16.0)
            .step(0.5),
        ],
        BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        "Separable Gaussian blur (radius in pixels).",
        &["blur", "gaussian", "soften"],
        NodeRole::Standard,
        cook_blur,
    )
}

fn cook_blur(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::blur(
        &img,
        p.f32("radius"),
    ))))
}

#[must_use]
pub fn sharpen_descriptor() -> NodeTypeDescriptor {
    tex_node(
        "sharpen",
        "Sharpen",
        vec![image_in(true)],
        vec![
            ParamSpec::new(
                "amount",
                "Amount",
                "filter",
                ParamType::Float,
                ParamValue::Float(1.0),
            )
            .hard(0.0, 4.0)
            .step(0.05),
        ],
        BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        "Unsharp-mask sharpen.",
        &["sharpen", "unsharp", "detail"],
        NodeRole::Standard,
        cook_sharpen,
    )
}

fn cook_sharpen(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::sharpen(
        &img,
        p.f32("amount"),
    ))))
}

#[must_use]
pub fn pack_orm_descriptor() -> NodeTypeDescriptor {
    let fallback = |key: &'static str, label: &'static str, default: f64, port: &'static str| {
        ParamSpec::new(
            key,
            label,
            "channels",
            ParamType::Float,
            ParamValue::Float(default),
        )
        .hard(0.0, 1.0)
        .step(0.01)
        .driven_by_port(port)
    };
    tex_node(
        "pack_orm",
        "Pack ORM",
        vec![
            PortSpec::single("occlusion", "Occlusion", DataType::Image, false).default_port(),
            PortSpec::single("roughness", "Roughness", DataType::Image, false),
            PortSpec::single("metallic", "Metallic", DataType::Image, false),
        ],
        vec![
            fallback("occlusion", "Occlusion", 1.0, "occlusion"),
            fallback("roughness", "Roughness", 0.7, "roughness"),
            fallback("metallic", "Metallic", 0.0, "metallic"),
        ],
        BypassBehavior::Mute,
        "Packs the glTF ORM layout the renderer consumes: R = occlusion, G = roughness, B = metallic (each input's red channel; a missing input uses its constant).",
        &["orm", "pack", "occlusion", "roughness", "metallic", "gltf"],
        NodeRole::Gather,
        cook_pack_orm,
    )
}

fn cook_pack_orm(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let occlusion = working_image(inputs, "occlusion");
    let roughness = working_image(inputs, "roughness");
    let metallic = working_image(inputs, "metallic");
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::pack_orm(
        occlusion.as_deref(),
        roughness.as_deref(),
        metallic.as_deref(),
        p.f32("occlusion"),
        p.f32("roughness"),
        p.f32("metallic"),
    ))))
}

#[must_use]
pub fn height_to_normal_descriptor() -> NodeTypeDescriptor {
    tex_node(
        "height_to_normal",
        "Height to Normal",
        vec![image_in(true)],
        vec![
            ParamSpec::new(
                "strength",
                "Strength",
                "normal",
                ParamType::Float,
                ParamValue::Float(4.0),
            )
            .hard(0.0, 16.0)
            .step(0.1),
        ],
        BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        "Sobel height-to-normal: the red channel is the height field; outputs a tangent-space normal map.",
        &["normal", "height", "bump", "sobel"],
        NodeRole::Standard,
        cook_height_to_normal,
    )
}

fn cook_height_to_normal(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(
        solarxy_imaging::height_to_normal(&img, p.f32("strength")),
    )))
}
