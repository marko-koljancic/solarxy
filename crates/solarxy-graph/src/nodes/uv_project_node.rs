//! The `uv_project` node (Phase 14): planar, box, cylindrical, or
//! spherical UV projection over the whole input set, normalized against
//! its AABB. High QA value: texel density needs UVs, and imports often
//! lack them. The kernel lives in `solarxy_kernel::uv_project`; box mode
//! rebuilds meshes non-indexed (see the kernel doc), which the validate
//! node's topology counts will reflect.

use solarxy_kernel::uv_project::{UvAxis, UvProjection, uv_project};

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "uv_project",
        version: 1,
        display_name: "UV Project",
        category: Category::Modifiers,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true).default_port(),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "UV Project",
            vec![
                ParamSpec::new(
                    "mode",
                    "Mode",
                    "projection",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("planar", "Planar"),
                            EnumVariant::new("box", "Box"),
                            EnumVariant::new("cylindrical", "Cylindrical"),
                            EnumVariant::new("spherical", "Spherical"),
                        ],
                    },
                    ParamValue::Enum("planar".into()),
                ),
                ParamSpec::new(
                    "axis",
                    "Axis",
                    "projection",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("x", "X"),
                            EnumVariant::new("y", "Y"),
                            EnumVariant::new("z", "Z"),
                        ],
                    },
                    ParamValue::Enum("y".into()),
                ),
                ParamSpec::new(
                    "scale",
                    "Scale",
                    "projection",
                    ParamType::Vec2,
                    ParamValue::Vec2([1.0, 1.0]),
                )
                .step(0.1),
                ParamSpec::new(
                    "offset",
                    "Offset",
                    "projection",
                    ParamType::Vec2,
                    ParamValue::Vec2([0.0, 0.0]),
                )
                .step(0.05),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Projects UVs onto the input (planar, box, cylindrical, or \
              spherical), normalized over its bounds.",
        search_aliases: &["uv", "unwrap", "project", "texture", "mapping"],
        cook: cook_uv_project,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_uv_project(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    let mode = match p.enum_key("mode") {
        "box" => UvProjection::Box,
        "cylindrical" => UvProjection::Cylindrical,
        "spherical" => UvProjection::Spherical,
        _ => UvProjection::Planar,
    };
    let axis = match p.enum_key("axis") {
        "x" => UvAxis::X,
        "z" => UvAxis::Z,
        _ => UvAxis::Y,
    };
    let scale2 = p.vec2("scale");
    let offset2 = p.vec2("offset");
    let scale = [scale2[0] as f32, scale2[1] as f32];
    let offset = [offset2[0] as f32, offset2[1] as f32];

    Ok(CookOutcome::Done(Outputs::geometry(uv_project(
        input, mode, axis, scale, offset,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::registry::coerce::Value;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::generate_box;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn cooks_at_defaults_and_writes_uvs() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let mut mesh = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        mesh.tex_coords = None; // simulate an import without UVs
        let set = GeometrySet::from_mesh(mesh);
        let inputs = Inputs::new(
            [(
                "geometry".to_string(),
                InputSlot::Single(Value::Geometry(Arc::new(set))),
            )]
            .into_iter()
            .collect(),
        );
        let assets = crate::assets::AssetTable::default();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(outputs) = cook_uv_project(&resolved, &inputs, &mut cx).unwrap()
        else {
            panic!("synchronous cook");
        };
        let out = outputs.get("geometry").unwrap().as_geometry().unwrap();
        assert!(out.meshes[0].tex_coords.is_some(), "UVs written");
    }
}
