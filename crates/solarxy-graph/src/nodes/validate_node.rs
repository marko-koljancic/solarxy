//! The `validate` modifier: the only
//! node with two outputs. It passes its input geometry through unchanged on
//! `geometry` and emits a `Report` on `report`, wrapping the existing
//! `solarxy-core` validation pipeline over a `RawModelData` view of the
//! input `GeometrySet`.

use std::sync::Arc;

use solarxy_core::ValidationReport;
use solarxy_core::validation::{ValidationConfig, ValidationThresholds, validate_raw_model_with_config};

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, JobRequest, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::{DataType, Value};
use crate::registry::param_spec::{ParamSpec, ParamType, Pred};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "validate",
        version: 2,
        display_name: "Validate",
        category: Category::Modifiers,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry to validate (passed through unchanged)."),
        ],
        outputs: vec![
            geometry_output(),
            PortSpec::single("report", "Report", DataType::Report, false).doc(
                "The issues found: what went wrong, how bad it is, and which mesh it \
                 belongs to, one row each. Empty when every enabled check passed.",
            ),
        ],
        params: params_with(
            "Validate",
            vec![
                check(
                    "normals",
                    "Normals",
                    "Flags a mesh whose normal count disagrees with its vertex count, \
                     and triangles whose stored normals point away from the surface \
                     their winding describes (more than about 120 degrees off). \
                     Inside-out geometry is the usual cause and `compute_normals` with \
                     Flip Orientation is the usual fix.",
                ),
                check(
                    "uvs",
                    "UVs",
                    "Flags a UV buffer whose length disagrees with the vertex count. \
                     Geometry carrying no UVs at all is not flagged by default: that \
                     warning normally depends on the source file format expecting \
                     them, and cooked geometry has no source format. Turn on Require \
                     UVs to flag it anyway. Use `uv_project` to add UVs.",
                ),
                ParamSpec::new(
                    "require_uvs",
                    "Require UVs",
                    "checks",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .show_if("uvs", Pred::Truthy)
                .doc(
                    "Off by default. When on, flags any mesh with no texture \
                     coordinates at all, regardless of source format, so you can \
                     ask whether cooked geometry lacks UVs before texturing or \
                     exporting it. Leave it off for procedural geometry that is \
                     legitimately UV-less. Only has an effect while UVs is on.",
                ),
                check(
                    "topology",
                    "Topology",
                    "Flags the structural defects: an empty or non-triangulated index \
                     buffer, an index pointing past the end of the vertices, zero-area \
                     triangles, edges shared by three or more triangles, and boundary \
                     edges. Boundary edges mean an open mesh, which warns here with no \
                     way to allow it, so expect them on a plane or a scan.",
                ),
                check(
                    "materials",
                    "Materials",
                    "Flags a mesh pointing at a material index its set does not have. \
                     `merge` already clears such a reference to none as it \
                     concatenates, so in practice this catches imports.",
                ),
                check(
                    "budget",
                    "Triangle Budget",
                    "Turns the Budget comparison on. It is on by default but silent \
                     until Budget itself is above 0, so switching it off only matters \
                     once you have set a number.",
                ),
                ParamSpec::new(
                    "triangle_budget",
                    "Budget",
                    "checks",
                    ParamType::Int,
                    ParamValue::Int(0),
                )
                .hard(0.0, 2_000_000_000.0)
                .show_if("budget", Pred::Truthy)
                .doc(
                    "How many triangles this geometry is allowed. 0 means no limit, \
                     which is why the check says nothing until you set one. Going over \
                     is a warning up to 20 percent above the number and an error beyond \
                     that. The count is the whole input set, not per mesh.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Runs the Solarxy validation checks over the input and emits what \
              it finds on the `report` output, while the geometry itself \
              passes through on `geometry` completely unchanged. Each toggle \
              turns on a group of related checks rather than a single \
              one.\n\n\
              It is the only node with two outputs, and that is what makes it \
              droppable into the middle of a chain you have already built: \
              wire it between a modeling branch and an output and the result \
              is identical, you just gain the report. Put one after an import \
              to see what the file arrived with; the fixes usually live in \
              `compute_normals` and `uv_project`.\n\n\
              Nothing here repairs anything, so a reported problem stays \
              reported until you add the node that fixes it. Above 250,000 \
              input triangles the checks move off the cook thread onto a \
              background worker, so on heavy geometry the report lands a \
              moment after the geometry does.",
        search_aliases: &["validate", "check", "lint", "inspect"],
        glyph: "validate",
        role: NodeRole::Analyzer,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

fn check(key: &str, label: &str, doc: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        "checks",
        ParamType::Bool,
        ParamValue::Bool(true),
    )
    .doc(doc)
}

/// Above this input triangle count the validate node offloads to an async
/// `ValidateGeometry` job (the web worker) instead of validating inline on
/// the cook thread. Below it, inline validation is cheap enough to keep the
/// realtime UX contract.
const ASYNC_TRIANGLE_THRESHOLD: u64 = 250_000;

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
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
        uv_presence_forced: p.bool("require_uvs"),
        index_buffer: p.bool("topology"),
    };
    let budget = p.i64("triangle_budget");
    let budget = if p.bool("budget") && budget > 0 {
        Some(budget.min(i64::from(u32::MAX)) as u32)
    } else {
        None
    };

    // Heavy inputs validate off-thread: the geometry rides the request as
    // an `Arc`, the driver retains it for the passthrough commit, and the
    // result arrives through the same generation-guarded job protocol as
    // imports.
    if cx.async_jobs && input.triangle_count() > ASYNC_TRIANGLE_THRESHOLD {
        return Ok(CookOutcome::Pending(JobRequest::ValidateGeometry {
            geometry: Arc::clone(input),
            config,
            budget,
        }));
    }

    // The GeometrySet -> RawModelData adapter (round-trip tested in the
    // kernel), then the existing pipeline.
    let raw = input.to_raw();
    let thresholds = ValidationThresholds::default();
    let result = validate_raw_model_with_config(&raw, "", &config, &thresholds, budget);
    let report = Arc::new(result.report.clone());
    // The full result (report + degenerate-face lists) rides the driver's
    // validation cache for the per-object overlay.
    cx.set_validation(result);

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

    #[test]
    fn require_uvs_flags_uvless_geometry_only_when_on() {
        use crate::params::ParamSource;
        // A valid quad carrying no UVs. Cooked geometry has no source format,
        // so the missing-UV warning is unreachable unless require_uvs forces
        // it. (The open-mesh boundary warning is present either way; we count
        // only MissingUvs.)
        let uvless = || {
            GeometrySet::from_mesh(KernelMesh::new(
                "quad",
                vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [0.0, 1.0, 0.0],
                ],
                vec![0, 1, 2, 0, 2, 3],
            ))
        };
        let missing_uvs = |require: bool| -> usize {
            let mut overrides = BTreeMap::new();
            if require {
                overrides.insert(
                    "require_uvs".to_string(),
                    ParamSource::Literal(ParamValue::Bool(true)),
                );
            }
            let resolved =
                crate::registry::resolve::resolve_params(&overrides, &descriptor().params).unwrap();
            let mut slots = BTreeMap::new();
            slots.insert(
                "geometry".to_string(),
                InputSlot::Single(Value::Geometry(Arc::new(uvless()))),
            );
            let inputs = Inputs::new(slots);
            let assets = crate::assets::AssetTable::new();
            let mut cx = CookCtx::new(&assets, false);
            let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
                panic!("small input cooks synchronously");
            };
            match out.get("report") {
                Some(Value::Report(r)) => r
                    .issues
                    .iter()
                    .filter(|i| i.kind == solarxy_core::validation::IssueKind::MissingUvs)
                    .count(),
                _ => panic!("validate emits a report"),
            }
        };
        assert_eq!(missing_uvs(false), 0, "quiet by default");
        assert_eq!(missing_uvs(true), 1, "require_uvs flags the UV-less mesh");
    }

    #[test]
    fn inline_cook_records_the_full_result_on_the_side_channel() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        // One degenerate triangle so the result carries degenerate faces.
        let mesh = KernelMesh::new("d", vec![[0.0; 3], [1.0, 0.0, 0.0]], vec![0, 0, 0]);
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(mesh)))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(_) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("inline path cooks synchronously");
        };
        let result = cx.take_validation().expect("side-channel recorded");
        assert!(result.degenerate_faces.iter().any(|f| !f.is_empty()));
    }

    #[test]
    fn heavy_input_parks_pending_with_a_validate_job() {
        // 250_001 (degenerate) triangles over three vertices: cheap to
        // build, over the async threshold.
        let count = ASYNC_TRIANGLE_THRESHOLD as u32 + 1;
        let indices: Vec<u32> = (0..count * 3).map(|i| i % 3).collect();
        let mesh = KernelMesh::new(
            "big",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            indices,
        );
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(mesh)))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        // Async available: the node must offload rather than block a cook.
        let mut cx = CookCtx::new(&assets, true);
        match cook(&resolved, &inputs, &mut cx).unwrap() {
            CookOutcome::Pending(JobRequest::ValidateGeometry { budget, .. }) => {
                assert!(budget.is_none(), "budget check defaults to un-budgeted");
            }
            _ => panic!("expected Pending(ValidateGeometry)"),
        }
        // Async unavailable: the same input validates inline.
        let mut cx = CookCtx::new(&assets, false);
        assert!(matches!(
            cook(&resolved, &inputs, &mut cx).unwrap(),
            CookOutcome::Done(_)
        ));
    }
}
