//! The `plane` primitive.

use solarxy_kernel::primitives::generate_plane;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

fn dimension(key: &str, label: &str, axis: &str) -> ParamSpec {
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
    .doc(format!(
        "Size along {axis}, in metres. The plane is centred on the origin, so \
         this extends {}0.5x either side rather than growing in one direction.",
        "\u{00b1}"
    ))
}

fn segments(key: &str, label: &str, axis: &str) -> ParamSpec {
    ParamSpec::new(key, label, "geometry", ParamType::Int, ParamValue::Int(1))
        .hard(1.0, 1024.0)
        .doc(format!(
            "How many divisions the plane is cut into along {axis}. 1 leaves a \
             single flat quad. Unlike the segment counts on `box`, raising this \
             is routine: a plane is usually the input to a displacement or a \
             deform, and those have nothing to move without points."
        ))
}

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "plane",
        version: 2,
        display_name: "Plane",
        category: Category::Primitives,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Plane",
            vec![
                dimension("width", "Width", "X"),
                dimension("height", "Height", "Y"),
                segments("width_segments", "Width Segments", "X"),
                segments("height_segments", "Height Segments", "Y"),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A flat rectangle in the XY plane facing +Z, centred on the origin and \
              optionally cut into a grid of quads. It is a single sheet: every normal \
              is +Z, and there is no thickness and no back face.\n\n\
              It is the usual base for anything displaced -- raise the segment counts \
              and feed it to a deform -- and it doubles as a ground plane or a \
              backdrop once `transform` has placed it.\n\n\
              It stands upright, it does not lie flat. The XY/+Z orientation is the \
              spec the other primitives share, so using it as ground means rotating \
              it -90 degrees about X first, to face +Y. It is the cheapest primitive \
              here: 4 points and 2 triangles at the default 1 x 1 segments.",
        search_aliases: &["quad", "grid", "ground"],
        glyph: "plane",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_plane(
        p.f32("width"),
        p.f32("height"),
        p.u32("width_segments"),
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
