//! The texture-context generators (phase 19): `constant`, `ramp`, and
//! `noise`. Portless sources whose dimensions are hard-capped at the
//! working resolution (decision C-5).

use super::common::params_with;
use super::image_support::{WORKING_EDGE, image_out, image_outputs};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

fn generator_descriptor(
    type_id: &'static str,
    display_name: &'static str,
    params: Vec<ParamSpec>,
    doc: &'static str,
    search_aliases: &'static [&'static str],
    cook: crate::cook::CookFn,
) -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id,
        version: 1,
        display_name,
        category: Category::Primitives,
        contexts: ContextSet::TEX,
        opens: None,
        inputs: vec![],
        outputs: vec![image_out()],
        params: params_with(display_name, params),
        bypass: BypassBehavior::Mute,
        doc,
        search_aliases,
        glyph: type_id,
        role: NodeRole::ImageSource,
        cook,
        migrate: None,
    }
}

fn dims_params() -> Vec<ParamSpec> {
    let dim = |key: &'static str, label: &'static str| {
        ParamSpec::new(key, label, "size", ParamType::Int, ParamValue::Int(512))
            .hard(16.0, f64::from(WORKING_EDGE))
            .soft(64.0, 1024.0)
            .step(1.0)
    };
    vec![dim("width", "Width"), dim("height", "Height")]
}

fn dims(p: &ResolvedParams) -> (u32, u32) {
    (p.u32("width"), p.u32("height"))
}

fn imaging_err(e: solarxy_imaging::ImagingError) -> CookError {
    CookError::Failed {
        message: e.to_string(),
    }
}

#[must_use]
pub fn constant_descriptor() -> NodeTypeDescriptor {
    let mut params = dims_params();
    params.push(ParamSpec::new(
        "color",
        "Color",
        "color",
        ParamType::Color,
        ParamValue::Color([0.5, 0.5, 0.5, 1.0]),
    ));
    generator_descriptor(
        "constant",
        "Constant",
        params,
        "A solid color image.",
        &["constant", "solid", "fill", "color"],
        cook_constant,
    )
}

fn cook_constant(
    p: &ResolvedParams,
    _in: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let (w, h) = dims(p);
    let img = solarxy_imaging::constant(w, h, p.color("color")).map_err(imaging_err)?;
    Ok(CookOutcome::Done(image_outputs(img)))
}

#[must_use]
pub fn ramp_descriptor() -> NodeTypeDescriptor {
    let mut params = dims_params();
    params.push(ParamSpec::new(
        "direction",
        "Direction",
        "ramp",
        ParamType::Enum {
            variants: vec![
                EnumVariant::new("horizontal", "Horizontal"),
                EnumVariant::new("vertical", "Vertical"),
                EnumVariant::new("radial", "Radial"),
            ],
        },
        ParamValue::Enum("horizontal".to_string()),
    ));
    params.push(ParamSpec::new(
        "color_a",
        "From",
        "ramp",
        ParamType::Color,
        ParamValue::Color([0.0, 0.0, 0.0, 1.0]),
    ));
    params.push(ParamSpec::new(
        "color_b",
        "To",
        "ramp",
        ParamType::Color,
        ParamValue::Color([1.0, 1.0, 1.0, 1.0]),
    ));
    generator_descriptor(
        "ramp",
        "Ramp",
        params,
        "A two-color gradient: horizontal, vertical, or radial.",
        &["ramp", "gradient"],
        cook_ramp,
    )
}

fn cook_ramp(
    p: &ResolvedParams,
    _in: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let (w, h) = dims(p);
    let direction = match p.enum_key("direction") {
        "vertical" => solarxy_imaging::RampDirection::Vertical,
        "radial" => solarxy_imaging::RampDirection::Radial,
        _ => solarxy_imaging::RampDirection::Horizontal,
    };
    let img = solarxy_imaging::ramp(w, h, direction, p.color("color_a"), p.color("color_b"))
        .map_err(imaging_err)?;
    Ok(CookOutcome::Done(image_outputs(img)))
}

#[must_use]
pub fn noise_descriptor() -> NodeTypeDescriptor {
    let mut params = dims_params();
    params.push(
        ParamSpec::new(
            "scale",
            "Scale",
            "noise",
            ParamType::Float,
            ParamValue::Float(8.0),
        )
        .hard(1.0, 64.0)
        .step(1.0),
    );
    params.push(
        ParamSpec::new("seed", "Seed", "noise", ParamType::Int, ParamValue::Int(0))
            .hard(0.0, 9999.0)
            .step(1.0),
    );
    generator_descriptor(
        "noise",
        "Noise",
        params,
        "Deterministic value noise (same seed, same image, everywhere).",
        &["noise", "random", "value noise"],
        cook_noise,
    )
}

fn cook_noise(
    p: &ResolvedParams,
    _in: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let (w, h) = dims(p);
    let img = solarxy_imaging::noise(w, h, p.f32("scale"), p.u32("seed")).map_err(imaging_err)?;
    Ok(CookOutcome::Done(image_outputs(img)))
}
