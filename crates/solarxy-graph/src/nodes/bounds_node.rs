//! The `bounds` utility node. Emits
//! the input's axis-aligned bounding box as geometry, or a single point
//! primitive at its center. The QA persona's measuring tape.
//!
//! v2 (0.8.0): center mode emits a true point primitive, realizing the
//! catalog's original intent now that point topology exists. It supersedes
//! the 0.7.x marker-cube substitute; the `marker_size` param
//! went with the cube and is stripped by the v1-to-v2 migration.

use solarxy_kernel::bounds_geo::{bounds_box, center_point};

use super::common::{geometry_output, params_with, strip_keys};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{
    BypassBehavior, Category, ContextSet, MigrateError, NodeRole, NodeTypeDescriptor, PortSpec,
};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "bounds",
        version: 2,
        display_name: "Bounds",
        category: Category::Utility,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc(
                    "The geometry to measure. Its extents drive the output; \
                     the geometry itself does not appear downstream. An empty \
                     or unconnected input yields empty geometry, with a \
                     warning in the empty case.",
                ),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Bounds",
            vec![
                ParamSpec::new(
                    "mode",
                    "Mode",
                    "bounds",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("box", "Box"),
                            EnumVariant::new("center", "Center"),
                        ],
                    },
                    ParamValue::Enum("box".into()),
                )
                .doc(
                    "Box emits the bounding box itself, matching the input's \
                     extents on every axis. Center discards the volume and \
                     emits a single point primitive at the box's centre, for \
                     when the pivot is what you are chasing rather than the \
                     extents.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Emits the input's axis-aligned bounding box as geometry: a solid \
              box spanning the input's extents, or a single point primitive \
              sitting at its centre. It measures the input and replaces it; \
              the box is the output, not an overlay on the original.\n\n\
              This is the measuring tape. Tap it off a chain to see how big \
              something actually is, where its centre really sits, or whether \
              two parts occupy the space you think they do -- `merge` the \
              bounds with the model it measured and you can eyeball the fit \
              directly. Bypassing it passes the input through, which makes it \
              cheap to leave wired in as an inspection tap.\n\n\
              The box is axis-aligned in object space, so a diagonally \
              oriented model gets a box much larger than the model itself; \
              that is the AABB being honest, not a bug. Center mode's point \
              draws at the renderer's uniform on-screen point size and is not \
              pickable in the viewport (select it on the node canvas). An \
              empty input warns and emits nothing rather than boxing the \
              fallback bounds into a confident unit cube around nothing.",
        search_aliases: &["bbox", "aabb", "extents", "measure", "center"],
        glyph: "bounds",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_v1_strip_marker_size),
    }
}

/// v1 -> v2: center mode became a true point primitive, so
/// the marker cube's `marker_size` has nothing left to size and is stripped.
#[allow(clippy::unnecessary_wraps)] // signature matches MigrateFn
fn migrate_v1_strip_marker_size(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError> {
    if from == 1 {
        strip_keys(params, &["marker_size"]);
    }
    Ok(())
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    // An empty set's AABB is the (-1,-1,-1)..(1,1,1) fallback, not a zero box,
    // so boxing it would emit a confident-looking unit cube around nothing.
    if input.is_renderable_empty() {
        cx.warn("bounds has no geometry to measure");
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    }

    let set = match p.enum_key("mode") {
        "center" => center_point(&input.bounds),
        _ => bounds_box(&input.bounds),
    };
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::generate_box;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(
        stored: BTreeMap<String, ParamSource>,
        input: GeometrySet,
    ) -> (Arc<GeometrySet>, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(input))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("bounds cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        (Arc::clone(set), cx.take_warnings())
    }

    #[test]
    fn box_mode_matches_the_input_extents() {
        let input = GeometrySet::from_mesh(generate_box(2.0, 4.0, 6.0, 1, 1, 1));
        let (set, warns) = run(BTreeMap::new(), input);
        assert!(warns.is_empty());
        let s = set.bounds.size();
        assert!((s.x - 2.0).abs() < 1e-4, "{s:?}");
        assert!((s.y - 4.0).abs() < 1e-4, "{s:?}");
        assert!((s.z - 6.0).abs() < 1e-4, "{s:?}");
    }

    #[test]
    fn center_mode_emits_a_point_primitive_at_the_center() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "mode".to_string(),
            ParamSource::Literal(ParamValue::Enum("center".into())),
        );
        let input = GeometrySet::from_mesh(generate_box(4.0, 4.0, 4.0, 1, 1, 1));
        let (set, warns) = run(stored, input);
        assert!(warns.is_empty());
        assert_eq!(set.mesh_count(), 1);
        let mesh = &set.meshes[0];
        assert_eq!(mesh.topology, solarxy_core::MeshTopology::Points);
        assert_eq!(mesh.positions.len(), 1);
        let p = mesh.positions[0];
        assert!(p[0].abs() < 1e-4 && p[1].abs() < 1e-4 && p[2].abs() < 1e-4);
    }

    /// The v1 -> v2 migration strips the marker cube's `marker_size`; a v1
    /// document with the param stored loads clean, keeping `mode`.
    #[test]
    fn migration_strips_marker_size_from_v1_params() {
        let mut raw = serde_json::Map::new();
        raw.insert("mode".to_string(), serde_json::json!("center"));
        raw.insert("marker_size".to_string(), serde_json::json!(0.25));
        migrate_v1_strip_marker_size(1, &mut raw).unwrap();
        assert!(!raw.contains_key("marker_size"), "marker_size stripped");
        assert_eq!(raw.get("mode"), Some(&serde_json::json!("center")));
    }

    /// The empty-input trap: `compute_bounds(&[])` returns a unit box, so a
    /// naive implementation would emit a confident 2x2x2 cube around nothing.
    #[test]
    fn an_empty_input_warns_and_emits_nothing_rather_than_a_unit_cube() {
        let (set, warns) = run(BTreeMap::new(), GeometrySet::empty());
        assert!(set.is_renderable_empty(), "no phantom unit cube");
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("no geometry"), "got: {}", warns[0]);
    }
}
