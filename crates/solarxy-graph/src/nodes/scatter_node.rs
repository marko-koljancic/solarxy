//! The `scatter` modifier. Sprinkles seeded random points over the input's
//! triangle surfaces, area-weighted so density is even regardless of how
//! the surface is triangulated. The output is a Points-topology cloud
//! whose points inherit the surface's normal, UV, and color; it is the
//! canonical points input for `copy_to_points`.

use solarxy_kernel::scatter::scatter_weighted;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "scatter",
        // v2: gains `density`. Additive, so the registry default fill
        // supplies the empty name and an existing scatter is unchanged.
        version: 2,
        display_name: "Scatter",
        category: Category::Copy,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc(
                    "The surface to scatter points over. Only triangle meshes have \
                      area; line and point inputs contribute nothing.",
                ),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Scatter",
            vec![
                ParamSpec::new(
                    "count",
                    "Count",
                    "scatter",
                    ParamType::Int,
                    ParamValue::Int(100),
                )
                .hard(1.0, 1_000_000.0)
                .soft(1.0, 10_000.0)
                .step(1.0)
                .doc(
                    "How many points to place. The hard ceiling of one million \
                     keeps a mistyped count from stalling the cook; past about \
                     ten thousand, expect the point display itself to become \
                     the cost.",
                ),
                ParamSpec::new(
                    "seed",
                    "Seed",
                    "scatter",
                    ParamType::Int,
                    ParamValue::Int(0),
                )
                .hard(0.0, 2_147_483_647.0)
                .soft(0.0, 9999.0)
                .step(1.0)
                .doc(
                    "Selects which random placement you get. Any change gives a \
                     completely different arrangement rather than a shifted one, \
                     so scrub it to hunt for one you like. The same seed always \
                     cooks the same points, which is what lets a saved scene \
                     reproduce exactly.",
                ),
                ParamSpec::new(
                    "density",
                    "Density Attribute",
                    "scatter",
                    ParamType::AttributeName,
                    ParamValue::Text(String::new()),
                )
                .doc(
                    "A float point attribute that biases WHERE the points \
                     land. Empty (the default) scatters by area alone. \
                     Named, each triangle is weighted by its area times the \
                     mean of the attribute at its three corners, so twice the \
                     value gathers roughly twice the points.\n\n\
                     Author it with `attribute_wrangle`: \
                     `@density = fit(@P.y, 0, 1, 0, 1);` gathers points \
                     toward the top of a surface, and `@density = @Cd.r;` \
                     follows a texture already on the geometry. Zero means \
                     never; negative clamps to zero rather than flipping the \
                     weight.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Sprinkles Count random points over the input's triangle surfaces \
              and outputs them as a point cloud. Placement is area-weighted: a \
              big face receives proportionally more points than a small one, so \
              density stays even no matter how the surface happens to be \
              triangulated, and the same Seed always reproduces the same \
              arrangement.\n\n\
              Each point inherits the surface under it: the interpolated \
              normal (so copies can orient to the surface downstream), the UV, \
              and the vertex color when the source carries one. Feed the cloud \
              to `copy_to_points` to stamp a template onto every point, or use \
              it directly as a visible dressing of the surface.\n\n\
              Only triangles have area, so line and point inputs scatter \
              nothing and the node warns instead of guessing. Points draw at a \
              uniform screen-space size and are unpickable in the viewport; \
              select them on the node canvas.",
        search_aliases: &[
            "points",
            "distribute",
            "sprinkle",
            "sample",
            "random",
            "spray",
        ],
        glyph: "scatter",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    let density = p.text("density").trim().to_string();
    // Asked BEFORE scattering, because afterwards the two cases are
    // indistinguishable: an unresolved lane falls back to area-only weighting
    // and produces a perfectly ordinary even scatter, so a typo looks exactly
    // like success.
    if !density.is_empty() && !solarxy_kernel::scatter::density_lane_resolves(input, &density) {
        cx.warn(format!(
            "no float point attribute named `{density}` on the incoming geometry, so the \
             scatter is weighted by area alone. Check the name against the Attributes \
             pane, or author the lane with `attribute_wrangle`."
        ));
    }
    let out = scatter_weighted(
        input,
        p.u32("count"),
        p.u32("seed"),
        (!density.is_empty()).then_some(density.as_str()),
    );
    if out.is_renderable_empty() {
        if density.is_empty() {
            cx.warn("scatter found no triangle surface to sample; output is empty");
        } else {
            // Distinguishing the two is the whole point: "no surface" and
            // "your density is zero everywhere" need different fixes, and
            // falling back to an even scatter would hide the second.
            cx.warn(format!(
                "scatter produced nothing: either there is no triangle surface to \
                 sample, or `{density}` is zero across all of it"
            ));
        }
    }
    Ok(CookOutcome::Done(Outputs::geometry(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_core::geometry::MeshTopology;
    use solarxy_kernel::primitives::generate_plane;
    use solarxy_kernel::{GeometrySet, KernelMesh};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(stored: BTreeMap<String, ParamSource>, input: GeometrySet) -> (Outputs, Vec<String>) {
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
            panic!("scatter cooks synchronously");
        };
        (out, cx.take_warnings())
    }

    fn set_of(out: &Outputs) -> &Arc<GeometrySet> {
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        set
    }

    #[test]
    fn a_mistyped_density_lane_warns_instead_of_scattering_evenly_in_silence() {
        // The failure this guards: an unresolved lane falls back to area-only
        // weighting, which looks exactly like a working scatter, so without a
        // warning a typo is invisible.
        let mut stored = BTreeMap::new();
        stored.insert(
            "density".to_string(),
            ParamSource::Literal(ParamValue::Text("denstiy".into())),
        );
        let (out, warnings) = run(
            stored,
            GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1)),
        );
        assert!(
            !set_of(&out).is_renderable_empty(),
            "it still scatters, which is why the warning matters"
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("denstiy"),
            "the warning names it: {warnings:?}"
        );
    }

    #[test]
    fn an_empty_density_field_is_not_a_warning() {
        // The default. Scattering by area alone is the normal case, not a
        // mistake, and warning about it would train people to ignore warnings.
        let (_, warnings) = run(
            BTreeMap::new(),
            GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1)),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn cooks_a_point_cloud_at_defaults() {
        let (out, warnings) = run(
            BTreeMap::new(),
            GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1)),
        );
        let set = set_of(&out);
        assert_eq!(set.meshes[0].topology, MeshTopology::Points);
        assert_eq!(set.meshes[0].vertex_count(), 100, "default count");
        assert!(warnings.is_empty());
    }

    #[test]
    fn count_and_seed_params_drive_the_kernel() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "count".to_string(),
            ParamSource::Literal(ParamValue::Int(17)),
        );
        stored.insert("seed".to_string(), ParamSource::Literal(ParamValue::Int(3)));
        let plane = GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1));
        let (out, _) = run(stored.clone(), plane.clone());
        assert_eq!(set_of(&out).meshes[0].vertex_count(), 17);

        stored.insert("seed".to_string(), ParamSource::Literal(ParamValue::Int(4)));
        let (out2, _) = run(stored, plane);
        assert_ne!(
            set_of(&out).meshes[0].positions,
            set_of(&out2).meshes[0].positions,
            "reseeding moves the points"
        );
    }

    #[test]
    fn an_area_less_input_warns_and_outputs_empty() {
        let cloud = GeometrySet::from_mesh(KernelMesh::points("p", vec![[0.0; 3]; 4]));
        let (out, warnings) = run(BTreeMap::new(), cloud);
        assert!(set_of(&out).is_renderable_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no triangle surface"), "{warnings:?}");
    }
}
