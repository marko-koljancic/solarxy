//! The `cylinder` primitive (node catalog part II, section 13). The
//! catalog allows `radius_top = 0` (a capped cone), unlike Minimystix.

use solarxy_kernel::primitives::generate_cylinder;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "cylinder",
        version: 2,
        display_name: "Cylinder",
        category: Category::Primitives,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Cylinder",
            vec![
                // radius_top allows 0 (cone tip); radius_bottom likewise.
                ParamSpec::new(
                    "radius_top",
                    "Radius Top",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(0.5),
                )
                .hard(0.0, 10000.0)
                .soft(0.0, 50.0)
                .unit(Unit::Meters),
                ParamSpec::new(
                    "radius_bottom",
                    "Radius Bottom",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(0.5),
                )
                .hard(0.0, 10000.0)
                .soft(0.0, 50.0)
                .unit(Unit::Meters),
                ParamSpec::new(
                    "height",
                    "Height",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 100.0)
                .unit(Unit::Meters),
                ParamSpec::new(
                    "radial_segments",
                    "Radial Segments",
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
                    ParamValue::Int(1),
                )
                .hard(1.0, 512.0),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A cylinder (or capped cone when a radius is zero).",
        search_aliases: &["tube", "pipe"],
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_cylinder(
        p.f32("radius_top"),
        p.f32("radius_bottom"),
        p.f32("height"),
        p.u32("radial_segments"),
        p.u32("height_segments"),
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
