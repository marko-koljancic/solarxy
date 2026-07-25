//! The texture-context Adjust group: `levels`,
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
        category: Category::TexAdjust,
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
                .step(0.01)
                .doc(
                    "Input at or below this maps to full black before the \
                     rest of the curve runs. Raising it crushes shadows and \
                     adds contrast. Push it past Input White and the two \
                     collapse into a hard threshold at this value rather \
                     than misbehaving.",
                ),
            float("in_white", "Input White", 1.0)
                .hard(0.0, 1.0)
                .step(0.01)
                .doc(
                    "Input at or above this maps to full white. Lowering it \
                     blows out highlights and adds contrast. It is always \
                     held a hair above Input Black, so the pair can never \
                     divide by zero; they threshold instead.",
                ),
            float("gamma", "Gamma", 1.0)
                .hard(0.1, 4.0)
                .soft(0.2, 3.0)
                .step(0.01)
                .doc(
                    "Bends the midtones after the input range is \
                     normalized: the value is raised to the power 1/gamma, \
                     so above 1 lifts midtones and below 1 deepens them. 1 \
                     is a straight line, and the black and white ends stay \
                     pinned whatever you set here.",
                ),
            float("out_black", "Output Black", 0.0)
                .hard(0.0, 1.0)
                .step(0.01)
                .doc(
                    "Where full black lands once the curve has run. Raise \
                     it to lift the whole image off black, the usual way to \
                     fake a washed-out or hazy look.",
                ),
            float("out_white", "Output White", 1.0)
                .hard(0.0, 1.0)
                .step(0.01)
                .doc(
                    "Where full white lands once the curve has run. Lower it \
                     to pull the image off white. Set it below Output Black \
                     and the output range runs backwards, inverting the \
                     image with the rest of the curve still applied.",
                ),
        ],
        "Photoshop-style levels over the RGB channels: Input Black and Input \
         White stretch to the full range, a midtone Gamma bends the curve, \
         then Output Black and Output White compress the result back into a \
         range.\n\n\
         The main tonal tool of a texture network. Reach for it to set the \
         range of a `noise` or `ramp` before it becomes a mask, or to fix a \
         flat imported image. `gamma` is the same midtone move with none of \
         the range controls; `brightness_contrast` is the blunter, symmetric \
         version.\n\n\
         It works on the stored sRGB-encoded bytes, the convention of 2D \
         image editors, not on linear light, so the numbers match what \
         Photoshop would show rather than what a shader would compute. The \
         whole curve collapses into a 256-entry lookup table applied per \
         channel, so the cost is the same whatever the settings, and alpha \
         is never touched.",
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
                .step(0.01)
                .doc(
                    "Added to every channel after the contrast scale, where \
                     0 is identity. It is a flat offset on the stored value, \
                     not an exposure multiply, so it lifts shadows exactly \
                     as much as highlights and clips at the ends.",
                ),
            float("contrast", "Contrast", 0.0)
                .hard(-1.0, 1.0)
                .step(0.01)
                .doc(
                    "Scales each channel away from the 0.5 pivot, where 0 is \
                     identity. The multiplier is 1 plus this value, so 1 \
                     doubles the spread and -1 flattens the image to \
                     mid-gray entirely.",
                ),
        ],
        "Scales each RGB channel around a 0.5 pivot by Contrast, then adds \
         Brightness on top. Both run through a 256-entry lookup table; alpha \
         is untouched.\n\n\
         The quick tonal fix, one step blunter than `levels`: two knobs, no \
         per-end control. Reach for it to nudge an imported image or to open \
         up a `noise` before it becomes a mask. When you need the black and \
         white points independently, or a midtone bend, go to `levels`.\n\n\
         The pivot is fixed at 0.5, so contrast leaves mid-gray exactly \
         where it is and moves everything else around it. Contrast at -1 \
         collapses the image to that pivot, flat gray plus whatever \
         Brightness adds. Both controls clip against 0 and 1, and because \
         the result is baked into a lookup table, a clipped highlight is \
         gone for good rather than waiting for a later node to pull it back.",
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
            float("hue", "Hue Shift", 0.0)
                .hard(-180.0, 180.0)
                .step(1.0)
                .doc(
                    "Rotates the hue wheel by this many degrees; 0 is \
                     identity and the rotation wraps, so -180 and 180 land \
                     in the same place. It can do nothing to a pixel with no \
                     saturation: gray has no hue to rotate.",
                ),
            float("saturation", "Saturation", 1.0)
                .hard(0.0, 2.0)
                .step(0.01)
                .doc(
                    "Multiplies HSL saturation, where 1 is identity and 0 \
                     desaturates to gray. Above 1 pushes toward pure hue, \
                     but the result clamps at 1, so heavy values flatten \
                     distinct colors into the same fully-saturated tone.",
                ),
            float("lightness", "Lightness", 1.0)
                .hard(0.0, 2.0)
                .step(0.01)
                .doc(
                    "Multiplies HSL lightness, where 1 is identity and 0 \
                     goes to black. Because it multiplies rather than \
                     offsets, it can never lift a pure black pixel off zero, \
                     and it clamps at 1 on the way up.",
                ),
        ],
        "Converts each pixel to HSL, shifts the hue, multiplies saturation \
         and lightness, and converts back. Alpha is untouched.\n\n\
         The color-side counterpart to `levels`. Reach for it to retint an \
         imported albedo, to pull the color out of an image on the way to a \
         mask (Saturation 0), or to make color variants of one texture \
         network by shifting hue alone.\n\n\
         Saturation and Lightness are multipliers, not offsets, and that \
         bites in two places: Lightness cannot lift a pure black pixel, \
         because zero times anything is still zero, and Hue Shift does \
         nothing whatever to a gray or white pixel, because a pixel with no \
         saturation has no hue to shift. This is also the one adjust node \
         with no lookup table behind it: it runs the HSL round trip per \
         pixel.",
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
        "Replaces every RGB channel with 255 minus its stored value. Alpha \
         rides through untouched, and there are no parameters.\n\n\
         The mask flipper. Most of its use is between a `ramp` or `noise` \
         and whatever consumes the mask, when the falloff points the wrong \
         way, or ahead of a `mix` to swap which side of a blend a mask \
         selects. It is also the quickest way to turn a height field into \
         its own inverse before `height_to_normal`.\n\n\
         It inverts the stored sRGB-encoded bytes rather than linear light, \
         so this is the photo-editor negative, not a photometric one. \
         Applying it twice returns the original image exactly, which the \
         imaging crate's tests pin.",
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
                .step(0.01)
                .doc(
                    "The curve, applied as the power 1/gamma, where 1 is \
                     identity. Above 1 lifts midtones, below 1 deepens them; \
                     black and white stay put either way. Hard-floored at \
                     0.1 so the curve cannot stand up vertical.",
                ),
        ],
        "Raises every RGB channel to the power 1/gamma through a 256-entry \
         lookup table. 1 is identity, and alpha is untouched.\n\n\
         The single-knob midtone bend, and the same curve `levels` applies \
         in the middle of its chain. Reach for it when the bend is all you \
         want and for `levels` when you also need the black and white \
         points. It is the usual correction between an image authored to be \
         looked at and one about to be read as data, e.g. a height field on \
         its way into `height_to_normal`.\n\n\
         The ends are pinned: 0 stays 0 and 1 stays 1 whatever the value, so \
         this only redistributes the middle and can never clip. Above 1 \
         brightens midtones and below 1 darkens them, which is the reverse \
         of what someone thinking of gamma as a plain exponent expects -- \
         the exponent actually applied is the reciprocal.",
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
