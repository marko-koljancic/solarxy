//! The `box` primitive (node catalog part II, section 13).

use solarxy_kernel::primitives::generate_box;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeRole, NodeTypeDescriptor};

fn dimension(key: &str, label: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        "geometry",
        ParamType::Float,
        ParamValue::Float(1.0),
    )
    .hard(0.001, 10000.0)
    .soft(0.01, 100.0)
    .unit(Unit::Meters)
}

fn segments(key: &str, label: &str) -> ParamSpec {
    ParamSpec::new(key, label, "geometry", ParamType::Int, ParamValue::Int(1)).hard(1.0, 512.0)
}

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "box",
        version: 2,
        display_name: "Box",
        category: Category::Primitives,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Box",
            vec![
                dimension("width", "Width"),
                dimension("height", "Height"),
                dimension("depth", "Depth"),
                segments("width_segments", "Width Segments"),
                segments("height_segments", "Height Segments"),
                segments("depth_segments", "Depth Segments"),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A rectangular box, subdivided per axis.",
        search_aliases: &["cube", "rectangle", "cuboid"],
        glyph: "box",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_box(
        p.f32("width"),
        p.f32("height"),
        p.f32("depth"),
        p.u32("width_segments"),
        p.u32("height_segments"),
        p.u32("depth_segments"),
    ));
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::coerce::Value;
    use std::collections::BTreeMap;

    #[test]
    fn cooks_a_box_at_defaults() {
        let specs = descriptor().params;
        let resolved = crate::registry::resolve::resolve_params(&BTreeMap::new(), &specs).unwrap();
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let out = cook(&resolved, &Inputs::default(), &mut cx).unwrap();
        let CookOutcome::Done(outputs) = out else {
            panic!("box cooks synchronously");
        };
        let Some(Value::Geometry(set)) = outputs.get("geometry") else {
            panic!("box outputs geometry");
        };
        assert_eq!(set.point_count(), 24);
        assert_eq!(set.triangle_count(), 12);
    }
}
