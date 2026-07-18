//! The `cylinder` primitive. The
//! catalog allows `radius_top = 0` (a capped cone), unlike Minimystix.

use solarxy_kernel::primitives::generate_cylinder;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "cylinder",
        version: 2,
        display_name: "Cylinder",
        category: Category::Primitives,
        contexts: ContextSet::GEO,
        opens: None,
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
                .unit(Unit::Meters)
                .doc(
                    "Radius of the top ring, at +height/2, in metres. Set it to 0 and \
                     the ring collapses to a point and its cap disappears, turning the \
                     cylinder into a cone; set it anywhere between 0 and \
                     `radius_bottom` for a truncated cone.",
                ),
                ParamSpec::new(
                    "radius_bottom",
                    "Radius Bottom",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(0.5),
                )
                .hard(0.0, 10000.0)
                .soft(0.0, 50.0)
                .unit(Unit::Meters)
                .doc(
                    "Radius of the bottom ring, at -height/2, in metres. 0 collapses it \
                     to a point and drops the bottom cap, the same as `radius_top` does \
                     at the other end. Both radii at 0 collapses the whole surface onto \
                     the Y axis and leaves nothing to see.",
                ),
                ParamSpec::new(
                    "height",
                    "Height",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 100.0)
                .unit(Unit::Meters)
                .doc(
                    "Length along Y, in metres. The cylinder is centred on the origin, \
                     so this extends half either side and the caps sit at +/-height/2, \
                     rather than growing up from the base.",
                ),
                ParamSpec::new(
                    "radial_segments",
                    "Radial Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(32),
                )
                .hard(3.0, 512.0)
                .doc(
                    "Facets around the circumference. This is what makes the tube read \
                     as round: 32 is smooth at ordinary sizes, and the minimum of 3 \
                     gives a triangular tube. It prices both caps as well as the torso.",
                ),
                ParamSpec::new(
                    "height_segments",
                    "Height Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(1),
                )
                .hard(1.0, 512.0)
                .doc(
                    "How many rows the torso is cut into between the caps. It never \
                     changes the silhouette, because the torso is straight-sided \
                     either way. Raise it only when something downstream needs the \
                     extra points -- a bend, a noise displacement -- which is why it \
                     defaults to 1.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A cylinder running along Y and centred on the origin, with a flat cap at \
              each end and a smooth-shaded torso. The two radii are independent, so \
              the same node covers tapered tubes and truncated cones.\n\n\
              Reach for it for pipes, pillars, and pegs in a blockout, then \
              `transform` to place it and `merge` to combine it with others. For a \
              plain cone prefer `cone`, which is this same generator with the top \
              radius pinned to 0 and one less param to set.\n\n\
              Either radius may be 0, which collapses that ring to a point and omits \
              its cap. Torso normals lean by the slope between the two radii, so a \
              collapsed tip still shades correctly with no special case. The caps \
              never share vertices with the torso, because their normals differ, so \
              the rim is a hard edge and the default comes to 134 points for 128 \
              triangles.",
        search_aliases: &["tube", "pipe"],
        glyph: "cylinder",
        role: NodeRole::Standard,
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
