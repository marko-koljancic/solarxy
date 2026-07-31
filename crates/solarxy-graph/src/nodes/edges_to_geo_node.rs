//! The `edges_to_geo` modifier: the unique edges of the input as line
//! segments, a structural wireframe that exists as real geometry rather
//! than a display overlay.

use solarxy_kernel::edges_to_geo::edges_to_geo;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::registry::coerce::DataType;
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "edges_to_geo",
        version: 1,
        display_name: "Edges to Geo",
        category: Category::Topology,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry whose edges become line segments."),
        ],
        outputs: vec![geometry_output()],
        params: params_with("Edges to Geo", vec![]),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Extracts every unique edge of the input as a real line segment: \
              a triangle mesh becomes its wireframe, drawn unlit at one pixel, \
              and shared edges appear once rather than once per neighboring \
              triangle.\n\n\
              Unlike the wireframe display overlay, this output is geometry: \
              it survives export, feeds downstream modifiers, and its points \
              carry the source's attributes, so a colored scan's wireframe \
              stays colored. Line inputs pass through with duplicate segments \
              folded; point clouds have no edges and contribute nothing.\n\n\
              A typical inspection chain pairs it with `points_from_geo`: \
              edges show the connectivity, points show the sampling. Materials \
              are dropped in the conversion, and wires are unpickable in the \
              viewport.",
        search_aliases: &["wireframe", "wire", "outline", "skeleton", "convert"],
        glyph: "edges_to_geo",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(_p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let input = &super::common::baked_input(input, cx)?;
    let out = edges_to_geo(input);
    if out.is_renderable_empty() && !input.meshes.is_empty() {
        cx.warn("edges_to_geo found no edges (point clouds have none); output is empty");
    }
    Ok(CookOutcome::Done(Outputs::geometry(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::registry::coerce::Value;
    use solarxy_core::geometry::MeshTopology;
    use solarxy_kernel::primitives::generate_box;
    use solarxy_kernel::{GeometrySet, KernelMesh};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(input: GeometrySet) -> (Outputs, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(input))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("cooks synchronously");
        };
        (out, cx.take_warnings())
    }

    #[test]
    fn a_box_cooks_into_its_wireframe() {
        let (out, warnings) = run(GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1)));
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        assert_eq!(set.meshes[0].topology, MeshTopology::Lines);
        assert!(set.meshes[0].is_renderable());
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_point_cloud_warns_and_outputs_empty() {
        let (out, warnings) = run(GeometrySet::from_mesh(KernelMesh::points(
            "p",
            vec![[0.0; 3]; 4],
        )));
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        assert!(set.is_renderable_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no edges"), "{warnings:?}");
    }
}
