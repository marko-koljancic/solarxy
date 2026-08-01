//! The `camera` root node: a viewport
//! camera the user authors in the graph and looks through per pane.
//!
//! Portless, root-context, `Mute`-bypassable: its `CameraDef` is resolved
//! directly from its params by the engine's scene builder (`engine::scene`),
//! exactly like a light's `LightDef`, never carried on a wire. Three kinds
//! (perspective / orthographic / physical); physical derives its FOV from a
//! focal length and sensor width. Not a geometry node: no ports, no material.
//! The render/turntable-export consumer the catalog's camera deferral was
//! waiting on now exists.
//!
//! It also owns the shot's **look**: exposure, tone mapper, lift/gamma/gain,
//! and two colour-grading LUT slots. That is why it has a cook at all, having
//! been passive until 0.8.2. A `.cube` table has to be decoded somewhere, and
//! the cook is the right somewhere: it runs when the node changes rather than
//! when a delta is built, and the driver caches the result per node. The
//! decoded tables reach `CameraDef` through the cook's side channel, the same
//! route the environment node's HDRI takes and for the same reason, which is
//! that neither has a wire to travel on.

use std::sync::Arc;

use super::common::general_params;
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Pred, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

fn kind_is(variant: &str) -> Pred {
    Pred::Eq(ParamValue::Enum(variant.to_string()))
}

/// The `tone` variant meaning "leave the pane's own tone mapper alone".
/// The default, so adding a camera to an existing scene never restyles it.
pub const TONE_INHERIT: &str = "inherit";

/// The camera's `look` group: exposure, tone, grade, and the two lookup
/// table slots.
///
/// Grouped under one tab with subgroups rather than a tab each, for the
/// reason the material node's layout records: the tab strip is a single
/// non-wrapping row, and three more tabs overflow it.
fn look_params() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "exposure",
            "Exposure",
            "look",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .subgroup("Tone")
        .hard(0.01, 64.0)
        .soft(0.1, 8.0)
        .doc(
            "Linear multiplier on the whole image before tone mapping, so 2 \
             is one stop brighter and 0.5 one stop darker. This is the first \
             control to reach for when a render is broadly too dark or too \
             bright, ahead of touching light intensities, because it moves \
             the exposure of the shot rather than the lighting of the scene.",
        ),
        ParamSpec::new(
            "tone",
            "Tone Map",
            "look",
            ParamType::Enum {
                variants: vec![
                    EnumVariant::new(TONE_INHERIT, "Inherit from pane"),
                    EnumVariant::new("none", "None (clip)"),
                    EnumVariant::new("linear", "Linear"),
                    EnumVariant::new("reinhard", "Reinhard"),
                    EnumVariant::new("aces", "ACES Filmic"),
                ],
            },
            ParamValue::Enum(TONE_INHERIT.to_string()),
        )
        .subgroup("Tone")
        .doc(
            "How high dynamic range is brought down to what a screen can \
             show. Inherit leaves the pane's own choice alone, which is the \
             default so that adding a camera never silently restyles a scene. \
             Set it to None when the Pre-Tonemap LUT below carries a full \
             tone transform such as ACES or AgX, because applying both would \
             tone map the image twice.",
        ),
        ParamSpec::new(
            "lift",
            "Lift",
            "look",
            ParamType::Vec3,
            ParamValue::Vec3([0.0; 3]),
        )
        .subgroup("Grade")
        .hard(-1.0, 1.0)
        .soft(-0.25, 0.25)
        .doc(
            "Raises or lowers the darkest part of the image, per channel, \
             after tone mapping. Positive values lift the blacks towards grey \
             for a faded or filmic base; negative values crush them. Because \
             it is an addition rather than a multiplication it moves the \
             shadows far more than the highlights, which is what separates it \
             from Gain.",
        ),
        ParamSpec::new(
            "gamma",
            "Gamma",
            "look",
            ParamType::Vec3,
            ParamValue::Vec3([1.0; 3]),
        )
        .subgroup("Grade")
        .hard(0.01, 10.0)
        .soft(0.4, 2.5)
        .doc(
            "Bends the midtones per channel without moving black or white: \
             above 1 brightens them, below 1 darkens them. This is the \
             control for an image whose ends are right and whose middle is \
             not, and the one to reach for when a colour cast sits in the \
             midtones rather than across the whole frame. 1 is neutral.",
        ),
        ParamSpec::new(
            "gain",
            "Gain",
            "look",
            ParamType::Vec3,
            ParamValue::Vec3([1.0; 3]),
        )
        .subgroup("Grade")
        .hard(0.0, 10.0)
        .soft(0.0, 3.0)
        .doc(
            "Multiplies each channel, which moves the highlights most and \
             leaves black at black. Use it to set the white point or to warm \
             and cool an image by pushing the red and blue channels apart. \
             1 is neutral on every channel.",
        ),
        ParamSpec::new(
            "lut_a",
            "Pre-Tonemap LUT",
            "look",
            ParamType::AssetRef {
                accept: [".cube"].iter().map(ToString::to_string).collect(),
            },
            ParamValue::Asset(crate::params::AssetId(String::new())),
        )
        .subgroup("Lookup tables")
        .doc(
            "A `.cube` table applied BEFORE tone mapping, on log-encoded \
             scene light. This is the slot for a full tone transform such as \
             ACES or AgX, which replaces the tone mapper rather than \
             decorating it, so set Tone Map to None when you load one here. \
             An ordinary look LUT belongs in the slot below and will look \
             wrong in this one.",
        ),
        ParamSpec::new(
            "lut_a_strength",
            "Pre-Tonemap Amount",
            "look",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .subgroup("Lookup tables")
        .hard(0.0, 1.0)
        .doc(
            "How far to blend towards the pre-tonemap table, from 0 for none \
             of it to 1 for all of it. Mostly useful for checking what the \
             table is doing by sliding it off and on; a tone transform is \
             usually wanted at full strength.",
        ),
        ParamSpec::new(
            "lut_b",
            "Look LUT",
            "look",
            ParamType::AssetRef {
                accept: [".cube"].iter().map(ToString::to_string).collect(),
            },
            ParamValue::Asset(crate::params::AssetId(String::new())),
        )
        .subgroup("Lookup tables")
        .doc(
            "A `.cube` table applied AFTER tone mapping, on the finished \
             image. This is the slot for the look LUTs people already own \
             from a grading suite, which are authored against a \
             display-referred picture. A tone transform belongs in the slot \
             above and will look wrong in this one.",
        ),
        ParamSpec::new(
            "lut_b_strength",
            "Look Amount",
            "look",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .subgroup("Lookup tables")
        .hard(0.0, 1.0)
        .doc(
            "How far to blend towards the look table, from 0 for none of it \
             to 1 for all of it. Unlike the pre-tonemap slot this one is \
             routinely dialled back: a look at half strength is a common way \
             to keep its character without its full contrast.",
        ),
    ]
}

#[must_use]
pub fn camera_descriptor() -> NodeTypeDescriptor {
    let mut params = general_params("Camera");
    params.extend(vec![
        // Transform: an orbit look-at (eye + target), like the light position/target.
        ParamSpec::new(
            "position",
            "Position",
            "transform",
            ParamType::Vec3,
            ParamValue::Vec3([7.0, 5.0, 7.0]),
        )
        .unit(Unit::Meters)
        .doc(
            "The eye, in metres: where the camera sits. With Target it fully \
             determines the view -- there is no roll control, because up is \
             always world Y, so a camera authored here can never be dutched.",
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
            "The point the camera aims at, in metres. Position and Target \
             together give both the aim and the distance, so on a \
             perspective camera moving either one reframes the shot; on an \
             orthographic camera only the aim matters and Ortho Scale does \
             the framing.",
        ),
        // Lens: the kind selects which of fov / focal length / ortho scale applies.
        ParamSpec::new(
            "kind",
            "Projection",
            "lens",
            ParamType::Enum {
                variants: vec![
                    EnumVariant::new("perspective", "Perspective"),
                    EnumVariant::new("orthographic", "Orthographic"),
                    EnumVariant::new("physical", "Physical (lens)"),
                ],
            },
            ParamValue::Enum("perspective".to_string()),
        )
        .doc(
            "Which projection the camera uses, and therefore which lens \
             control applies. Perspective takes Field of View directly. \
             Physical (lens) computes that same field of view from Focal \
             Length and Sensor Width, for when you would rather think in \
             millimetres. Orthographic drops perspective altogether and \
             frames by Ortho Scale, keeping parallel lines parallel for an \
             elevation or a technical view. Switching this only hides and \
             shows the controls below; the values you set are kept, so you \
             can flip back and forth without losing a lens.",
        ),
        ParamSpec::new(
            "fov_y",
            "Field of View",
            "lens",
            ParamType::Float,
            ParamValue::Float(45.0),
        )
        .hard(1.0, 179.0)
        .soft(10.0, 120.0)
        .unit(Unit::Degrees)
        .show_if("kind", kind_is("perspective"))
        .doc(
            "The VERTICAL angle the camera sees, in degrees. Smaller is \
             tighter and flatter, larger is wider and more distorted at the \
             edges; past about 90 the stretch in the corners gets hard to \
             miss. Perspective only -- a physical camera derives this same \
             number from Focal Length and Sensor Width instead, and an \
             orthographic camera has no field of view at all.",
        ),
        ParamSpec::new(
            "focal_length",
            "Focal Length (mm)",
            "lens",
            ParamType::Float,
            ParamValue::Float(50.0),
        )
        .hard(1.0, 2000.0)
        .soft(14.0, 300.0)
        .show_if("kind", kind_is("physical"))
        .doc(
            "The lens focal length in millimetres, as a photographer means \
             it: 50 normal, 24 wide, 200 a long telephoto. It only means \
             something against a sensor size -- together with Sensor Width \
             it sets the field of view, as fov = 2 * atan(sensor / (2 * \
             focal)), so a LONGER focal length gives a NARROWER view. \
             Physical projection only. See Sensor Width for how that formula \
             differs from a real camera's.",
        ),
        ParamSpec::new(
            "sensor_width",
            "Sensor Width (mm)",
            "lens",
            ParamType::Float,
            ParamValue::Float(36.0),
        )
        .hard(1.0, 100.0)
        .soft(10.0, 70.0)
        .show_if("kind", kind_is("physical"))
        .doc(
            "The film-back width in millimetres: 36 is full-frame 35mm, 25 \
             is Super 35, 23.5 is APS-C. A larger sensor at the same Focal \
             Length sees more, which is why the same lens is wide on \
             full-frame and long on a phone. One caveat worth knowing: this \
             value drives the VERTICAL field of view, where a real camera \
             would use its sensor HEIGHT. A 50mm at 36mm here frames about \
             40 degrees tall; a real full-frame camera, working from its \
             24mm sensor height, frames about 27. So a physical camera reads \
             wider than the equivalent real lens -- compensate with a longer \
             Focal Length or a smaller Sensor Width.",
        ),
        ParamSpec::new(
            "ortho_scale",
            "Ortho Scale",
            "lens",
            ParamType::Float,
            ParamValue::Float(5.0),
        )
        .hard(0.001, 100_000.0)
        .soft(0.1, 100.0)
        .unit(Unit::Meters)
        .show_if("kind", kind_is("orthographic"))
        .doc(
            "Half the visible height, in metres: at the default 5 the camera \
             frames 10 metres top to bottom, and the width follows from the \
             pane's shape. This is the orthographic stand-in for zoom. With \
             no perspective, moving the camera closer changes nothing about \
             the framing, so this is the only way to fit more or less in \
             frame. Orthographic only.",
        ),
        ParamSpec::new(
            "near",
            "Near Clip",
            "lens",
            ParamType::Float,
            ParamValue::Float(0.1),
        )
        .hard(0.0001, 100_000.0)
        .soft(0.01, 10.0)
        .unit(Unit::Meters)
        .doc(
            "How close a surface may come before it is clipped away, in \
             metres. This control does nothing today: a pane looking through \
             this camera takes its position, aim, field of view and \
             projection, but derives its own near and far planes from the \
             orbit distance and never reads this value. It is resolved and \
             saved with the document; it will not change the image.",
        ),
        ParamSpec::new(
            "far",
            "Far Clip",
            "lens",
            ParamType::Float,
            ParamValue::Float(1000.0),
        )
        .hard(0.001, 1_000_000.0)
        .soft(10.0, 10_000.0)
        .unit(Unit::Meters)
        .doc(
            "How far a surface may be before it is clipped away, in metres. \
             Like Near Clip, this control does nothing today -- the pane \
             derives its own clip planes from the orbit distance and ignores \
             this value.",
        ),
        // Framing: the aspect drives the viewport gate and the default export aspect.
        ParamSpec::new(
            "aspect",
            "Aspect (W/H)",
            "framing",
            ParamType::Float,
            ParamValue::Float(16.0 / 9.0),
        )
        .hard(0.1, 10.0)
        .soft(0.5, 3.0)
        .doc(
            "The framing aspect, width over height. It does not change what \
             the camera sees -- the pane's own shape does that -- it draws \
             the framing gate: a rectangle inset in any pane locked to this \
             camera, marking what a render at this aspect would keep. It \
             also sets the shape of the film back on the camera gizmo. 16/9 \
             by default.",
        ),
        // Gizmo: the wireframe frustum, mirroring the light helper pair.
        ParamSpec::new(
            "show_gizmo",
            "Show Gizmo",
            "gizmo",
            ParamType::Bool,
            ParamValue::Bool(true),
        )
        .doc(
            "Draw this camera's wireframe frustum in the viewport: the film \
             back at its Aspect, four edges converging on the eye, and a \
             wedge marking which way is up. A pane looking through this \
             camera never draws its own gizmo, the way Blender hides the \
             camera you are inside, so this is about seeing the camera from \
             other panes.",
        ),
        ParamSpec::new(
            "gizmo_size",
            "Gizmo Size",
            "gizmo",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .hard(0.1, 10.0)
        .doc(
            "How big the wireframe frustum is drawn, in world metres. Purely \
             cosmetic: it has no effect on what the camera sees or renders. \
             Raise it when the gizmo is lost in a large scene, lower it when \
             it swamps a small one.",
        ),
    ]);
    params.extend(look_params());

    NodeTypeDescriptor {
        type_id: "camera",
        // v2 added the `look` group. A pure addition: every new param fills
        // from its registry default and every default is the identity of
        // its effect (exposure 1, neutral grade, no table, tone inherited),
        // so a v1 camera renders exactly as it did and needs no migration
        // hook.
        version: 2,
        display_name: "Camera",
        category: Category::Cameras,
        contexts: ContextSet::OBJ,
        opens: None,
        inputs: vec![],
        outputs: vec![],
        params,
        bypass: BypassBehavior::Mute,
        doc: "A camera you can look through, pane by pane: an eye at \
              Position aimed at Target, projecting as perspective, \
              orthographic, or physical (lens).\n\n\
              Author a shot here instead of leaving it in a viewport you \
              will orbit away from. Lock a pane to the camera, frame it, and \
              screenshots and turntable exports through that pane use it. \
              Like the light nodes it is portless and lives in the root \
              graph beside your `geo` containers: the scene builder reads \
              its params directly, so there is no wire to connect and \
              nothing downstream of it.\n\n\
              Near Clip and Far Clip currently do nothing -- a pane takes \
              this camera's position, aim, field of view and projection, but \
              derives its own clip planes from the orbit distance. The lens \
              controls are mutually exclusive by Projection (Field of View \
              for perspective, Focal Length and Sensor Width for physical, \
              Ortho Scale for orthographic), so a control you are looking \
              for and cannot find is usually hidden behind a different \
              Projection. Up is always world Y: there is no roll.",
        search_aliases: &["camera", "view", "cam", "lens"],
        glyph: "camera",
        role: NodeRole::Camera,
        cook: cook_camera,
        migrate: None,
    }
}

/// Decodes whatever `.cube` tables the two look slots point at.
///
/// The camera is still passive in the sense that matters: it produces no
/// output value and nothing downstream reads it. This exists only because
/// a table has to be decoded somewhere, and the cook is the right
/// somewhere: it runs when the node changes rather than when a delta is
/// built, and the driver caches the result per node. Decoding in the
/// lowering instead would re-parse most of a megabyte of text on every
/// frame that touched the scene.
///
/// Both slots decode inline on the web path as well as natively, unlike
/// the environment's HDRI which parks on a worker job. A `.cube` is small
/// ASCII and parses in well under a millisecond, so a job would cost a
/// round trip to save nothing.
fn cook_camera(
    p: &ResolvedParams,
    _in: &Inputs,
    cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    for (slot, key) in [(0usize, "lut_a"), (1usize, "lut_b")] {
        let Some(asset) = p.asset(key) else {
            continue;
        };
        let entry = cx.assets.get(asset).ok_or_else(|| CookError::Failed {
            message: format!("the table referenced by {key} is not staged"),
        })?;
        let table = solarxy_formats::lut::decode_cube_bytes(&entry.bytes).map_err(|e| {
            CookError::Failed {
                message: format!("{}: {e}", entry.name),
            }
        })?;
        cx.set_lut(slot, Arc::new(table));
    }
    Ok(CookOutcome::Done(Outputs::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_a_portless_root_node() {
        let d = camera_descriptor();
        assert_eq!(d.type_id, "camera");
        assert!(d.contexts.contains(crate::document::ContextKind::Obj));
        assert!(d.inputs.is_empty() && d.outputs.is_empty());
    }

    #[test]
    fn lens_params_are_conditional_on_kind() {
        let d = camera_descriptor();
        let fov = d.params.iter().find(|p| p.key == "fov_y").unwrap();
        assert!(
            fov.show_if
                .iter()
                .any(|s| s.param == "kind" && s.pred == kind_is("perspective"))
        );
        let focal = d.params.iter().find(|p| p.key == "focal_length").unwrap();
        assert!(
            focal
                .show_if
                .iter()
                .any(|s| s.param == "kind" && s.pred == kind_is("physical"))
        );
    }
}
