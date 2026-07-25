//! The `torus` primitive.

use solarxy_kernel::primitives::generate_torus;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "torus",
        version: 2,
        display_name: "Torus",
        category: Category::Generators,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Torus",
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
                    "Distance from the centre of the torus out to the centre of the \
                     tube, in metres. It is the ring's radius, not the outer edge: \
                     the silhouette reaches radius + tube, so the default 0.5 against \
                     a 0.2 tube measures 0.7 from the origin.",
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
                    "Radius of the tube's cross-section, in metres, measured out from \
                     the ring. It eats the hole from both sides: the hole's radius is \
                     radius - tube, so at tube = radius the hole shuts completely and \
                     above that the surface passes through itself.",
                ),
                ParamSpec::new(
                    "radial_segments",
                    "Radial Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(16),
                )
                .hard(3.0, 1024.0)
                .doc(
                    "Facets around the tube's cross-section, so this is how round the \
                     tube itself is. The minimum of 3 gives a tube of triangular \
                     section. This is the cross-section, not the sweep: the sweep is \
                     `tubular_segments`.",
                ),
                ParamSpec::new(
                    "tubular_segments",
                    "Tubular Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(32),
                )
                .hard(3.0, 1024.0)
                .doc(
                    "Facets around the sweep, so this is how round the ring is. The \
                     minimum of 3 bends the tube into a triangle. It usually wants to \
                     be the higher of the two counts -- the defaults are 32 against \
                     16 -- because the sweep covers the longer distance.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A torus, a donut, centred on the origin and swept around the Z axis so it \
              lies in the XY plane. Normals point radially out of the tube and are \
              exact, so it shades smooth.\n\n\
              Reach for it for rings, tyres, and handles, and as a test object: it \
              curves in two directions and carries a clean UV grid, which makes it an \
              honest check of normals, texture mapping, or a displacement before you \
              commit to real geometry.\n\n\
              The two segment counts are named the opposite way round from most \
              people's first guess. `radial_segments` subdivides the tube's \
              cross-section, how round the tube is; `tubular_segments` subdivides the \
              sweep, how round the ring is. The defaults, 16 and 32, give 561 points \
              and 1024 triangles. Like `plane` it stands upright: the hole faces \
              along Z, not Y.",
        search_aliases: &["donut", "ring"],
        glyph: "torus",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_torus(
        p.f32("radius"),
        p.f32("tube"),
        p.u32("radial_segments"),
        p.u32("tubular_segments"),
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
