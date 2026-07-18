//! The `camera` root node: a viewport
//! camera the user authors in the graph and looks through per pane.
//!
//! Portless, root-context, `Mute`-bypassable, and cooks passively: its
//! `CameraDef` is resolved directly from its params by the engine's scene
//! builder (`engine::scene`), exactly like a light's `LightDef`, never carried
//! on a wire. Three kinds (perspective / orthographic / physical); physical
//! derives its FOV from a focal length and sensor width. Not a geometry node:
//! no ports, no material. The render/turntable-export consumer the catalog's
//! camera deferral was waiting on now exists.

use super::common::{general_params, passive_cook};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Pred, Unit};
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

fn kind_is(variant: &str) -> Pred {
    Pred::Eq(ParamValue::Enum(variant.to_string()))
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

    NodeTypeDescriptor {
        type_id: "camera",
        version: 1,
        display_name: "Camera",
        // No dedicated Camera category; Utility is the root-object home (the
        // registry invariant gates context, not category).
        category: Category::Utility,
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
        role: NodeRole::Standard,
        cook: passive_cook,
        migrate: None,
    }
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
