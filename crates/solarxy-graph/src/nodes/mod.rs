//! The node catalog: one file per node type, each exposing `descriptor()`
//! (and its cook function, optional migrate hook, and unit tests). The
//! builtin registry collects them.
//!
//! Adding a node is two touch points (node catalog part I, section 10):
//! create `nodes/<type_id>.rs` with its `descriptor()`, then add one line
//! to [`builtin_descriptors`]. The registry invariants validate it; the
//! palette, handles, and parameter panel pick it up from the snapshot with
//! no frontend change.

// The scene lowering reaches in for `rotate_order_from_key`: the geo's world
// matrix must read the same order the transform node's cook does.
pub(crate) mod common;

// Primitives (subflow).
mod box_node;
mod cone_node;
mod cylinder_node;
mod plane_node;
mod sphere_node;
mod torus_knot_node;
mod torus_node;

// Modifiers (subflow).
mod array_node;
mod compute_normals_node;
mod delete_node;
mod material_node;
mod merge_node;
mod mirror_node;
mod subdivide_node;
mod transform_node;
mod uv_project_node;
mod validate_node;

// Utility (subflow).
mod bounds_node;
mod null_node;
mod switch_node;

// Container + utility + lights (root) and imports (subflow).
mod camera_node;
mod geo_node;
mod import_image;
mod imports;
mod lights;
mod note_node;

pub use imports::{parse_bytes, parse_model, parse_model_validated};

use crate::GraphError;
use crate::registry::{NodeTypeDescriptor, Registry};

/// Every builtin descriptor, in registration order. The 3a set is the ten
/// subflow nodes that exercise the cook core; the remaining thirteen
/// (container, lights, note, imports, validate) register here too as they
/// land.
#[must_use]
pub fn builtin_descriptors() -> Vec<NodeTypeDescriptor> {
    vec![
        // Primitives (subflow).
        box_node::descriptor(),
        sphere_node::descriptor(),
        cylinder_node::descriptor(),
        cone_node::descriptor(),
        plane_node::descriptor(),
        torus_node::descriptor(),
        torus_knot_node::descriptor(),
        // Modifiers (subflow).
        transform_node::descriptor(),
        merge_node::descriptor(),
        compute_normals_node::descriptor(),
        validate_node::descriptor(),
        material_node::descriptor(),
        uv_project_node::descriptor(),
        subdivide_node::descriptor(),
        array_node::descriptor(),
        mirror_node::descriptor(),
        delete_node::descriptor(),
        // Utility (subflow).
        null_node::descriptor(),
        switch_node::descriptor(),
        bounds_node::descriptor(),
        // Imports (subflow).
        imports::obj_descriptor(),
        imports::gltf_descriptor(),
        imports::stl_descriptor(),
        imports::ply_descriptor(),
        import_image::descriptor(),
        // Container + utility (root/both).
        geo_node::descriptor(),
        note_node::descriptor(),
        camera_node::camera_descriptor(),
        // Lights (root).
        lights::point_descriptor(),
        lights::directional_descriptor(),
        lights::spot_descriptor(),
        lights::ambient_descriptor(),
        lights::hemisphere_descriptor(),
        lights::rect_area_descriptor(),
    ]
}

/// Builds the validated builtin registry (fails only if a descriptor
/// violates an invariant, which the tests catch first).
pub fn builtin_registry() -> Result<Registry, GraphError> {
    Registry::with_descriptors(builtin_descriptors())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_is_valid() {
        // Construction enforces every registry invariant.
        let registry = builtin_registry().expect("builtin registry must satisfy its invariants");
        assert_eq!(registry.len(), builtin_descriptors().len());
    }

    #[test]
    fn every_geometry_producing_node_has_a_default_geometry_output() {
        let registry = builtin_registry().unwrap();
        for desc in registry.descriptors() {
            // Root-context nodes (geo, note, lights) are portless; only
            // nodes that declare outputs must have a default output, and
            // geometry-producing ones must default to the `geometry` port
            // (import_image produces Image, not geometry).
            if desc.outputs.is_empty() {
                continue;
            }
            let out = desc
                .default_output()
                .unwrap_or_else(|| panic!("{}: no default output", desc.type_id));
            if desc
                .outputs
                .iter()
                .any(|o| o.data_type == crate::registry::coerce::DataType::Geometry)
            {
                assert_eq!(out.key, "geometry", "{}", desc.type_id);
            }
        }
    }

    #[test]
    fn all_builtin_nodes_registered() {
        let registry = builtin_registry().unwrap();
        // The 23 MVP node types plus import_image (Phase 13), the Phase 14
        // wave (material, uv_project, subdivide), and the Phase 15 modeling
        // wave (array, mirror, delete, null, switch, bounds).
        assert_eq!(registry.len(), 34);
    }
}
