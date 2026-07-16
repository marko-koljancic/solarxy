//! The texture-context Adjust group (phase 19): `levels`,
//! `brightness_contrast`, `hue_saturation`, `invert`, and `gamma`. Every
//! node is a synchronous per-pixel map over the working-resolution image
//! (`solarxy-imaging` does the math; these bodies only gather and
//! delegate).

use super::common::params_with;
use super::image_support::{image_in, image_out, image_outputs, require_image};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

/// The shared shell every single-input adjust node uses.
fn adjust_descriptor(
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
        category: Category::Modifiers,
        contexts: ContextSet::TEX,
        opens: None,
        inputs: vec![image_in(true)],
        outputs: vec![image_out()],
        params: params_with(display_name, params),
        bypass: BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        doc,
        search_aliases,
        glyph: type_id,
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

fn float(key: &'static str, label: &'static str, default: f64) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        "adjust",
        ParamType::Float,
        ParamValue::Float(default),
    )
}

#[must_use]
pub fn levels_descriptor() -> NodeTypeDescriptor {
    adjust_descriptor(
        "levels",
        "Levels",
        vec![
            float("in_black", "Input Black", 0.0)
                .hard(0.0, 1.0)
                .step(0.01),
            float("in_white", "Input White", 1.0)
                .hard(0.0, 1.0)
                .step(0.01),
            float("gamma", "Gamma", 1.0)
                .hard(0.1, 4.0)
                .soft(0.2, 3.0)
                .step(0.01),
            float("out_black", "Output Black", 0.0)
                .hard(0.0, 1.0)
                .step(0.01),
            float("out_white", "Output White", 1.0)
                .hard(0.0, 1.0)
                .step(0.01),
        ],
        "Remaps tonal range: input black/white points, midtone gamma, output range.",
        &["levels", "tone", "remap", "histogram"],
        cook_levels,
    )
}

fn cook_levels(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::levels(
        &img,
        p.f32("in_black"),
        p.f32("in_white"),
        p.f32("gamma"),
        p.f32("out_black"),
        p.f32("out_white"),
    ))))
}

#[must_use]
pub fn brightness_contrast_descriptor() -> NodeTypeDescriptor {
    adjust_descriptor(
        "brightness_contrast",
        "Brightness / Contrast",
        vec![
            float("brightness", "Brightness", 0.0)
                .hard(-1.0, 1.0)
                .step(0.01),
            float("contrast", "Contrast", 0.0)
                .hard(-1.0, 1.0)
                .step(0.01),
        ],
        "Additive brightness and pivot-0.5 contrast.",
        &["brightness", "contrast", "exposure"],
        cook_brightness_contrast,
    )
}

fn cook_brightness_contrast(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(
        solarxy_imaging::brightness_contrast(&img, p.f32("brightness"), p.f32("contrast")),
    )))
}

#[must_use]
pub fn hue_saturation_descriptor() -> NodeTypeDescriptor {
    adjust_descriptor(
        "hue_saturation",
        "Hue / Saturation",
        vec![
            float("hue", "Hue Shift", 0.0).hard(-180.0, 180.0).step(1.0),
            float("saturation", "Saturation", 1.0)
                .hard(0.0, 2.0)
                .step(0.01),
            float("lightness", "Lightness", 1.0)
                .hard(0.0, 2.0)
                .step(0.01),
        ],
        "HSL adjustment: hue shift in degrees, saturation and lightness as multipliers.",
        &["hue", "saturation", "hsl", "color"],
        cook_hue_saturation,
    )
}

fn cook_hue_saturation(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(
        solarxy_imaging::hue_saturation(
            &img,
            p.f32("hue"),
            p.f32("saturation"),
            p.f32("lightness"),
        ),
    )))
}

#[must_use]
pub fn invert_descriptor() -> NodeTypeDescriptor {
    adjust_descriptor(
        "invert",
        "Invert",
        vec![],
        "Inverts RGB; alpha is untouched.",
        &["invert", "negative"],
        cook_invert,
    )
}

fn cook_invert(
    _p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::invert(
        &img,
    ))))
}

#[must_use]
pub fn gamma_descriptor() -> NodeTypeDescriptor {
    adjust_descriptor(
        "gamma",
        "Gamma",
        vec![
            float("gamma", "Gamma", 1.0)
                .hard(0.1, 4.0)
                .soft(0.2, 3.0)
                .step(0.01),
        ],
        "A plain gamma curve (1 = identity).",
        &["gamma", "curve"],
        cook_gamma,
    )
}

fn cook_gamma(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::gamma(
        &img,
        p.f32("gamma"),
    ))))
}
