//! The `validate` modifier (node catalog part II, section 13): the only
//! node with two outputs. It passes its input geometry through unchanged on
//! `geometry` and emits a `Report` on `report`, wrapping the existing
//! `solarxy-core` validation pipeline over a `RawModelData` view of the
//! input `GeometrySet`.

use std::sync::Arc;

use solarxy_core::ValidationReport;
use solarxy_core::validation::{ValidationConfig, ValidationThresholds, validate_raw_model_with_config};

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::{DataType, Value};
use crate::registry::param_spec::{ParamSpec, ParamType, Pred};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "validate",
        version: 1,
        display_name: "Validate",
        category: Category::Modifiers,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry to validate (passed through unchanged)."),
        ],
        outputs: vec![
            geometry_output(),
            PortSpec::single("report", "Report", DataType::Report, false)
                .doc("The validation report."),
        ],
        params: params_with(
            "Validate",
            vec![
                check("normals", "Normals"),
                check("uvs", "UVs"),
                check("topology", "Topology"),
                check("materials", "Materials"),
                check("budget", "Triangle Budget"),
                ParamSpec::new(
                    "triangle_budget",
                    "Budget",
                    "checks",
                    ParamType::Int,
                    ParamValue::Int(0),
                )
                .hard(0.0, 2_000_000_000.0)
                .show_if("budget", Pred::Truthy),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Runs the Solarxy validation checks over the input geometry, \
              passing the geometry through and emitting a report.",
        search_aliases: &["validate", "check", "lint", "inspect"],
        cook,
        migrate: None,
    }
}

fn check(key: &str, label: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        "checks",
        ParamType::Bool,
        ParamValue::Bool(true),
    )
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        // Connected-but-empty upstream: pass empty through, empty report.
        let mut out = Outputs::geometry(solarxy_kernel::GeometrySet::empty());
        out.insert(
            "report",
            Value::Report(Arc::new(ValidationReport::default())),
        );
        return Ok(CookOutcome::Done(out));
    };

    // Map the check toggles onto the pipeline config.
    let config = ValidationConfig {
        normal_mismatch: p.bool("normals"),
        flipped_normals: p.bool("normals"),
        non_manifold_edges: p.bool("topology"),
        triangle_budget: p.bool("budget"),
        allow_open_mesh: false,
        degenerate_triangles: p.bool("topology"),
        material_refs: p.bool("materials"),
        uv_presence: p.bool("uvs"),
        index_buffer: p.bool("topology"),
    };
    let thresholds = ValidationThresholds::default();
    let budget = p.i64("triangle_budget");
    let budget = if p.bool("budget") && budget > 0 {
        Some(budget.min(i64::from(u32::MAX)) as u32)
    } else {
        None
    };

    // The GeometrySet -> RawModelData adapter (round-trip tested in the
    // kernel), then the existing pipeline.
    let raw = input.to_raw();
    let result = validate_raw_model_with_config(&raw, "", &config, &thresholds, budget);
    let report = Arc::new(result.report);

    // Pass the input geometry through unchanged, plus the report.
    let mut out = Outputs::single("geometry", Value::Geometry(Arc::clone(input)));
    out.insert("report", Value::Report(report));
    Ok(CookOutcome::Done(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use solarxy_kernel::primitives::generate_box;
    use solarxy_kernel::{GeometrySet, KernelMesh};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(set: GeometrySet) -> (u64, usize) {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(set))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("validate cooks synchronously");
        };
        let geo_points = out
            .get("geometry")
            .and_then(Value::as_geometry)
            .map_or(0, |g| g.point_count());
        let issues = match out.get("report") {
            Some(Value::Report(r)) => r.issues.len(),
            _ => panic!("validate emits a report"),
        };
        (geo_points, issues)
    }

    #[test]
    fn passes_geometry_through_and_reports_on_a_clean_box() {
        // A well-formed box passes geometry through; the report may hold
        // benign warnings but no panics.
        let (points, _issues) = run(GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1)));
        assert_eq!(points, 24, "geometry passes through unchanged");
    }

    #[test]
    fn flags_a_broken_mesh() {
        // A mesh with no normals and a degenerate/empty index set produces
        // at least one issue.
        let mesh = KernelMesh::new("broken", vec![[0.0; 3], [0.0; 3], [0.0; 3]], vec![0, 1, 2]);
        let (_points, issues) = run(GeometrySet::from_mesh(mesh));
        assert!(issues > 0, "a degenerate triangle should be reported");
    }
}
