//! The `camera` root node (node catalog amendment 2026-07-16): a viewport
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
        .unit(Unit::Meters),
        ParamSpec::new(
            "target",
            "Target",
            "transform",
            ParamType::Vec3,
            ParamValue::Vec3([0.0; 3]),
        )
        .unit(Unit::Meters),
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
        .show_if("kind", kind_is("perspective")),
        ParamSpec::new(
            "focal_length",
            "Focal Length (mm)",
            "lens",
            ParamType::Float,
            ParamValue::Float(50.0),
        )
        .hard(1.0, 2000.0)
        .soft(14.0, 300.0)
        .show_if("kind", kind_is("physical")),
        ParamSpec::new(
            "sensor_width",
            "Sensor Width (mm)",
            "lens",
            ParamType::Float,
            ParamValue::Float(36.0),
        )
        .hard(1.0, 100.0)
        .soft(10.0, 70.0)
        .show_if("kind", kind_is("physical")),
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
        .show_if("kind", kind_is("orthographic")),
        ParamSpec::new(
            "near",
            "Near Clip",
            "lens",
            ParamType::Float,
            ParamValue::Float(0.1),
        )
        .hard(0.0001, 100_000.0)
        .soft(0.01, 10.0)
        .unit(Unit::Meters),
        ParamSpec::new(
            "far",
            "Far Clip",
            "lens",
            ParamType::Float,
            ParamValue::Float(1000.0),
        )
        .hard(0.001, 1_000_000.0)
        .soft(10.0, 10_000.0)
        .unit(Unit::Meters),
        // Framing: the aspect drives the viewport gate and the default export aspect.
        ParamSpec::new(
            "aspect",
            "Aspect (W/H)",
            "framing",
            ParamType::Float,
            ParamValue::Float(16.0 / 9.0),
        )
        .hard(0.1, 10.0)
        .soft(0.5, 3.0),
        // Gizmo: the wireframe frustum, mirroring the light helper pair.
        ParamSpec::new(
            "show_gizmo",
            "Show Gizmo",
            "gizmo",
            ParamType::Bool,
            ParamValue::Bool(true),
        ),
        ParamSpec::new(
            "gizmo_size",
            "Gizmo Size",
            "gizmo",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .hard(0.1, 10.0),
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
        doc: "A viewport camera you look through per pane. Perspective, \
              orthographic, or physical (lens); lock a pane to it to frame the \
              shot, and render through it in screenshots and turntable exports.",
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
