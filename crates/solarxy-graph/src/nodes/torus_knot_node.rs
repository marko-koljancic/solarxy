//! The `torus_knot` primitive (node catalog part II, section 13).

use solarxy_kernel::primitives::generate_torus_knot;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "torus_knot",
        version: 2,
        display_name: "Torus Knot",
        category: Category::Primitives,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Torus Knot",
            vec![
                ParamSpec::new(
                    "radius",
                    "Radius",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(0.5),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 100.0)
                .unit(Unit::Meters),
                ParamSpec::new(
                    "tube",
                    "Tube",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(0.2),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 100.0)
                .unit(Unit::Meters),
                ParamSpec::new("p", "P", "geometry", ParamType::Int, ParamValue::Int(2))
                    .hard(1.0, 10.0),
                ParamSpec::new("q", "Q", "geometry", ParamType::Int, ParamValue::Int(3))
                    .hard(1.0, 10.0),
                ParamSpec::new(
                    "tubular_segments",
                    "Tubular Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(128),
                )
                .hard(3.0, 2048.0),
                ParamSpec::new(
                    "radial_segments",
                    "Radial Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(32),
                )
                .hard(3.0, 2048.0),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A (p, q) torus knot swept as a tube.",
        search_aliases: &["knot", "pretzel"],
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_torus_knot(
        p.f32("radius"),
        p.f32("tube"),
        p.u32("p"),
        p.u32("q"),
        p.u32("tubular_segments"),
        p.u32("radial_segments"),
    ));
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn cooks_at_defaults() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        assert!(matches!(
            cook(&resolved, &Inputs::default(), &mut cx),
            Ok(CookOutcome::Done(_))
        ));
    }
}
