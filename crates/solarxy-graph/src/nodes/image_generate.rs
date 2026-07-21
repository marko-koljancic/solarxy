//! The texture-context generators: `constant`, `ramp`, `noise`, `voronoi`,
//! `gradient`, `checker`, and `brick`. Portless sources whose dimensions are
//! hard-capped at the working resolution.

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

#[must_use]
pub fn voronoi_descriptor() -> NodeTypeDescriptor {
    let mut params = dims_params();
    params.push(
        ParamSpec::new(
            "scale",
            "Scale",
            "voronoi",
            ParamType::Float,
            ParamValue::Float(8.0),
        )
        .hard(0.5, 64.0)
        .soft(2.0, 16.0)
        .step(0.5)
        .doc(
            "How many cells span the image: 8 scatters an 8x8 lattice of \
             feature points whatever the pixel size. Raise it for smaller, \
             denser cells, lower it for a few big ones. The count is the same \
             on both axes, so a non-square image gets stretched cells.",
        ),
    );
    params.push(
        ParamSpec::new(
            "seed",
            "Seed",
            "voronoi",
            ParamType::Int,
            ParamValue::Int(0),
        )
        .hard(0.0, 9999.0)
        .step(1.0)
        .doc(
            "Selects where the feature points land inside their cells. Any \
             change rescatters them into a new pattern rather than shifting \
             the old one, and the same seed always cooks the same pixels, so \
             a saved scene reproduces exactly.",
        ),
    );
    params.push(
        ParamSpec::new(
            "jitter",
            "Jitter",
            "voronoi",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .hard(0.0, 1.0)
        .step(0.01)
        .doc(
            "How far each feature point strays from its cell centre: 0 pins \
             every point to the centre for a regular grid, 1 lets it fall \
             anywhere in the cell for a fully irregular pattern.",
        ),
    );
    params.push(
        ParamSpec::new(
            "metric",
            "Metric",
            "voronoi",
            ParamType::Enum {
                variants: vec![
                    EnumVariant::new("euclidean", "Euclidean"),
                    EnumVariant::new("manhattan", "Manhattan"),
                    EnumVariant::new("chebyshev", "Chebyshev"),
                ],
            },
            ParamValue::Enum("euclidean".to_string()),
        )
        .doc(
            "The distance measure that decides which feature owns a texel: \
             Euclidean gives round cells, Manhattan diamond-shaped ones, and \
             Chebyshev square ones. It reshapes both the cells and the \
             Distance falloff.",
        ),
    );
    params.push(
        ParamSpec::new(
            "pattern",
            "Pattern",
            "voronoi",
            ParamType::Enum {
                variants: vec![
                    EnumVariant::new("distance", "Distance"),
                    EnumVariant::new("cell_id", "Cell ID"),
                    EnumVariant::new("edges", "Edges"),
                ],
            },
            ParamValue::Enum("distance".to_string()),
        )
        .doc(
            "What each texel stores: Distance is the nearest-feature distance \
             (a cellular falloff, dark at the centres), Cell ID is a flat \
             hashed grey per cell (a random mask), and Edges draws bright \
             lines along the cell boundaries.",
        ),
    );
    generator_descriptor(
        "voronoi",
        "Voronoi",
        params,
        "Worley / Voronoi cellular noise: one jittered feature point per \
         lattice cell, read back per texel as a distance falloff, a per-cell \
         value, or the cell edges.\n\n\
         A staple of stone, scale, crackle, and organic-cell texturing. Feed \
         the Distance pattern into `levels` for cracked-earth or reptile \
         masks, the Cell ID pattern into `mix` for a random per-cell tint, or \
         the Edges pattern as a wear or grout mask.\n\n\
         The output is opaque grey: R, G and B carry the same value and alpha \
         is 1, so this is a scalar field rather than a color. It is \
         deterministic in the seed and, like `noise`, does not tile at the \
         image edge.",
        &["voronoi", "worley", "cellular", "cells"],
        cook_voronoi,
    )
}

fn cook_voronoi(
    p: &ResolvedParams,
    _in: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let (w, h) = dims(p);
    let metric = match p.enum_key("metric") {
        "manhattan" => solarxy_imaging::VoronoiMetric::Manhattan,
        "chebyshev" => solarxy_imaging::VoronoiMetric::Chebyshev,
        _ => solarxy_imaging::VoronoiMetric::Euclidean,
    };
    let pattern = match p.enum_key("pattern") {
        "cell_id" => solarxy_imaging::VoronoiPattern::CellId,
        "edges" => solarxy_imaging::VoronoiPattern::Edges,
        _ => solarxy_imaging::VoronoiPattern::Distance,
    };
    let img = solarxy_imaging::voronoi(
        w,
        h,
        p.f32("scale"),
        p.u32("seed"),
        p.f32("jitter"),
        metric,
        pattern,
    )
    .map_err(imaging_err)?;
    Ok(CookOutcome::Done(image_outputs(img)))
}

#[must_use]
pub fn gradient_descriptor() -> NodeTypeDescriptor {
    let mut params = dims_params();
    params.push(
        ParamSpec::new(
            "mode",
            "Mode",
            "gradient",
            ParamType::Enum {
                variants: vec![
                    EnumVariant::new("linear", "Linear"),
                    EnumVariant::new("radial", "Radial"),
                    EnumVariant::new("angular", "Angular"),
                    EnumVariant::new("diamond", "Diamond"),
                ],
            },
            ParamValue::Enum("linear".to_string()),
        )
        .doc(
            "How the blend factor is measured around the Centre: Linear runs \
             left to right through it, Radial spreads outward from it, Angular \
             sweeps the angle around it (a conic gradient), and Diamond is a \
             rotated square. All four honour Centre.",
        ),
    );
    params.push(
        ParamSpec::new(
            "color_a",
            "From",
            "gradient",
            ParamType::Color,
            ParamValue::Color([0.0, 0.0, 0.0, 1.0]),
        )
        .doc(
            "The color at the start of the gradient: the left edge, the \
             centre, or the start of the angular sweep depending on Mode. All \
             four channels interpolate, so an alpha set here ramps as well.",
        ),
    );
    params.push(
        ParamSpec::new(
            "color_b",
            "To",
            "gradient",
            ParamType::Color,
            ParamValue::Color([1.0, 1.0, 1.0, 1.0]),
        )
        .doc(
            "The color at the end of the gradient: the right edge, the outer \
             limit of the falloff, or the end of the angular sweep. Swap it \
             with From to reverse the gradient.",
        ),
    );
    params.push(
        ParamSpec::new(
            "center",
            "Center",
            "gradient",
            ParamType::Vec2,
            ParamValue::Vec2([0.5, 0.5]),
        )
        .hard(0.0, 1.0)
        .step(0.01)
        .doc(
            "Where the gradient originates, in normalized 0..1 coordinates \
             (0.5, 0.5 is the image centre). Radial, Angular, and Diamond \
             spread from here; Linear uses only its x as the midpoint.",
        ),
    );
    generator_descriptor(
        "gradient",
        "Gradient",
        params,
        "A two-color gradient with a movable centre and four falloff shapes: \
         linear, radial, angular (conic), or diamond.\n\n\
         The richer sibling of `ramp`: reach for it when you need a conic or \
         diamond falloff or an off-centre origin that `ramp`'s fixed \
         horizontal / vertical / radial cannot give. Feed it into `mix` as a \
         base or blend layer, or into `levels` to shape the falloff.\n\n\
         All four channels interpolate, alpha included. Distances are measured \
         in normalized coordinates, so the rings and diamonds become ellipses \
         and rhombi on a non-square image, and the factor is clamped so the \
         corners past the far color sit flat.",
        &["gradient", "conic", "radial", "linear"],
        cook_gradient,
    )
}

fn cook_gradient(
    p: &ResolvedParams,
    _in: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let (w, h) = dims(p);
    let mode = match p.enum_key("mode") {
        "radial" => solarxy_imaging::GradientMode::Radial,
        "angular" => solarxy_imaging::GradientMode::Angular,
        "diamond" => solarxy_imaging::GradientMode::Diamond,
        _ => solarxy_imaging::GradientMode::Linear,
    };
    let center = p.vec2("center");
    let center = [center[0] as f32, center[1] as f32];
    let img = solarxy_imaging::gradient(w, h, mode, p.color("color_a"), p.color("color_b"), center)
        .map_err(imaging_err)?;
    Ok(CookOutcome::Done(image_outputs(img)))
}

#[must_use]
pub fn checker_descriptor() -> NodeTypeDescriptor {
    let mut params = dims_params();
    params.push(
        ParamSpec::new(
            "color_a",
            "Color A",
            "checker",
            ParamType::Color,
            ParamValue::Color([0.1, 0.1, 0.1, 1.0]),
        )
        .doc(
            "The color of the even tiles (the top-left one). Written straight \
             into the texels with no conversion, alpha included, so it lands \
             exactly as the picker shows it.",
        ),
    );
    params.push(
        ParamSpec::new(
            "color_b",
            "Color B",
            "checker",
            ParamType::Color,
            ParamValue::Color([0.9, 0.9, 0.9, 1.0]),
        )
        .doc(
            "The color of the odd tiles, alternating with Color A across the \
             board. Written straight into the texels with no conversion, \
             alpha included.",
        ),
    );
    params.push(
        ParamSpec::new(
            "tiles_x",
            "Tiles X",
            "checker",
            ParamType::Int,
            ParamValue::Int(8),
        )
        .hard(1.0, 256.0)
        .soft(2.0, 32.0)
        .step(1.0)
        .doc(
            "How many tiles span the image horizontally. Independent of Tiles \
             Y, so an uneven count gives rectangular tiles, and the grid maps \
             to normalized coordinates rather than staying square.",
        ),
    );
    params.push(
        ParamSpec::new(
            "tiles_y",
            "Tiles Y",
            "checker",
            ParamType::Int,
            ParamValue::Int(8),
        )
        .hard(1.0, 256.0)
        .soft(2.0, 32.0)
        .step(1.0)
        .doc(
            "How many tiles span the image vertically. Independent of Tiles \
             X, so an uneven count gives rectangular tiles.",
        ),
    );
    generator_descriptor(
        "checker",
        "Checker",
        params,
        "A two-color checkerboard of Tiles X by Tiles Y alternating cells.\n\n\
         The classic UV and scale reference: drop it on a model to read \
         texel density and seams at a glance, or use it as a hard-edged mask \
         and tint source. Two checkers at different tile counts through a \
         `mix` gives a quick plaid.\n\n\
         Colors are written without conversion, alpha included, so what the \
         picker shows is what the texels hold. Tiles map to a normalized \
         grid, so a non-square image or an uneven tile count yields \
         rectangular cells rather than square ones.",
        &["checker", "checkerboard", "grid", "uv"],
        cook_checker,
    )
}

fn cook_checker(
    p: &ResolvedParams,
    _in: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let (w, h) = dims(p);
    let img = solarxy_imaging::checker(
        w,
        h,
        p.color("color_a"),
        p.color("color_b"),
        p.u32("tiles_x"),
        p.u32("tiles_y"),
    )
    .map_err(imaging_err)?;
    Ok(CookOutcome::Done(image_outputs(img)))
}

#[must_use]
pub fn brick_descriptor() -> NodeTypeDescriptor {
    let mut params = dims_params();
    params.push(
        ParamSpec::new(
            "brick_color",
            "Brick",
            "brick",
            ParamType::Color,
            ParamValue::Color([0.55, 0.25, 0.18, 1.0]),
        )
        .doc(
            "The color of the bricks themselves. Written straight into the \
             texels with no conversion, alpha included, so it lands exactly \
             as the picker shows it.",
        ),
    );
    params.push(
        ParamSpec::new(
            "mortar_color",
            "Mortar",
            "brick",
            ParamType::Color,
            ParamValue::Color([0.85, 0.85, 0.82, 1.0]),
        )
        .doc(
            "The color of the mortar lines between the bricks. Written \
             straight into the texels with no conversion, alpha included.",
        ),
    );
    params.push(
        ParamSpec::new(
            "columns",
            "Columns",
            "brick",
            ParamType::Int,
            ParamValue::Int(6),
        )
        .hard(1.0, 64.0)
        .soft(2.0, 16.0)
        .step(1.0)
        .doc(
            "How many bricks span the image horizontally. Independent of \
             Rows, so tall or squat bricks are a matter of the two counts.",
        ),
    );
    params.push(
        ParamSpec::new("rows", "Rows", "brick", ParamType::Int, ParamValue::Int(12))
            .hard(1.0, 64.0)
            .soft(2.0, 24.0)
            .step(1.0)
            .doc(
                "How many brick courses span the image vertically. \
                 Independent of Columns; alternate courses shift by Row \
                 Offset for the running bond.",
            ),
    );
    params.push(
        ParamSpec::new(
            "mortar",
            "Mortar Width",
            "brick",
            ParamType::Float,
            ParamValue::Float(0.06),
        )
        .hard(0.0, 0.5)
        .step(0.01)
        .doc(
            "The mortar thickness as a fraction of a cell (0 is no mortar, \
             0.5 leaves no brick). Applied on all four sides of each brick, \
             so the visible brick shrinks as this grows.",
        ),
    );
    params.push(
        ParamSpec::new(
            "row_offset",
            "Row Offset",
            "brick",
            ParamType::Float,
            ParamValue::Float(0.5),
        )
        .hard(0.0, 1.0)
        .step(0.01)
        .doc(
            "How far alternate courses shift sideways, as a fraction of a \
             brick: 0.5 is the classic running bond, 0 stacks the bricks in \
             straight columns.",
        ),
    );
    generator_descriptor(
        "brick",
        "Brick",
        params,
        "A running-bond brick wall: Columns by Rows bricks separated by \
         mortar, with alternate courses shifted by Row Offset.\n\n\
         A ready architectural pattern and a compact test of a texture \
         chain. Feed it into `height_to_normal` for a raised-brick surface, \
         into `levels` for a wear mask, or `mix` it with `noise` to break up \
         the flat colors.\n\n\
         Colors are written without conversion, alpha included. The layout is \
         in normalized coordinates, so a non-square image stretches the \
         bricks; Mortar Width is a fraction of a cell, applied on every side, \
         and Row Offset drives the bond from stacked to running.",
        &["brick", "wall", "masonry", "bond"],
        cook_brick,
    )
}

fn cook_brick(
    p: &ResolvedParams,
    _in: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let (w, h) = dims(p);
    let img = solarxy_imaging::brick(
        w,
        h,
        p.color("brick_color"),
        p.color("mortar_color"),
        p.u32("columns"),
        p.u32("rows"),
        p.f32("mortar"),
        p.f32("row_offset"),
    )
    .map_err(imaging_err)?;
    Ok(CookOutcome::Done(image_outputs(img)))
}
