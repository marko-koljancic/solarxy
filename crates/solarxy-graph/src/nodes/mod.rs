//! The node catalog: one file per node type, each exposing `descriptor()`
//! (and its cook function, optional migrate hook, and unit tests). The
//! builtin registry collects them.
//!
//! Adding a node is two touch points:
//! create `nodes/<type_id>.rs` with its `descriptor()`, then add one line
//! to [`builtin_descriptors`]. The registry invariants validate it; the
//! palette, handles, and parameter panel pick it up from the snapshot with
//! no frontend change.

// The scene lowering reaches in for `rotate_order_from_key`: the geo's world
// matrix must read the same order the transform node's cook does.
pub(crate) mod common;

// Primitives (subflow).
mod box_node;
mod circle_node;
mod cone_node;
mod cylinder_node;
mod line_node;
mod plane_node;
mod sphere_node;
mod torus_knot_node;
mod torus_node;

// Modifiers (subflow).
mod array_node;
mod attribute_copy_node;
mod attribute_create_node;
mod attribute_from_image_node;
mod attribute_promote_node;
mod attribute_randomize_node;
mod attribute_wrangle_node;
mod compute_normals_node;
mod copy_to_points_node;
mod delete_node;
mod displace_node;
mod edges_to_geo_node;
mod material_node;
mod merge_node;
mod mirror_node;
mod points_from_geo_node;
mod scatter_node;
mod subdivide_node;
mod transform_node;
mod uv_project_node;
mod validate_node;

// Utility (subflow).
mod bounds_node;
mod null_node;
mod switch_node;

// Material context: the container plus the surface nodes.
mod mat_nodes;

// Output: export taps and the render config node.
mod export_nodes;

// Texture context: the container plus the image ops.
mod image_adjust;
mod image_generate;
mod image_ops;
pub(crate) mod image_support;
mod texnet_node;

// Container + utility + lights (root) and imports (subflow).
mod camera_node;
mod geo_node;
mod import_image;
mod imports;
mod lights;
mod note_node;
mod text_node;

pub use imports::{parse_bytes, parse_model, parse_model_validated};

use crate::GraphError;
use crate::registry::{NodeTypeDescriptor, Registry};

/// Every builtin descriptor, grouped by category for bookkeeping. The
/// registry stores descriptors keyed by type id (a `BTreeMap`), so the
/// snapshot lists nodes alphabetically regardless of this order; category
/// presentation order is the frontend's `CATEGORY_ORDER`.
#[must_use]
pub fn builtin_descriptors() -> Vec<NodeTypeDescriptor> {
    vec![
        // Containers.
        geo_node::descriptor(),
        mat_nodes::matnet_descriptor(),
        texnet_node::descriptor(),
        // Generators (Geo).
        box_node::descriptor(),
        sphere_node::descriptor(),
        cylinder_node::descriptor(),
        cone_node::descriptor(),
        plane_node::descriptor(),
        torus_node::descriptor(),
        torus_knot_node::descriptor(),
        line_node::descriptor(),
        circle_node::descriptor(),
        // Attribute (Geo).
        attribute_create_node::descriptor(),
        attribute_randomize_node::descriptor(),
        attribute_wrangle_node::descriptor(),
        attribute_promote_node::descriptor(),
        attribute_copy_node::descriptor(),
        attribute_from_image_node::descriptor(),
        compute_normals_node::descriptor(),
        uv_project_node::descriptor(),
        // Transform (Geo).
        transform_node::descriptor(),
        displace_node::descriptor(),
        // Copy & Instance (Geo).
        array_node::descriptor(),
        copy_to_points_node::descriptor(),
        scatter_node::descriptor(),
        mirror_node::descriptor(),
        // Topology (Geo).
        merge_node::descriptor(),
        subdivide_node::descriptor(),
        delete_node::descriptor(),
        points_from_geo_node::descriptor(),
        edges_to_geo_node::descriptor(),
        // Shaders (Geo binding + Mat surfaces).
        material_node::descriptor(),
        mat_nodes::principled_descriptor(),
        mat_nodes::matcap_descriptor(),
        mat_nodes::toon_descriptor(),
        mat_nodes::unlit_descriptor(),
        mat_nodes::mix_material_descriptor(),
        // Import.
        imports::obj_descriptor(),
        imports::gltf_descriptor(),
        imports::stl_descriptor(),
        imports::ply_descriptor(),
        import_image::descriptor(),
        mat_nodes::tex_ref_descriptor(),
        // Export.
        export_nodes::geo_export_descriptor(),
        export_nodes::image_export_descriptor(),
        export_nodes::render_descriptor(),
        // Lights (root).
        lights::point_descriptor(),
        lights::directional_descriptor(),
        lights::spot_descriptor(),
        lights::ambient_descriptor(),
        lights::hemisphere_descriptor(),
        lights::rect_area_descriptor(),
        // Utility.
        null_node::descriptor(),
        switch_node::descriptor(),
        bounds_node::descriptor(),
        validate_node::descriptor(),
        camera_node::camera_descriptor(),
        note_node::descriptor(),
        text_node::descriptor(),
        // Texture: Generate.
        image_generate::constant_descriptor(),
        image_generate::ramp_descriptor(),
        image_generate::noise_descriptor(),
        image_generate::voronoi_descriptor(),
        image_generate::gradient_descriptor(),
        image_generate::checker_descriptor(),
        image_generate::brick_descriptor(),
        // Texture: Adjust.
        image_adjust::levels_descriptor(),
        image_adjust::brightness_contrast_descriptor(),
        image_adjust::hue_saturation_descriptor(),
        image_adjust::invert_descriptor(),
        image_adjust::gamma_descriptor(),
        // Texture: Composite.
        image_ops::mix_descriptor(),
        image_ops::blur_descriptor(),
        image_ops::sharpen_descriptor(),
        image_ops::pack_orm_descriptor(),
        image_ops::height_to_normal_descriptor(),
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
    use crate::document::ContextKind;

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

    /// The registry's size is asserted rather than described, because a
    /// description goes stale silently: this comment used to enumerate the
    /// waves that built the set, and its hand-arithmetic drifted from the
    /// assert below. The counts per context are the useful fact, and they are
    /// derived here rather than recited.
    #[test]
    fn all_builtin_nodes_registered() {
        let registry = builtin_registry().unwrap();
        assert_eq!(registry.len(), 76);

        let in_context = |kind: ContextKind| {
            builtin_descriptors()
                .iter()
                .filter(|d| d.contexts.contains(kind))
                .count()
        };
        assert!(in_context(ContextKind::Obj) > 0, "the Obj context is empty");
        assert!(in_context(ContextKind::Geo) > 0, "the Geo context is empty");
        assert!(in_context(ContextKind::Mat) > 0, "the Mat context is empty");
        assert!(in_context(ContextKind::Tex) > 0, "the Tex context is empty");
    }

    /// The taxonomy is presentation: pinning the per-category counts makes
    /// a category change a deliberate act (edit this table) rather than a
    /// side effect of an unrelated descriptor edit.
    #[test]
    fn the_taxonomy_holds_its_assigned_counts() {
        use crate::registry::Category;

        let count = |cat: Category| {
            builtin_descriptors()
                .iter()
                .filter(|d| d.category == cat)
                .count()
        };
        let expected = [
            (Category::Container, 3),
            (Category::Generators, 9),
            (Category::Attribute, 8),
            (Category::Transform, 2),
            (Category::Copy, 4),
            (Category::Topology, 5),
            (Category::Shaders, 6),
            (Category::Import, 6),
            (Category::Export, 3),
            (Category::Lights, 6),
            // 0.8.1: `camera` moved out of Utility into its own section.
            (Category::Cameras, 1),
            // +1: the `text` datablock joined in round 2.
            (Category::Utility, 6),
            (Category::TexGenerate, 7),
            (Category::TexAdjust, 5),
            (Category::TexComposite, 5),
        ];
        for (cat, n) in expected {
            assert_eq!(count(cat), n, "{}", cat.display_name());
        }
        assert_eq!(expected.iter().map(|(_, n)| n).sum::<usize>(), 76);
    }
}
