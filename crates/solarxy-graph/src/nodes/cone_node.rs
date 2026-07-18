//! The `cone` primitive.

use solarxy_kernel::primitives::generate_cone;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "cone",
        version: 2,
        display_name: "Cone",
        category: Category::Primitives,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Cone",
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
                    "Radius of the base ring, at -height/2, in metres. The apex sits \
                     directly above the centre of that ring on the Y axis, so this \
                     sets how wide the cone flares rather than where it points.",
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
                    "Distance from base to apex along Y, in metres. The cone is \
                     centred on the origin, so raising it moves both ends apart: the \
                     apex to +height/2 and the base to -height/2, not the apex alone.",
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
                    "Facets around the base circumference. At 32 it reads as a smooth \
                     cone; drop it to 4 for a square pyramid or to the minimum of 3 \
                     for a tetrahedron, which is what the `pyramid` alias is about.",
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
                    "How many rows the sloping side is cut into between apex and base. \
                     The silhouette is unchanged, because the side is straight either \
                     way, so raise it only for a downstream deform that needs the \
                     extra points to work with.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A cone centred on the origin, with its apex at +height/2 and a flat base \
              cap at -height/2. The sloping side is smooth-shaded, and there is no \
              top cap.\n\n\
              Reach for it for spikes, roofs, and trees in a blockout, then \
              `transform` to place it and `merge` to combine it. At low \
              `radial_segments` it is also the pyramid primitive: 4 gives a square \
              pyramid, 3 a tetrahedron.\n\n\
              This is `cylinder` with the top radius pinned to 0, sharing one \
              generator so the tip is handled in exactly one place. That means the \
              apex is not a single welded point: the tip row keeps one coincident \
              vertex per column so each column holds its own UV, and the degenerate \
              half of each tip quad is skipped. The default is 100 points for 64 \
              triangles, against the cylinder's 134 for 128.",
        search_aliases: &["pyramid", "spike"],
        glyph: "cone",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_cone(
        p.f32("radius"),
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
