//! The `compute_normals` modifier.
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
        doc: "Rebuilds every mesh's vertex normals from its triangles. Each \
              triangle's geometric normal is accumulated onto the three points \
              it touches and the sum is normalized, so a point shared by \
              several triangles ends up carrying their area-weighted \
              average.\n\n\
              Reach for it when an import arrives with no normals, or with \
              normals that disagree with the surface, or after something \
              upstream left them stale. The `validate` node's Normals check \
              reports exactly what this clears, so the two pair naturally: \
              validate to see the problem, compute_normals to fix it.\n\n\
              It can only smooth where points are actually shared. Primitives \
              split their corners so that each face carries its own copy -- a \
              box has 24 points for 8 corners -- and recomputing normals on \
              one leaves it just as flat-shaded as before. Smooth shading out \
              of split geometry needs the points welded first, which this node \
              does not do. Winding and face normals are triangle concepts: \
              point clouds and polylines pass through untouched with a \
              warning.",
        search_aliases: &["normals", "recompute", "smooth"],
        glyph: "compute_normals",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let flip = p.bool("flip_orientation");

    if input.has_non_triangle_meshes() {
        cx.warn(
            "compute_normals applies to triangle meshes; line and point meshes pass \
             through unchanged",
        );
    }

    // Clone the set shell; rewrite only the buffers we touch (indices on a
    // flip, normals always), sharing positions/UVs by refcount. Winding and
    // face normals are triangle concepts, so line and point meshes are left
    // alone (the flip's triple-swap would corrupt a pair list).
    let mut set = (**input).clone();
    for mesh in &mut set.meshes {
        if mesh.topology != solarxy_core::MeshTopology::Triangles {
            continue;
        }
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

    /// A polyline through a flipping compute_normals must come out intact:
    /// the triple-swap over a pair list would scramble segment order, and
    /// no normals should be invented for it.
    #[test]
    fn a_polyline_warns_and_survives_a_flip_untouched() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "flip_orientation".to_string(),
            ParamSource::Literal(ParamValue::Bool(true)),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let line = KernelMesh::polyline("l", vec![[0.0; 3], [1.0; 3], [2.0; 3]], vec![0, 1, 1, 2]);
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(line)))),
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
        assert_eq!(*set.meshes[0].indices, vec![0, 1, 1, 2], "pairs intact");
        assert!(set.meshes[0].normals.is_none(), "no normals invented");
        let warns = cx.take_warnings();
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("pass"), "got: {}", warns[0]);
    }
}
