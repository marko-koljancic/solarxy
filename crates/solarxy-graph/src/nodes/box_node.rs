//! The `box` primitive.

use solarxy_kernel::primitives::generate_box;

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
        "Size along {axis}, in metres. The box is centred on the origin, so \
         this extends {}0.5x either side rather than growing in one direction.",
        "\u{00b1}"
    ))
}

fn segments(key: &str, label: &str, axis: &str) -> ParamSpec {
    ParamSpec::new(key, label, "geometry", ParamType::Int, ParamValue::Int(1))
        .hard(1.0, 512.0)
        .doc(format!(
            "How many divisions the {axis} faces are cut into. 1 leaves a flat \
             quad. Raise it only when something downstream needs the extra \
             points to work with -- a deform, a noise displacement, a \
             subdivide -- because every segment multiplies the point count."
        ))
}

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "box",
        version: 2,
        display_name: "Box",
        category: Category::Primitives,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Box",
            vec![
                dimension("width", "Width", "X"),
                dimension("height", "Height", "Y"),
                dimension("depth", "Depth", "Z"),
                segments("width_segments", "Width Segments", "X"),
                segments("height_segments", "Height Segments", "Y"),
                segments("depth_segments", "Depth Segments", "Z"),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A rectangular box centred on the origin, sized per axis and \
              optionally divided into a grid of quads on each face.\n\n\
              This is the usual starting point for a blockout: box out the \
              massing first, then reach for `transform` to place it and \
              `merge` to combine it with others. It generates flat-shaded \
              geometry with hard edges, because each face carries its own \
              corner points -- 24 points for the default 12 triangles, not 8 \
              shared ones.\n\n\
              The segment counts only matter to something downstream that \
              needs the extra points. A plain box needs none, so they default \
              to 1.",
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
