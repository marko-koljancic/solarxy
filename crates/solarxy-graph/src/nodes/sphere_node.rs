//! The `sphere` primitive.

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
        category: Category::Generators,
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
                .unit(Unit::Meters)
                .doc(
                    "Distance from the centre to the surface, in metres. The sphere is \
                     centred on the origin, so the default 0.5 is a 1 m ball and the \
                     poles land at (0, +/-radius, 0).",
                ),
                ParamSpec::new(
                    "width_segments",
                    "Width Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(32),
                )
                .hard(3.0, 512.0)
                .doc(
                    "Columns of longitude around the Y axis, so this is how round the \
                     sphere reads when you look down at it. The minimum is 3, which \
                     leaves a three-sided husk. Each column adds a point to every \
                     latitude row, so this is the more expensive of the two counts.",
                ),
                ParamSpec::new(
                    "height_segments",
                    "Height Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(16),
                )
                .hard(2.0, 512.0)
                .doc(
                    "Rows of latitude from pole to pole, so this is how round the \
                     profile reads from the side. The minimum is 2, which gives a \
                     bipyramid: two cones meeting at the equator. The default 16 \
                     against 32 columns keeps the quads roughly square, this axis \
                     spanning half a turn against the other's full one.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A UV sphere centred on the origin, built as a latitude/longitude grid \
              with its poles on the Y axis. Its normals are exact -- each one is just \
              the normalized position -- so it shades smooth rather than faceted.\n\n\
              Reach for it for blockout massing and as a general test object: \
              `transform` places it, `merge` combines it with others. Raise the \
              segment counts for a rounder silhouette, drop them for a low-poly \
              look; both change the shape, unlike the segment counts on `box`.\n\n\
              Point count is (width + 1) x (height + 1), which at the default 32 x 16 \
              is 561 points and 960 triangles. The extra column is the UV seam, \
              repeating the first column's positions at u = 1 instead of u = 0, and \
              each pole row holds one coincident vertex per column so every column \
              keeps its own UV. At each pole the degenerate half of every quad is \
              skipped, leaving a triangle fan.",
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
