//! The texture-context generators: `constant`, `ramp`, and
//! `noise`. Portless sources whose dimensions are hard-capped at the
//! working resolution.

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
    let dim = |key: &'static str, label: &'static str, axis: &str| {
        ParamSpec::new(key, label, "size", ParamType::Int, ParamValue::Int(512))
            .hard(16.0, f64::from(WORKING_EDGE))
            .soft(64.0, 1024.0)
            .step(1.0)
            .doc(format!(
                "Pixels across the generated image's {axis}. Independent of \
                 the other axis, so a non-square image is fine, but the \
                 pattern is laid out in normalized coordinates and stretches \
                 to fit rather than staying square. The ceiling is \
                 {WORKING_EDGE}, the working resolution the texture context \
                 cooks at; these are single-threaded CPU loops, so doubling \
                 both dimensions quadruples the pixels a cook touches."
            ))
    };
    vec![
        dim("width", "Width", "width"),
        dim("height", "Height", "height"),
    ]
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
    params.push(
        ParamSpec::new(
            "color",
            "Color",
            "color",
            ParamType::Color,
            ParamValue::Color([0.5, 0.5, 0.5, 1.0]),
        )
        .doc(
            "The fill, RGBA. It is written straight into the stored 8-bit \
             texels with no color conversion, so it lands exactly as the \
             picker shows it. The alpha lane is real: 1 is opaque, and \
             lowering it only changes what `mix` in Over mode does with the \
             image.",
        ),
    );
    generator_descriptor(
        "constant",
        "Constant",
        params,
        "A solid image: every texel is the Color param, at the given size.\n\n\
         The workhorse fill of a texture network. Reach for it as the base \
         layer under a `mix`, as a flat map into `pack_orm`, or as a solid \
         tint to multiply an imported image against. Two constants and a \
         `mix` is the shortest path to a mask.\n\n\
         The color is not converted on the way in, so what the picker shows \
         is what the texels hold. Alpha survives every adjust node \
         untouched (`blur` is the one exception, it filters alpha too), but \
         only `mix` in Over mode ever reads it.",
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
    params.push(
        ParamSpec::new(
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
        )
        .doc(
            "How the blend factor is measured across the image: Horizontal \
             runs left to right, Vertical top to bottom, Radial outward from \
             the centre. Radial reaches `To` at the four edge midpoints \
             rather than at the corners, so the corners sit clamped at flat \
             `To`.",
        ),
    );
    params.push(
        ParamSpec::new(
            "color_a",
            "From",
            "ramp",
            ParamType::Color,
            ParamValue::Color([0.0, 0.0, 0.0, 1.0]),
        )
        .doc(
            "The color at the start of the gradient: the left edge, the top \
             edge, or the image centre, depending on Direction. All four \
             channels interpolate, so an alpha set here ramps as well.",
        ),
    );
    params.push(
        ParamSpec::new(
            "color_b",
            "To",
            "ramp",
            ParamType::Color,
            ParamValue::Color([1.0, 1.0, 1.0, 1.0]),
        )
        .doc(
            "The color at the end of the gradient: the right edge, the \
             bottom edge, or the outer limit of the radial falloff. Swap it \
             with `From` to reverse the gradient.",
        ),
    );
    generator_descriptor(
        "ramp",
        "Ramp",
        params,
        "A two-color gradient across the image: `From` at one end, `To` at \
         the other, linearly interpolated, horizontal, vertical, or \
         radial.\n\n\
         The standard mask and gradient source. Feed it into `mix` as the \
         base or the blend layer, or into `levels` to shape the falloff, \
         which is the usual way to give a linear ramp a knee.\n\n\
         All four channels interpolate, alpha included. Radial measures \
         distance from the image centre in normalized coordinates and \
         doubles it, so it hits `To` at the four edge midpoints and the \
         corners are clamped flat; that same normalization makes the rings \
         ellipses rather than circles on a non-square image.",
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
        .step(1.0)
        .doc(
            "How many noise cells span the image: 8 lays an 8x8 lattice \
             whatever the pixel size. Raise it for finer grain, lower it for \
             broad blobs. The count is the same on both axes, so a \
             non-square image gets stretched cells.",
        ),
    );
    params.push(
        ParamSpec::new("seed", "Seed", "noise", ParamType::Int, ParamValue::Int(0))
            .hard(0.0, 9999.0)
            .step(1.0)
            .doc(
                "Selects the hash lattice. Any change gives a completely \
                 different image rather than a shifted one, so scrub it to \
                 hunt for a pattern you like. The same seed always cooks the \
                 same pixels, which is what lets a saved scene reproduce \
                 exactly.",
            ),
    );
    generator_descriptor(
        "noise",
        "Noise",
        params,
        "Value noise: a lattice of hashed values, smoothstep-interpolated \
         into a grayscale image. Deterministic, so the same seed and size \
         cook the same pixels in every session and on every machine.\n\n\
         The base of most procedural texture work. Run it through `levels` \
         or `brightness_contrast` to shape its range, `blur` to soften it, \
         or feed it straight to `height_to_normal` for a bumpy surface. Two \
         noise nodes at different scales through a `mix` in Multiply is the \
         cheap way to get detail at two frequencies.\n\n\
         The output is opaque gray: R, G and B carry the same value and \
         alpha is always 1, so this is a scalar field rather than a color. \
         There is no octave or fractal control, just one lattice at one \
         frequency, and nothing wraps that lattice at the image edge, so the \
         result does not tile.",
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
