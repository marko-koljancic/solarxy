//! The `points_from_geo` modifier: collapses geometry to a point cloud,
//! either its vertices verbatim or one point per primitive center. An
//! inspection lens for scans and a points source for `copy_to_points`.

use solarxy_kernel::points_from_geo::{PointsFrom, points_from_geo};

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "points_from_geo",
        version: 1,
        display_name: "Points from Geo",
        category: Category::Topology,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry to collapse into a point cloud."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Points from Geo",
            vec![
                ParamSpec::new(
                    "mode",
                    "Mode",
                    "points",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("vertices", "Vertices"),
                            EnumVariant::new("primitive_centers", "Primitive Centers"),
                        ],
                    },
                    ParamValue::Enum("vertices".into()),
                )
                .doc(
                    "What each output point corresponds to. Vertices keeps every \
                     input vertex with its attributes carried verbatim. \
                     Primitive Centers places one point at each triangle \
                     centroid or segment midpoint, averaging the corner \
                     attributes into it.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Collapses the input to a Points-topology cloud. Vertices mode \
              keeps every vertex where it is, with normals and UVs lifted into \
              the point attributes and every attribute riding along untouched; \
              Primitive Centers places one point per triangle or segment at its \
              center, averaging the corner attributes into it.\n\n\
              Two jobs, one node: as an inspection lens it strips a surface \
              down to its point structure, showing vertex distribution and \
              density at a glance. As a modeling source it turns any mesh into \
              targets for `copy_to_points` without scattering, so copies land \
              exactly on vertices or face centers rather than randomly.\n\n\
              Points draw unlit at a uniform screen-space size, colored by \
              their `color` attribute when one exists, and materials are \
              dropped in the conversion.",
        search_aliases: &["vertices", "cloud", "convert", "centers", "centroid"],
        glyph: "points_from_geo",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let mode = match p.enum_key("mode") {
        "primitive_centers" => PointsFrom::PrimitiveCenters,
        _ => PointsFrom::Vertices,
    };
    Ok(CookOutcome::Done(Outputs::geometry(points_from_geo(
        input, mode,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_core::geometry::MeshTopology;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::generate_box;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(stored: BTreeMap<String, ParamSource>) -> Arc<GeometrySet> {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(
                generate_box(1.0, 1.0, 1.0, 1, 1, 1),
            )))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        Arc::clone(set)
    }

    #[test]
    fn vertices_mode_keeps_the_vertex_count() {
        let set = run(BTreeMap::new());
        assert_eq!(set.meshes[0].topology, MeshTopology::Points);
        assert_eq!(set.meshes[0].vertex_count(), 24, "the box's 24 corners");
    }

    #[test]
    fn centers_mode_yields_one_point_per_triangle() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "mode".to_string(),
            ParamSource::Literal(ParamValue::Enum("primitive_centers".into())),
        );
        let set = run(stored);
        assert_eq!(set.meshes[0].vertex_count(), 12, "the box's 12 triangles");
    }
}
