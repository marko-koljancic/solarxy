//! The `sphere` primitive (node catalog part II, section 13).

use solarxy_kernel::primitives::generate_sphere;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "sphere",
        version: 2,
        display_name: "Sphere",
        category: Category::Primitives,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Sphere",
            vec![
                ParamSpec::new(
                    "radius",
                    "Radius",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(0.5),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 50.0)
                .unit(Unit::Meters),
                ParamSpec::new(
                    "width_segments",
                    "Width Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(32),
                )
                .hard(3.0, 512.0),
                ParamSpec::new(
                    "height_segments",
                    "Height Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(16),
                )
                .hard(2.0, 512.0),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A UV sphere with poles along Y.",
        search_aliases: &["ball", "globe"],
        glyph: "sphere",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_sphere(
        p.f32("radius"),
        p.u32("width_segments"),
        p.u32("height_segments"),
    ));
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::coerce::Value;
    use std::collections::BTreeMap;

    #[test]
    fn cooks_a_sphere_at_defaults() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(outputs) = cook(&resolved, &Inputs::default(), &mut cx).unwrap()
        else {
            panic!("sphere cooks synchronously");
        };
        let Some(Value::Geometry(set)) = outputs.get("geometry") else {
            panic!("sphere outputs geometry");
        };
        assert_eq!(set.point_count(), 561);
    }
}
