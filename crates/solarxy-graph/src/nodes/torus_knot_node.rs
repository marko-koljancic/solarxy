//! The `torus_knot` primitive.

use solarxy_kernel::primitives::generate_torus_knot;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "torus_knot",
        version: 2,
        display_name: "Torus Knot",
        category: Category::Primitives,
        contexts: ContextSet::GEO,
        opens: None,
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
                .unit(Unit::Meters)
                .doc(
                    "Scales the whole knot curve, in metres. It is not an outer \
                     radius: the curve's distance from the Z axis rides between 0.5x \
                     and 1.5x this value as it winds, so the silhouette reaches about \
                     1.5 x radius, plus `tube`.",
                ),
                ParamSpec::new(
                    "tube",
                    "Tube",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(0.2),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 100.0)
                .unit(Unit::Meters)
                .doc(
                    "Radius of the swept tube's cross-section, in metres. Nothing \
                     checks the knot for self-clearance, so once this grows large \
                     against `radius` neighbouring passes of the curve simply \
                     intersect each other.",
                ),
                ParamSpec::new("p", "P", "geometry", ParamType::Int, ParamValue::Int(2))
                    .hard(1.0, 10.0)
                    .doc(
                        "How many times the curve winds around the Z axis before it \
                         closes. With `q` it picks the knot: (2, 3) is the trefoil, \
                         (2, 5) the cinquefoil. Keep it coprime with `q` -- a shared \
                         factor makes the curve retrace its own path instead of \
                         tying a new knot.",
                    ),
                ParamSpec::new("q", "Q", "geometry", ParamType::Int, ParamValue::Int(3))
                    .hard(1.0, 10.0)
                    .doc(
                        "How many times the curve winds through the ring's hole before \
                         it closes. Raising it against a fixed `p` adds lobes. As with \
                         `p`, a factor shared between the two retraces the curve: it \
                         is covered gcd(p, q) times, laying that many coincident \
                         copies of the tube on the same path.",
                    ),
                ParamSpec::new(
                    "tubular_segments",
                    "Tubular Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(128),
                )
                .hard(3.0, 2048.0)
                .doc(
                    "Samples taken along the knot curve. This is what keeps the curve \
                     itself smooth, and it needs to be generous because the curve is \
                     long and twists constantly: hence 128 by default, against 32 for \
                     the cross-section.",
                ),
                ParamSpec::new(
                    "radial_segments",
                    "Radial Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(32),
                )
                .hard(3.0, 2048.0)
                .doc(
                    "Facets around the tube's cross-section, the same meaning it has \
                     on `torus`: it makes the tube round, not the curve. When the \
                     mesh is too heavy this is usually the cheaper of the two to cut.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A tube swept along a (p, q) torus knot: a closed curve that winds p times \
              around the Z axis while winding q times through the ring's hole. The \
              default (2, 3) is the trefoil.\n\n\
              It is mostly a showpiece and a test object. Its curve turns through \
              every orientation, which makes it the honest check for a shader, a \
              normal map, or a deform -- the kind of thing a box or a sphere would \
              let pass unnoticed.\n\n\
              It is far and away the heaviest primitive here. The defaults, 128 \
              tubular by 32 radial segments, come to 4257 points and 8192 triangles, \
              about eight times the sphere, and both counts reach 2048. They \
              multiply, so trim `radial_segments` before `tubular_segments`: the \
              sweep needs \
              its samples to keep the curve smooth, the cross-section rarely needs \
              all 32.",
        search_aliases: &["knot", "pretzel"],
        glyph: "torus_knot",
        role: NodeRole::Standard,
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
