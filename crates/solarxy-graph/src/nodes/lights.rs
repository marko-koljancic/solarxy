//! The six light nodes (node catalog part II, section 12): point,
//! directional, spot, ambient, hemisphere, rect-area.
//!
//! Lights are portless, root-context, `Mute`-bypassable, and cook
//! passively: their `LightDef` is resolved directly from their params by
//! the engine's scene builder (`engine::scene`), not carried on a wire.
//! Shadow-capable lights (point / directional / spot) carry `cast_shadow`
//! with exclusive-caster radio semantics enforced downstream (decision 27).

use super::common::{general_params, passive_cook};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Pred, Unit};
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// The `visible` / `show_helper` / `helper_size` group every light carries.
fn helper_group() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "visible",
            "Visible",
            "rendering",
            ParamType::Bool,
            ParamValue::Bool(true),
        ),
        ParamSpec::new(
            "show_helper",
            "Show Helper",
            "rendering",
            ParamType::Bool,
            ParamValue::Bool(false),
        ),
        ParamSpec::new(
            "helper_size",
            "Helper Size",
            "rendering",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .hard(0.1, 10.0),
    ]
}

fn color_param(key: &str, label: &str, default: [f32; 4]) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        "light",
        ParamType::Color,
        ParamValue::Color(default),
    )
}

fn intensity(default: f64) -> ParamSpec {
    ParamSpec::new(
        "intensity",
        "Intensity",
        "light",
        ParamType::Float,
        ParamValue::Float(default),
    )
    .hard(0.0, 1000.0)
    .soft(0.0, 10.0)
}

fn map_size_param(default: &str) -> ParamSpec {
    ParamSpec::new(
        "map_size",
        "Shadow Map Size",
        "shadow",
        ParamType::Enum {
            variants: vec![
                EnumVariant::new("512", "512"),
                EnumVariant::new("1024", "1024"),
                EnumVariant::new("2048", "2048"),
            ],
        },
        ParamValue::Enum(default.to_string()),
    )
    .show_if("cast_shadow", Pred::Truthy)
}

fn bias_param(default: f64) -> ParamSpec {
    ParamSpec::new(
        "bias",
        "Shadow Bias",
        "shadow",
        ParamType::Float,
        ParamValue::Float(default),
    )
    .hard(-0.01, 0.01)
    .step(0.0001)
    .show_if("cast_shadow", Pred::Truthy)
}

/// `glyph` is the icon key: the type id with its `_light` suffix dropped
/// (`point`, `directional`, ...), passed per light like the other identity
/// fields.
fn assemble(
    type_id: &'static str,
    display_name: &'static str,
    doc: &'static str,
    aliases: &'static [&'static str],
    glyph: &'static str,
    display: &str,
    specific: Vec<ParamSpec>,
) -> NodeTypeDescriptor {
    let mut params = general_params(display);
    params.extend(specific);
    params.extend(helper_group());
    NodeTypeDescriptor {
        type_id,
        version: 1,
        display_name,
        category: Category::Lights,
        contexts: ContextSet::OBJ,
        opens: None,
        inputs: vec![],
        outputs: vec![],
        params,
        bypass: BypassBehavior::Mute,
        doc,
        search_aliases: aliases,
        glyph,
        role: NodeRole::Light,
        cook: passive_cook,
        migrate: None,
    }
}

#[must_use]
pub fn point_descriptor() -> NodeTypeDescriptor {
    assemble(
        "point_light",
        "Point Light",
        "An omnidirectional light with distance falloff.",
        &["light", "omni", "bulb"],
        "point",
        "Point Light",
        vec![
            ParamSpec::new(
                "position",
                "Position",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([10.0, 10.0, 5.0]),
            )
            .unit(Unit::Meters),
            color_param("color", "Color", WHITE),
            intensity(1.5),
            ParamSpec::new(
                "range",
                "Range",
                "light",
                ParamType::Float,
                ParamValue::Float(0.0),
            )
            .hard(0.0, 100_000.0)
            .soft(0.0, 1000.0)
            .unit(Unit::Meters),
            ParamSpec::new(
                "decay",
                "Decay",
                "light",
                ParamType::Float,
                ParamValue::Float(2.0),
            )
            .hard(0.0, 10.0),
            ParamSpec::new(
                "cast_shadow",
                "Cast Shadow",
                "light",
                ParamType::Bool,
                ParamValue::Bool(true),
            ),
            map_size_param("1024"),
            bias_param(-0.0001),
        ],
    )
}

#[must_use]
pub fn directional_descriptor() -> NodeTypeDescriptor {
    assemble(
        "directional_light",
        "Directional Light",
        "A parallel light (like the sun); its shadow frustum auto-fits the \
         scene bounds.",
        &["light", "sun", "sky"],
        "directional",
        "Directional Light",
        vec![
            ParamSpec::new(
                "position",
                "Position",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([10.0, 10.0, 5.0]),
            )
            .unit(Unit::Meters),
            ParamSpec::new(
                "target",
                "Target",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([0.0; 3]),
            )
            .unit(Unit::Meters),
            color_param("color", "Color", WHITE),
            intensity(1.5),
            ParamSpec::new(
                "cast_shadow",
                "Cast Shadow",
                "light",
                ParamType::Bool,
                ParamValue::Bool(true),
            ),
            map_size_param("2048"),
            bias_param(0.0001),
        ],
    )
}

#[must_use]
pub fn spot_descriptor() -> NodeTypeDescriptor {
    assemble(
        "spot_light",
        "Spot Light",
        "A cone light with an angle and soft-edge penumbra.",
        &["light", "cone", "flashlight"],
        "spot",
        "Spot Light",
        vec![
            ParamSpec::new(
                "position",
                "Position",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([10.0, 10.0, 5.0]),
            )
            .unit(Unit::Meters),
            ParamSpec::new(
                "target",
                "Target",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([0.0; 3]),
            )
            .unit(Unit::Meters),
            color_param("color", "Color", WHITE),
            intensity(1.5),
            ParamSpec::new(
                "range",
                "Range",
                "light",
                ParamType::Float,
                ParamValue::Float(0.0),
            )
            .hard(0.0, 100_000.0)
            .soft(0.0, 1000.0)
            .unit(Unit::Meters),
            ParamSpec::new(
                "decay",
                "Decay",
                "light",
                ParamType::Float,
                ParamValue::Float(2.0),
            )
            .hard(0.0, 10.0),
            ParamSpec::new(
                "angle",
                "Angle",
                "light",
                ParamType::Float,
                ParamValue::Float(45.0),
            )
            .hard(1.0, 89.0)
            .unit(Unit::Degrees),
            ParamSpec::new(
                "penumbra",
                "Penumbra",
                "light",
                ParamType::Float,
                ParamValue::Float(0.0),
            )
            .hard(0.0, 1.0)
            .unit(Unit::Normalized),
            ParamSpec::new(
                "cast_shadow",
                "Cast Shadow",
                "light",
                ParamType::Bool,
                ParamValue::Bool(true),
            ),
            map_size_param("1024"),
            bias_param(-0.0001),
        ],
    )
}

#[must_use]
pub fn ambient_descriptor() -> NodeTypeDescriptor {
    assemble(
        "ambient_light",
        "Ambient Light",
        "A uniform fill light with no position or shadow; modulates the \
         scene ambient/IBL term.",
        &["light", "fill", "environment"],
        "ambient",
        "Ambient Light",
        vec![color_param("color", "Color", WHITE), intensity(0.5)],
    )
}

#[must_use]
pub fn hemisphere_descriptor() -> NodeTypeDescriptor {
    assemble(
        "hemisphere_light",
        "Hemisphere Light",
        "A two-color sky/ground ambient light.",
        &["light", "sky", "gradient"],
        "hemisphere",
        "Hemisphere Light",
        vec![
            color_param("sky_color", "Sky Color", WHITE),
            color_param("ground_color", "Ground Color", [0.267, 0.267, 0.267, 1.0]),
            intensity(1.0),
        ],
    )
}

#[must_use]
pub fn rect_area_descriptor() -> NodeTypeDescriptor {
    // v2 dropped `rotate` / `scale` / `uniform_scale`: the v1 soft
    // point-light approximation never read them, and keeping controls the
    // renderer ignores is a lie in the UI. They return with a real LTC
    // area-light model (backlog note).
    let mut desc = assemble(
        "rect_area_light",
        "Rect Area Light",
        "A rectangular area light (rendered as a soft point-light \
         approximation in v1).",
        &["light", "area", "softbox", "panel"],
        "rect_area",
        "Rect Area Light",
        vec![
            ParamSpec::new(
                "translate",
                "Translate",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([0.0; 3]),
            )
            .unit(Unit::Meters),
            color_param("color", "Color", WHITE),
            intensity(1.5),
            ParamSpec::new(
                "width",
                "Width",
                "light",
                ParamType::Float,
                ParamValue::Float(10.0),
            )
            .hard(0.1, 1000.0)
            .unit(Unit::Meters),
            ParamSpec::new(
                "height",
                "Height",
                "light",
                ParamType::Float,
                ParamValue::Float(10.0),
            )
            .hard(0.1, 1000.0)
            .unit(Unit::Meters),
        ],
    );
    desc.version = 2;
    desc.migrate = Some(super::common::migrate_strip_rect_area_transform);
    desc
}
