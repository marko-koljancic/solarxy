//! The `geo` container (node catalog part II, section 12): the subflow
//! host. No ports, no wire output. The renderer resolves its display object
//! from the subflow's active display node and applies this node's transform
//! as the `SceneObject` transform (not baked into vertices, so transform
//! edits never recook the subflow).

use super::common::{params_with, passive_cook};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "geo",
        version: 1,
        display_name: "Geo",
        category: Category::Container,
        contexts: ContextMask::ROOT,
        inputs: vec![],
        outputs: vec![],
        params: params_with(
            "Geo",
            vec![
                ParamSpec::new(
                    "translate",
                    "Translate",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0; 3]),
                )
                .unit(Unit::Meters),
                ParamSpec::new(
                    "rotate",
                    "Rotate",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0; 3]),
                )
                .unit(Unit::Degrees),
                ParamSpec::new(
                    "scale",
                    "Scale",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([1.0; 3]),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 100.0),
                ParamSpec::new(
                    "uniform_scale",
                    "Uniform Scale",
                    "transform",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 100.0),
            ],
        ),
        // Bypassing a geo excludes its whole subflow from the scene.
        bypass: BypassBehavior::Mute,
        doc: "A container node hosting a subflow; renders its subflow's \
              active display object with this node's transform applied.",
        search_aliases: &["object", "container", "group", "subflow"],
        cook: passive_cook,
        migrate: None,
    }
}
