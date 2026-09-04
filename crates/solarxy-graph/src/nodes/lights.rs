//! The six light nodes: point,
//! directional, spot, ambient, hemisphere, rect-area.
//!
//! Lights are portless, root-context, `Mute`-bypassable, and cook
//! passively: their `LightDef` is resolved directly from their params by
//! the engine's scene builder (`engine::scene`), not carried on a wire.
//! Shadow-capable lights (point / directional / spot) carry `cast_shadow`
//! with exclusive-caster radio semantics enforced downstream.

use super::common::{general_params, passive_cook};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Pred, Unit};
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// The `visible` / `show_helper` / `helper_size` group every light carries.
/// The helper docs are passed per light: the shape differs by kind, and for
/// ambient (which draws nothing) and rect-area (which sizes itself from its
/// width and height) these controls are inert, which the doc has to say.
fn helper_group(show_helper_doc: &str, helper_size_doc: &str) -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "visible",
            "Visible",
            "rendering",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc(
            "Whether this light is in the scene at all. Off removes its \
             contribution and hides its helper, and releases its slot in the \
             interactive viewport's 8-light budget for the next light that \
             wants one.",
        ),
        ParamSpec::new(
            "show_helper",
            "Show Helper",
            "rendering",
            ParamType::Bool,
            ParamValue::Bool(false),
        )
        .doc(show_helper_doc),
        ParamSpec::new(
            "helper_size",
            "Helper Size",
            "rendering",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .hard(0.1, 10.0)
        .doc(helper_size_doc),
    ]
}

fn color_param(key: &str, label: &str, default: [f32; 4], doc: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        "light",
        ParamType::Color,
        ParamValue::Color(default),
    )
    .doc(doc)
}

/// `soft_max` is the slider's comfortable ceiling, passed per light rather
/// than shared because the six no longer sit on one scale. The four that
/// entered the raster loop had their defaults tripled when the hidden
/// multiplier left the shader; ambient and hemisphere never did. Passing
/// one number for all six would either squash their sliders into the
/// bottom sixth or leave the others unable to reach a useful value, so
/// each keeps the same range *relative to its own default* that it had
/// before.
fn intensity(default: f64, soft_max: f64) -> ParamSpec {
    ParamSpec::new(
        "intensity",
        "Intensity",
        "light",
        ParamType::Float,
        ParamValue::Float(default),
    )
    .hard(0.0, 1000.0)
    .soft(0.0, soft_max)
    .doc(
        "Linear multiplier on this light's contribution, and linear means \
         what it says: doubling this doubles the light, and two lights an \
         octave apart in this number are an octave apart on screen. 0 turns \
         the light off without removing it from the scene, which is the \
         quick way to A/B one you want to keep. There are still no lumens or \
         watts behind the number, so it is not calibrated against the \
         physical world, but it is consistent within a scene and against a \
         value authored anywhere else: nothing is scaled behind your back on \
         the way to the shader.",
    )
}

/// Shared by the three shadow-capable lights (identical group, type, and
/// default on each), so the exclusive-caster rule is stated once.
fn cast_shadow_param() -> ParamSpec {
    ParamSpec::new(
        "cast_shadow",
        "Cast Shadow",
        "light",
        ParamType::Bool,
        ParamValue::Bool(true),
    )
    .doc(
        "Whether this light renders the shadow map. Exactly one light in the \
         scene may cast at a time: switching this on here switches it off on \
         every other light, as a single undo step. Switching it off here \
         leaves the scene with no shadows until you grant it to another light.",
    )
}

/// Shared by point and spot (identical spec on both).
fn range_param() -> ParamSpec {
    ParamSpec::new(
        "range",
        "Range",
        "light",
        ParamType::Float,
        ParamValue::Float(0.0),
    )
    .hard(0.0, 100_000.0)
    .soft(0.0, 1000.0)
    .unit(Unit::Meters)
    .doc(
        "Distance at which the light's contribution reaches zero, in metres. \
         0, the default, means no cutoff at all: the light carries \
         infinitely far and only Decay dims it. Above 0 the falloff is \
         windowed so brightness arrives at exactly zero on the Range sphere, \
         which is how you stop a lamp from lighting the far side of a set. \
         Range and Decay are independent; both apply when both are set.",
    )
}

/// Shared by point and spot (identical spec on both).
fn decay_param() -> ParamSpec {
    ParamSpec::new(
        "decay",
        "Decay",
        "light",
        ParamType::Float,
        ParamValue::Float(2.0),
    )
    .hard(0.0, 10.0)
    .doc(
        "Falloff exponent: brightness is divided by the distance raised to \
         this power. The default 2 is physical inverse-square. 0 disables \
         decay outright, so the light is equally bright at any distance -- \
         handy for a flat fill, wrong for anything meant to read as a real \
         source. Values between 0 and 2 give the gentler falloff that is \
         often easier to light with than the physical answer.",
    )
}

/// Shared by point and spot: the emitter's physical size.
///
/// Only the path tracer reads it, and the doc says so rather than leaving a
/// control that does nothing in the viewport look broken. A shadow map is a
/// visibility test from a single place, so it has no way to be half in
/// shadow; softening it means blurring the map, which softens a contact
/// shadow and a distant one by the same amount and is a different effect
/// wearing this one's name.
fn radius_param() -> ParamSpec {
    ParamSpec::new(
        "radius",
        "Radius",
        "light",
        ParamType::Float,
        ParamValue::Float(0.0),
    )
    .hard(0.0, 1000.0)
    .soft(0.0, 5.0)
    .unit(Unit::Meters)
    .doc(
        "How big the emitter is, in metres. 0, the default, is a \
         mathematical point, which casts a shadow with a perfectly hard \
         edge -- the giveaway that a light is not a real object. Give it a \
         size and the shadow gains a penumbra that widens with distance \
         from the surface, the way a real lamp's does, because part of the \
         emitter is visible where the rest is hidden.\n\n\
         This is read by rendered output only; the interactive viewport \
         draws hard-edged shadows whatever you set here. That is not an \
         oversight to be fixed later: a shadow map answers one visibility \
         question from one place, and blurring it would soften a contact \
         shadow as much as a distant one.",
    )
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
    .doc(
        "The resolution the shadow map would render at, trading crisper \
         shadow edges against memory and fill cost. This control does \
         nothing today: the shadow map size is fixed by the host (2048 in \
         the web app) and nothing reads this value. It is resolved and saved \
         with the document, waiting on the per-light shadow work, so setting \
         it now changes neither the image nor performance.",
    )
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
    .doc(
        "Depth offset applied when testing against the shadow map, the usual \
         dial for trading shadow acne against peter-panning. This control \
         does nothing today: the shader hardcodes one bias for every caster \
         and nothing reads this value. Dragging it will not change the \
         image; if you are fighting acne, the bias is not currently yours to \
         tune.",
    )
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
    specific: Vec<ParamSpec>,
    helper_docs: (&str, &str),
) -> NodeTypeDescriptor {
    // Every light's `name` default is its display name, so the two were
    // always passed the same string; `general_params` takes it from here.
    let mut params = general_params(display_name);
    params.extend(specific);
    params.extend(helper_group(helper_docs.0, helper_docs.1));
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
    let mut desc = assemble(
        "point_light",
        "Point Light",
        "An omnidirectional light: it emits from Position equally in every \
         direction, dimming with distance according to Range and Decay.\n\n\
         The workhorse for a local source -- a bulb, a candle, a muzzle \
         flash. Drop it in the root graph beside your `geo` containers; it \
         takes no wires, because the scene builder reads its params straight \
         off the node rather than passing a light down a chain. Reach for \
         `directional_light` when you want a sun instead, or `spot_light` \
         when you want the same falloff inside a cone.\n\n\
         Two limits bite. It spends one of the 8 direct-light slots the \
         interactive viewport binds, and past 8 the first 8 in document \
         order win with the rest dropped silently, so a scene that quietly \
         stops responding to new lights is probably at that cap (ambient \
         and hemisphere lights are free, and an invisible light gives its \
         slot back). That ceiling is the viewport's, not the scene's -- \
         rendered output reads every light. And shadow casting is \
         exclusive: switching Cast Shadow on here switches it off on every \
         other light, in one undo step.",
        &["light", "omni", "bulb"],
        "point",
        vec![
            ParamSpec::new(
                "position",
                "Position",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([10.0, 10.0, 5.0]),
            )
            .unit(Unit::Meters)
            .doc(
                "Where the light sits, in metres. Everything radiates from \
                 here: Range and Decay measure their distance from this \
                 point, and the helper sphere is drawn around it.",
            ),
            color_param(
                "color",
                "Color",
                WHITE,
                "The light's color, linear RGB. It multiplies the surface \
                 color, so a saturated light cannot put back a hue the \
                 surface does not reflect. Alpha is ignored. Tint this \
                 rather than Intensity when you want warmth, not brightness.",
            ),
            intensity(4.5, 30.0),
            range_param(),
            decay_param(),
            radius_param(),
            cast_shadow_param(),
            map_size_param("1024"),
            bias_param(-0.0001),
        ],
        (
            "Draw a wireframe sphere at Position, so you can see where the \
             light is without hunting for it. The helper is drawn in the \
             light's own color, and is hidden whenever Visible is off.",
            "How big the helper sphere is drawn, in world metres. Purely \
             cosmetic -- it has no effect on the light itself. Raise it when \
             the helper is lost in a large scene.",
        ),
    );
    // v2 rescales the stored intensity: the raster path stopped multiplying
    // every light's contribution by three, so a value saved before that has
    // to move by the same factor to mean what it did. Ambient and
    // hemisphere deliberately do NOT bump, because they fold into the
    // hemisphere rows of the light uniform and never entered that loop.
    //
    // v3 adds Radius. A pure addition whose default is the identity of its
    // effect -- a zero radius is the point emitter every earlier scene
    // already had -- so it needs no arm of its own, and the hook stays the
    // one v2 installed because it already keys on the version it came from.
    desc.version = 3;
    desc.migrate = Some(super::common::migrate_scale_intensity);
    desc
}

#[must_use]
pub fn directional_descriptor() -> NodeTypeDescriptor {
    let mut desc = assemble(
        "directional_light",
        "Directional Light",
        "A parallel light, like the sun: every ray travels the same \
         direction, so nothing is nearer to it and nothing falls off with \
         distance. Only the direction from Position to Target matters.\n\n\
         The key light for most scenes -- sun, moon, a large window. Aim it \
         by moving Target rather than Position, and pair it with a \
         `hemisphere_light` or `ambient_light` to lift the shadow side, \
         because a directional light on its own leaves every face turned \
         away from it black.\n\n\
         Its Position lights nothing. The shading uses only the \
         Position-to-Target direction, and the shadow frustum auto-fits the \
         scene bounds instead of sitting at Position, so moving the node in \
         space moves nothing but its helper arrow. It spends one of the 8 \
         direct-light slots the interactive viewport binds -- that ceiling is \
         the viewport's, not the scene's -- and Cast Shadow is exclusive: \
         granting it here revokes it from every other light in a single undo \
         step.",
        &["light", "sun", "sky"],
        "directional",
        vec![
            ParamSpec::new(
                "position",
                "Position",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([10.0, 10.0, 5.0]),
            )
            .unit(Unit::Meters)
            .doc(
                "Where the helper arrow is drawn, in metres, and the tail of \
                 the aiming vector. The shading ignores it: rays are \
                 parallel, so only the direction toward Target counts, and \
                 the shadow frustum fits itself to the scene rather than to \
                 this point. Move it to park the helper somewhere readable; \
                 the lighting will not change.",
            ),
            ParamSpec::new(
                "target",
                "Target",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([0.0; 3]),
            )
            .unit(Unit::Meters)
            .doc(
                "The point the light aims at, in metres. Only the direction \
                 from Position to Target is used, so the distance between \
                 them is irrelevant -- this is a rotation control wearing \
                 XYZ clothes. If Target and Position coincide the light \
                 falls back to pointing straight down.",
            ),
            color_param(
                "color",
                "Color",
                WHITE,
                "The light's color, linear RGB. It multiplies the surface \
                 color, so a saturated light cannot put back a hue the \
                 surface does not reflect. Alpha is ignored. A slightly warm \
                 sun against a cool `hemisphere_light` fill is the cheapest \
                 believable daylight there is.",
            ),
            intensity(4.5, 30.0),
            cast_shadow_param(),
            map_size_param("2048"),
            bias_param(0.0001),
        ],
        (
            "Draw a wireframe arrow at Position pointing toward Target. \
             Worth turning on while aiming: the direction is the only thing \
             this light actually uses, and the arrow is the only way to see \
             it. Hidden whenever Visible is off.",
            "How long the helper arrow is drawn, in world metres. Purely \
             cosmetic -- it has no effect on the light.",
        ),
    );
    // v2 rescales the stored intensity: the raster path stopped multiplying
    // every light's contribution by three, so a value saved before that has
    // to move by the same factor to mean what it did. Ambient and
    // hemisphere deliberately do NOT bump, because they fold into the
    // hemisphere rows of the light uniform and never entered that loop.
    desc.version = 2;
    desc.migrate = Some(super::common::migrate_scale_intensity);
    desc
}

#[must_use]
pub fn spot_descriptor() -> NodeTypeDescriptor {
    let mut desc = assemble(
        "spot_light",
        "Spot Light",
        "A cone of light from Position toward Target: full intensity in the \
         middle, nothing past the outer Angle, and a Penumbra that controls \
         how abruptly it gets there.\n\n\
         The pick for a deliberate pool of light -- a lamp, a torch, a stage \
         special. It falls off with distance exactly like a `point_light` \
         and shares its Range and Decay, so think of it as a point light \
         wearing a cone; use the point light when you do not need the cone.\n\n\
         Penumbra defaults to 0, which gives a razor-hard cone edge -- the \
         classic tell of a CG spotlight, and rarely what you want; a little \
         goes a long way. Angle is the HALF-angle, so the default 45 spreads \
         90 degrees in total. The light spends one of the 8 direct-light \
         slots the interactive viewport binds -- that ceiling is the \
         viewport's, not the scene's -- and Cast Shadow is exclusive: granting \
         it here revokes it from every other light in a single undo step.",
        &["light", "cone", "flashlight"],
        "spot",
        vec![
            ParamSpec::new(
                "position",
                "Position",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([10.0, 10.0, 5.0]),
            )
            .unit(Unit::Meters)
            .doc(
                "The apex of the cone, in metres: where the light sits and \
                 emits from. Range and Decay measure distance from this \
                 point, and the helper cone is drawn from it toward Target.",
            ),
            ParamSpec::new(
                "target",
                "Target",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([0.0; 3]),
            )
            .unit(Unit::Meters)
            .doc(
                "The point the cone aims at, in metres. Only the direction \
                 from Position is used, so moving Target further away aims \
                 the light without lengthening its reach -- that is Range. \
                 If Target and Position coincide the light points straight \
                 down.",
            ),
            color_param(
                "color",
                "Color",
                WHITE,
                "The light's color, linear RGB. It multiplies the surface \
                 color, so a saturated light cannot put back a hue the \
                 surface does not reflect. Alpha is ignored.",
            ),
            intensity(4.5, 30.0),
            range_param(),
            decay_param(),
            ParamSpec::new(
                "angle",
                "Angle",
                "light",
                ParamType::Float,
                ParamValue::Float(45.0),
            )
            .hard(1.0, 89.0)
            .unit(Unit::Degrees)
            .doc(
                "Half-angle of the cone's outer edge, in degrees: the full \
                 spread is twice this, so the default 45 is a 90-degree \
                 cone. Nothing outside the angle receives any light. It also \
                 sets the width of the helper cone.",
            ),
            ParamSpec::new(
                "penumbra",
                "Penumbra",
                "light",
                ParamType::Float,
                ParamValue::Float(0.0),
            )
            .hard(0.0, 1.0)
            .unit(Unit::Normalized)
            .doc(
                "How soft the cone edge is, 0 to 1. 0, the default, is a \
                 hard edge: full intensity right up to Angle, then nothing. \
                 Raising it shrinks the full-intensity inner cone toward the \
                 centre -- the inner half-angle is Angle * (1 - Penumbra) -- \
                 and fades across the gap, so 1 spreads the falloff over the \
                 whole cone and leaves no flat core at all.",
            ),
            radius_param(),
            cast_shadow_param(),
            map_size_param("1024"),
            bias_param(-0.0001),
        ],
        (
            "Draw a wireframe cone from Position along the aim, at the outer \
             Angle, plus a dimmer inner circle when Penumbra has opened a \
             gap worth seeing. The cone is drawn out to Range when it has \
             one, so the helper shows where the light actually stops.",
            "How long the helper cone is drawn, in world metres, when Range \
             is 0 (an unbounded light has no natural length to draw). Once \
             Range is set the cone is drawn out to Range instead and this \
             does nothing. Cosmetic either way.",
        ),
    );
    // v2 rescales the stored intensity: the raster path stopped multiplying
    // every light's contribution by three, so a value saved before that has
    // to move by the same factor to mean what it did. Ambient and
    // hemisphere deliberately do NOT bump, because they fold into the
    // hemisphere rows of the light uniform and never entered that loop.
    //
    // v3 adds Radius, on the same reasoning as the point light's: a pure
    // addition whose default is the identity of its effect.
    desc.version = 3;
    desc.migrate = Some(super::common::migrate_scale_intensity);
    desc
}

#[must_use]
pub fn ambient_descriptor() -> NodeTypeDescriptor {
    assemble(
        "ambient_light",
        "Ambient Light",
        "A uniform fill: it adds the same light to every surface, whatever \
         its position and whichever way it faces. No position, no direction, \
         no shadow, no falloff.\n\n\
         The blunt instrument for lifting shadows that a key light left too \
         dark. `hemisphere_light` is usually the better answer -- it costs \
         the same and at least varies from sky to ground -- so reach for \
         ambient when you specifically want flatness, or want a quick global \
         lift while blocking out a scene.\n\n\
         It costs no light slot: ambient and hemisphere lights fold into the \
         ambient term instead of competing for the interactive viewport's 8 \
         direct-light slots, so stack as many as you like. Two honest limits: it ADDS to the IBL \
         environment rather than scaling it, so it cannot dim an HDRI, and \
         ambient occlusion still darkens it, so it will not fully flatten \
         creases. Show Helper and Helper Size do nothing here -- with no \
         position and no direction there is no honest shape to draw.",
        &["light", "fill", "environment"],
        "ambient",
        vec![
            color_param(
                "color",
                "Color",
                WHITE,
                "The fill color, linear RGB, multiplied by Intensity and \
                 added to every surface equally. It multiplies the surface \
                 color like any other light. Keep it dim and slightly tinted \
                 -- a bright neutral ambient is what makes a render look \
                 washed out and unlit. Alpha is ignored.",
            ),
            intensity(0.5, 10.0),
        ],
        (
            "This control does nothing on an ambient light. An ambient light \
             has no position and no direction, so there is no shape to draw \
             and no place to draw it; the viewport shows nothing however \
             this is set.",
            "This control does nothing on an ambient light: there is no \
             helper to size.",
        ),
    )
}

#[must_use]
pub fn hemisphere_descriptor() -> NodeTypeDescriptor {
    assemble(
        "hemisphere_light",
        "Hemisphere Light",
        "A two-tone ambient: Sky Color from above, Ground Color from below, \
         blended across each surface by how far its normal tilts up or \
         down. No position, no direction, no shadow.\n\n\
         The default choice for fill, and a cheap stand-in for a real \
         environment: a cool sky over a warm ground reads as outdoors \
         without loading an HDRI. It sits under a `directional_light` key in \
         most rigs. Prefer it to `ambient_light`, which costs exactly the \
         same and gives none of the variation.\n\n\
         It costs no light slot -- ambient and hemisphere lights fold into \
         the ambient term rather than taking one of the 8 direct-light \
         slots. The blend is decided purely by the surface normal, so it is \
         a gradient in ORIENTATION, not in space: a floor at the top of your \
         scene still gets Sky Color, and nothing occludes the light except \
         ambient occlusion. Having no position, its helper dome always draws \
         at the world origin no matter where the scene sits.",
        &["light", "sky", "gradient"],
        "hemisphere",
        vec![
            color_param(
                "sky_color",
                "Sky Color",
                WHITE,
                "Linear RGB reaching surfaces that face up; multiplied by \
                 Intensity. This is the dominant half in practice, because \
                 most of what you light -- floors, shoulders, the tops of \
                 things -- faces up. Alpha is ignored.",
            ),
            color_param(
                "ground_color",
                "Ground Color",
                [0.267, 0.267, 0.267, 1.0],
                "Linear RGB reaching surfaces that face down; multiplied by \
                 Intensity. Read it as bounce off the floor, and tint it \
                 toward whatever the floor is made of. Setting it equal to \
                 Sky Color makes this light exactly an `ambient_light`. \
                 Alpha is ignored.",
            ),
            intensity(1.0, 10.0),
        ],
        (
            "Draw a wireframe dome, in Sky Color, at the WORLD ORIGIN -- a \
             hemisphere light has no position, so the dome cannot follow \
             your scene. It is an indicator that the light exists, not a \
             picture of where it is.",
            "How big the helper dome is drawn, in world metres. Purely \
             cosmetic -- it has no effect on the light.",
        ),
    )
}

#[must_use]
pub fn rect_area_descriptor() -> NodeTypeDescriptor {
    // v3 restores `rotate` (v2 dropped it, along with `scale` and
    // `uniform_scale`, because the v1 point-light approximation read none
    // of them). Scale stays gone for good: Width and Height already say how
    // big the panel is, and a second way to say it is a second thing to
    // disagree.
    let mut desc = assemble(
        "rect_area_light",
        "Rect Area Light",
        "A rectangular emitter -- the softbox of the light kit -- with a \
         Width and Height, sitting at Translate and facing straight down \
         until you rotate it.\n\n\
         What you reach for it for: soft key and fill on a product or \
         character shot, and the broad specular roll-off a panel gives that \
         a pinpoint source cannot. Widen it and the highlight spreads and \
         the terminator softens; turn it edge-on and it dims, because you \
         are looking at less of it.\n\n\
         The shading integrates over the whole rectangle (linearly \
         transformed cosines), so Width, Height and Rotate all reach the \
         image rather than only the helper. Two caveats remain. It cannot \
         cast shadows -- it has no Cast Shadow param, and the exclusive \
         shadow caster stays with the punctual lights -- so it lights \
         through geometry. And Helper Size is ignored, because the helper \
         rectangle takes its size from Width and Height. It spends one of \
         the 8 direct-light slots the interactive viewport binds, like any \
         other.",
        &["light", "area", "softbox", "panel"],
        "rect_area",
        vec![
            ParamSpec::new(
                "translate",
                "Translate",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([0.0; 3]),
            )
            .unit(Unit::Meters)
            .doc(
                "The centre of the rectangle, in metres. Unrotated, the \
                 panel lies flat with its Width along X, its Height along \
                 Z, and its emitting face pointing straight down.",
            ),
            color_param(
                "color",
                "Color",
                WHITE,
                "The panel's color, linear RGB, multiplied by Intensity. It \
                 multiplies the surface color like any other light. Alpha is \
                 ignored.",
            ),
            intensity(4.5, 30.0),
            ParamSpec::new(
                "width",
                "Width",
                "light",
                ParamType::Float,
                ParamValue::Float(10.0),
            )
            .hard(0.1, 1000.0)
            .unit(Unit::Meters)
            .doc(
                "One edge length of the emitting rectangle, in metres, \
                 along the panel's local X. It reaches the shading: a wider \
                 panel spreads the specular highlight and softens the \
                 terminator, and emits more total light, because a bigger \
                 emitter is a brighter one at the same intensity.",
            ),
            ParamSpec::new(
                "height",
                "Height",
                "light",
                ParamType::Float,
                ParamValue::Float(10.0),
            )
            .hard(0.1, 1000.0)
            .unit(Unit::Meters)
            .doc(
                "The other edge length of the emitting rectangle, in \
                 metres, along the panel's local Z. Setting it far from \
                 Width gives the long thin source a strip light makes, \
                 which stretches a highlight along one axis only.",
            ),
            ParamSpec::new(
                "rotate",
                "Rotate",
                "transform",
                ParamType::Vec3,
                ParamValue::Vec3([0.0; 3]),
            )
            .unit(Unit::Degrees)
            .doc(
                "Euler angles in degrees, composed in XYZ order, turning \
                 the panel away from face-down. Rotating about Y is the one \
                 that matters for a square panel; for a rectangular one it \
                 also decides which way the long edge runs, which is the \
                 difference between a strip light lying down and standing \
                 up.",
            ),
            ParamSpec::new(
                "two_sided",
                "Two Sided",
                "light",
                ParamType::Bool,
                ParamValue::Bool(false),
            )
            .doc(
                "Emit from the back face as well as the front. Off, the \
                 panel lights only what it faces and anything behind it is \
                 untouched, which is what a real softbox does. On, it \
                 behaves like a floating pane of light, which is useful for \
                 filling a room from a plane in its middle without placing \
                 two lights.",
            ),
        ],
        (
            "Draw the emitting rectangle at Translate, sized by Width and \
             Height, with a short stub along its normal so the emitting side \
             is unambiguous. Worth turning on: it is the only place Width \
             and Height have any visible effect at all.",
            "This control does nothing on a rect-area light. The helper \
             rectangle takes its size from Width and Height instead.",
        ),
    );
    // v4 rescales the stored intensity, the same change the other three
    // slot-consuming lights take at v2. This one is at a different number
    // only because it had already bumped twice for unrelated reasons,
    // which is why the rescale could not be one shared hook on the
    // descriptor builder the way the specification assumed.
    desc.version = 4;
    // The v1 arm is unchanged on purpose. It strips a v1 document's
    // `rotate`, `scale` and `uniform_scale`, and every later version must
    // keep stripping them: a v1 `rotate` was authored against a light that
    // ignored it, so carrying it forward would silently re-aim panels in
    // old scenes. v3's own `rotate` and `two_sided` need no arm at all --
    // an unset param resolves to its spec default, which is the
    // face-down, single-sided panel v2 documents already describe.
    desc.migrate = Some(super::common::migrate_strip_rect_area_transform);
    desc
}
