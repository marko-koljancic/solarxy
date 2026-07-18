//! The `geo` container: the subflow
//! host. No ports, no wire output. The renderer resolves its display object
//! from the subflow's active display node and applies this node's transform
//! as the `SceneObject` transform (not baked into vertices, so transform
//! edits never recook the subflow).

use super::common::{migrate_geo, params_with, passive_cook, rendering_params, rotate_order_param};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::document::ContextKind;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    // v2: `receive_shadow` dropped from the rendering group (never wired).
    // v3: `rotate_order` added, and the world matrix moved onto the kernel's
    // `compose_trs`. Before this, `geo` hardcoded ZYX while `transform`
    // defaulted to XYZ, so identical angles meant different orientations; the
    // rotate gizmo, which must decompose back into the target's order, is what
    // forced the two to agree. See `migrate_geo` for how old files keep their
    // exact appearance.
    NodeTypeDescriptor {
        type_id: "geo",
        version: 3,
        display_name: "Geo",
        category: Category::Container,
        contexts: ContextSet::OBJ,
        // The geo container opens a geometry network; the engine creates
        // and kinds the child canvas from this, never from the type id.
        opens: Some(ContextKind::Geo),
        inputs: vec![],
        outputs: vec![],
        params: {
            let mut params = params_with(
                "Geo",
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
                        "Where the object sits in the world, in metres from \
                         the origin. Applied after rotation and scale, so it \
                         moves the object as a finished whole. The move gizmo \
                         writes here.",
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
                        "Euler angles in degrees, one per axis, applied in \
                         Rotate Order. With two or more axes nonzero the \
                         order changes the result, so the two params are read \
                         together.",
                    ),
                    rotate_order_param().doc(
                        "The order the three Euler angles compose in. It only \
                         matters once two or more axes are nonzero -- a \
                         single-axis rotation is identical under all six \
                         orders. Match the order your DCC used if you are \
                         transcribing angles from one, or the object arrives \
                         pointing somewhere else.",
                    ),
                    ParamSpec::new(
                        "scale",
                        "Scale",
                        "transform",
                        ParamType::Vec3,
                        ParamValue::Vec3([1.0; 3]),
                    )
                    .hard(0.001, 10000.0)
                    .soft(0.01, 100.0)
                    .doc(
                        "Per-axis scale multipliers. 1 on every axis leaves \
                         the object alone; unequal values stretch it. \
                         Multiplied by Uniform Scale, so the effective scale \
                         on each axis is this times that.",
                    ),
                    ParamSpec::new(
                        "uniform_scale",
                        "Uniform Scale",
                        "transform",
                        ParamType::Float,
                        ParamValue::Float(1.0),
                    )
                    .hard(0.001, 10000.0)
                    .soft(0.01, 100.0)
                    .doc(
                        "One multiplier over all three axes, on top of Scale. \
                         Reach for it to resize the whole object without \
                         disturbing a per-axis ratio you have already dialled \
                         in.",
                    ),
                ],
            );
            // The root render flags (visible / cast_shadow) live on the
            // container, not its subflow nodes.
            params.extend(rendering_params());
            params
        },
        // Bypassing a geo excludes its whole subflow from the scene.
        bypass: BypassBehavior::Mute,
        doc: "A container: one object in the scene, holding a whole geometry \
              network inside it. It has no ports and produces no wire value. \
              What it renders is whichever node inside carries the display \
              flag, placed in the world by this node's transform.\n\n\
              Containers are how a scene stays a scene instead of one \
              enormous graph. The object level holds geos, cameras, and \
              lights -- the things a scene is made of -- and each geo's \
              network holds the modelling that builds that one object. \
              Double-click a geo to dive into its network; the breadcrumb \
              walks you back out. Bypassing a geo takes its entire subflow \
              out of the scene in one click.\n\n\
              The rendering flags live here and only here. Visible and Cast \
              Shadow are per-object properties, so they belong to the object, \
              not to the box or the merge inside it -- which is why a plain \
              geometry node has no such params, and why hunting for a Visible \
              checkbox on your `box` will not find one. The transform is the \
              same story: it is applied to the object at draw time rather \
              than baked into the points, so dragging a geo around never \
              recooks the network inside it, however heavy that network is.",
        search_aliases: &["object", "container", "group", "subflow"],
        glyph: "geo",
        role: NodeRole::Container,
        cook: passive_cook,
        migrate: Some(migrate_geo),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamSource;

    fn params_with_rotate(rotate: [f64; 3]) -> serde_json::Map<String, serde_json::Value> {
        let mut map = serde_json::Map::new();
        map.insert(
            "rotate".to_string(),
            serde_json::to_value(ParamSource::Literal(ParamValue::Vec3(rotate))).unwrap(),
        );
        map
    }

    fn stamped_order(map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
        match serde_json::from_value::<ParamSource>(map.get("rotate_order")?.clone()) {
            Ok(ParamSource::Literal(ParamValue::Enum(key))) => Some(key),
            _ => None,
        }
    }

    /// The whole point of the v3 migration: a geo whose rotation ACTUALLY
    /// depended on the old ZYX order keeps it explicitly, so the document
    /// renders exactly as it did before the unification.
    #[test]
    fn a_multi_axis_rotated_geo_keeps_its_old_order() {
        let mut params = params_with_rotate([30.0, 40.0, 0.0]);
        migrate_geo(2, &mut params).unwrap();
        assert_eq!(stamped_order(&params).as_deref(), Some("zyx"));
    }

    /// ...and a geo where the order was never observable is left alone, so it
    /// picks up the new XYZ default and the document stays free of noise. One
    /// nonzero lane rotates identically under all six orders.
    #[test]
    fn a_single_axis_rotated_geo_is_left_at_the_new_default() {
        for rotate in [[0.0, 0.0, 0.0], [0.0, 45.0, 0.0], [90.0, 0.0, 0.0]] {
            let mut params = params_with_rotate(rotate);
            migrate_geo(2, &mut params).unwrap();
            assert!(
                !params.contains_key("rotate_order"),
                "{rotate:?} should not have been stamped"
            );
        }
    }

    /// An explicit order already in the document is never overwritten.
    #[test]
    fn an_existing_rotate_order_survives_the_migration() {
        let mut params = params_with_rotate([30.0, 40.0, 50.0]);
        params.insert(
            "rotate_order".to_string(),
            serde_json::to_value(ParamSource::Literal(ParamValue::Enum("yxz".to_string())))
                .unwrap(),
        );
        migrate_geo(2, &mut params).unwrap();
        assert_eq!(stamped_order(&params).as_deref(), Some("yxz"));
    }

    /// The v1 step still strips what it always stripped, and a v1 document
    /// walks BOTH steps (the registry calls the hook once per version).
    #[test]
    fn the_v1_step_still_strips_receive_shadow() {
        let mut params = params_with_rotate([30.0, 40.0, 0.0]);
        params.insert("receive_shadow".to_string(), serde_json::json!(true));
        migrate_geo(1, &mut params).unwrap();
        assert!(!params.contains_key("receive_shadow"));
        // v1 -> v2 must not stamp; that is the v2 -> v3 step's job.
        assert!(!params.contains_key("rotate_order"));
        migrate_geo(2, &mut params).unwrap();
        assert_eq!(stamped_order(&params).as_deref(), Some("zyx"));
    }
}
