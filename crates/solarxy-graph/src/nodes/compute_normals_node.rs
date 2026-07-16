//! The `compute_normals` modifier (node catalog part II, section 13).
//! Recomputes vertex normals via the core face-normal-accumulation kernel;
//! `flip_orientation` reverses winding and negates normals.

use std::sync::Arc;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "compute_normals",
        version: 2,
        display_name: "Compute Normals",
        category: Category::Modifiers,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry whose normals to recompute."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Compute Normals",
            vec![
                ParamSpec::new(
                    "flip_orientation",
                    "Flip Orientation",
                    "geometry",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Reverses triangle winding and negates normals \
                      (fixes inside-out meshes).",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Recomputes per-vertex normals from the triangle topology.",
        search_aliases: &["normals", "recompute", "smooth"],
        glyph: "compute_normals",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let flip = p.bool("flip_orientation");

    // Clone the set shell; rewrite only the buffers we touch (indices on a
    // flip, normals always), sharing positions/UVs by refcount.
    let mut set = (**input).clone();
    for mesh in &mut set.meshes {
        if flip {
            let mut indices = (*mesh.indices).clone();
            for tri in indices.chunks_exact_mut(3) {
                tri.swap(1, 2);
            }
            mesh.indices = Arc::new(indices);
        }
        mesh.recompute_normals();
    }
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_kernel::{GeometrySet, KernelMesh};
    use std::collections::BTreeMap;

    fn one_triangle_input() -> Inputs {
        // A CCW triangle in the XY plane (faces +Z), no normals yet.
        let mesh = KernelMesh::new(
            "tri",
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(mesh)))),
        );
        Inputs::new(slots)
    }

    fn run(flip: bool) -> [f32; 3] {
        let mut stored = BTreeMap::new();
        stored.insert(
            "flip_orientation".to_string(),
            ParamSource::Literal(ParamValue::Bool(flip)),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &one_triangle_input(), &mut cx).unwrap()
        else {
            panic!("cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        set.meshes[0].normals.as_ref().unwrap()[0]
    }

    #[test]
    fn computes_outward_normal() {
        let n = run(false);
        assert!(n[2] > 0.99, "CCW triangle faces +Z, got {n:?}");
    }

    #[test]
    fn flip_reverses_the_normal() {
        let n = run(true);
        assert!(n[2] < -0.99, "flipped triangle faces -Z, got {n:?}");
    }
}
