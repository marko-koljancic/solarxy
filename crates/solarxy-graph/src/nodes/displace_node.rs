//! The `displace` deformer: moves points along a chosen direction scaled
//! by a constant amplitude times an optional scalar attribute lane. The
//! attribute-driven counterpart of `transform`: where transform moves the
//! whole mesh by one matrix, displace moves every point by its own data.

use solarxy_kernel::deform_ops::displace_mesh;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::engine::attr_table::resolve_lane;
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Pred};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "displace",
        version: 1,
        display_name: "Displace",
        category: Category::Transform,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry whose points move."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Displace",
            vec![
                ParamSpec::new(
                    "direction",
                    "Direction",
                    "displace",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("normal", "Normal"),
                            EnumVariant::new("vector", "Vector"),
                            EnumVariant::new("attribute", "Attribute"),
                        ],
                    },
                    ParamValue::Enum("normal".into()),
                )
                .doc(
                    "Where each point's movement direction comes from: the \
                     point normal (`N`), one constant vector for every \
                     point, or a vec3/vec4 attribute lane by name.",
                ),
                ParamSpec::new(
                    "vector",
                    "Vector",
                    "displace",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0, 1.0, 0.0]),
                )
                .show_if("direction", Pred::Eq(ParamValue::Enum("vector".into())))
                .doc("The constant direction every point moves along."),
                ParamSpec::new(
                    "direction_attr",
                    "Direction Attribute",
                    "displace",
                    ParamType::AttributeName,
                    ParamValue::Text("N".into()),
                )
                .show_if("direction", Pred::Eq(ParamValue::Enum("attribute".into())))
                .doc(
                    "The vec3 (or vec4, xyz) point lane supplying each \
                     point's direction.",
                ),
                ParamSpec::new(
                    "amplitude",
                    "Amplitude",
                    "displace",
                    ParamType::Float,
                    ParamValue::Float(0.1),
                )
                .hard(-10000.0, 10000.0)
                .soft(-2.0, 2.0)
                .doc(
                    "How far each point moves, in metres along its (unit) \
                     direction. Negative pushes inward. With an amplitude \
                     attribute set, this multiplies the lane's value.",
                ),
                ParamSpec::new(
                    "amp_attr",
                    "Amplitude Attribute",
                    "displace",
                    ParamType::AttributeName,
                    ParamValue::Text(String::new()),
                )
                .doc(
                    "Optional float point lane multiplying the amplitude per \
                     point. Empty means the constant amplitude alone. This is \
                     the driving seat for attribute workflows: randomize a \
                     lane, or sample one from an image, and feed it here.",
                ),
                ParamSpec::new(
                    "normalize",
                    "Normalize Direction",
                    "displace",
                    ParamType::Bool,
                    ParamValue::Bool(true),
                )
                .doc(
                    "Unit-length each direction before scaling, so the \
                     amplitude is an honest distance. Off, a longer direction \
                     vector moves its point proportionally further.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Moves every point along a direction scaled by an amplitude: \
              the point normal by default, a constant vector, or a vec3 \
              attribute lane, with an optional float lane multiplying the \
              amplitude per point.\n\n\
              This is where attributes start driving shape: \
              `attribute_randomize` a float lane and feed it to Amplitude \
              Attribute for surface noise, or sample an image into a lane \
              with `attribute_from_image` for map-driven relief.\n\n\
              Normals are left as they were (deliberately, so chained \
              displaces compound predictably); chain `compute_normals` after \
              the last displace to relight the result. A mesh without a \
              usable direction source passes through with a warning.",
        search_aliases: &[
            "displacement",
            "deform",
            "push",
            "noise",
            "height",
            "relief",
        ],
        glyph: "displace",
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
    let amplitude = p.f32("amplitude");
    let normalize = p.bool("normalize");
    let vector = p.vec3_f32("vector");
    let mode = p.enum_key("direction");
    // The lane name a non-vector mode resolves per mesh; `normal` rides
    // the same resolver (fixed normals surface as the `N` pseudo-lane).
    let dir_name = match mode {
        "vector" => None,
        "attribute" => Some(p.text("direction_attr").trim().to_string()),
        _ => Some(solarxy_kernel::reserved::NORMAL.to_string()),
    };
    let amp_name = p.text("amp_attr").trim().to_string();

    let mut missing_dir = false;
    let mut bad_amp = false;
    let meshes: Vec<solarxy_kernel::KernelMesh> = input
        .meshes
        .iter()
        .map(|mesh| {
            let dir_lane = match dir_name.as_deref() {
                None => None,
                Some(name) => match resolve_lane(mesh, name) {
                    Some(lane) if matches!(lane.ty(), "vec3" | "vec4") => Some(lane),
                    _ => {
                        missing_dir = true;
                        return mesh.clone();
                    }
                },
            };
            let amp_lane = if amp_name.is_empty() {
                None
            } else {
                match resolve_lane(mesh, &amp_name) {
                    Some(lane) if lane.ty() == "float" => Some(lane),
                    _ => {
                        bad_amp = true;
                        None
                    }
                }
            };
            displace_mesh(
                mesh,
                |i| dir_lane.map_or(Some(vector), |lane| lane.direction(i)),
                |i| {
                    amplitude
                        * amp_lane.map_or(1.0, |lane| {
                            lane.component("float", i, 0).unwrap_or(0.0) as f32
                        })
                },
                normalize,
            )
        })
        .collect();

    if missing_dir {
        let name = dir_name.as_deref().unwrap_or("N");
        cx.warn(format!(
            "no vec3 `{name}` lane on at least one mesh; those meshes pass \
             through undisplaced"
        ));
    }
    if bad_amp {
        cx.warn(format!(
            "`{amp_name}` is not a float lane on at least one mesh; the \
             constant amplitude applies there"
        ));
    }
    Ok(CookOutcome::Done(Outputs::geometry(
        solarxy_kernel::GeometrySet::from_parts(meshes, input.materials.clone()),
    )))
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
        set: GeometrySet,
    ) -> (Arc<GeometrySet>, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(set))),
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
        (Arc::clone(set), cx.take_warnings())
    }

    fn lit(v: ParamValue) -> ParamSource {
        ParamSource::Literal(v)
    }

    #[test]
    fn displaces_a_box_outward_along_its_normals() {
        // A unit box's normals point away from the centre, so every
        // displaced point moves further from the origin.
        let set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let (out, warnings) = run(BTreeMap::new(), set.clone());
        assert!(warnings.is_empty(), "{warnings:?}");
        let before = &set.meshes[0].positions;
        let after = &out.meshes[0].positions;
        let d = |p: &[f32; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        for (a, b) in before.iter().zip(after.iter()) {
            assert!(d(b) > d(a), "moved outward: {a:?} -> {b:?}");
        }
        assert!(
            Arc::ptr_eq(&out.meshes[0].indices, &set.meshes[0].indices),
            "topology rides by refcount"
        );
    }

    #[test]
    fn a_float_lane_scales_the_amplitude_per_point() {
        let mut mesh = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        let count = mesh.positions.len();
        // First point amplitude 0, the rest 1.
        let mut lane = vec![1.0f32; count];
        lane[0] = 0.0;
        mesh.attributes.insert(
            "height".into(),
            solarxy_kernel::AttributeData::Float(Arc::new(lane)),
        );
        let set = GeometrySet::from_mesh(mesh);
        let mut stored = BTreeMap::new();
        stored.insert(
            "amp_attr".to_string(),
            lit(ParamValue::Text("height".into())),
        );
        let (out, warnings) = run(stored, set.clone());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            out.meshes[0].positions[0], set.meshes[0].positions[0],
            "amplitude 0 leaves the point in place"
        );
        assert_ne!(out.meshes[0].positions[1], set.meshes[0].positions[1]);
    }

    #[test]
    fn a_missing_direction_lane_passes_through_with_a_warning() {
        // A point cloud has no normals buffer and no `N` lane.
        let set = GeometrySet::from_mesh(solarxy_kernel::KernelMesh::points(
            "pts",
            vec![[0.0; 3], [1.0, 0.0, 0.0]],
        ));
        let (out, warnings) = run(BTreeMap::new(), set.clone());
        assert_eq!(*out.meshes[0].positions, *set.meshes[0].positions);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("N"), "{warnings:?}");
    }

    #[test]
    fn vector_mode_moves_every_point_the_same_way() {
        let set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let mut stored = BTreeMap::new();
        stored.insert(
            "direction".to_string(),
            lit(ParamValue::Enum("vector".into())),
        );
        stored.insert("vector".to_string(), lit(ParamValue::Vec3([0.0, 0.0, 3.0])));
        stored.insert("amplitude".to_string(), lit(ParamValue::Float(0.5)));
        let (out, warnings) = run(stored, set.clone());
        assert!(warnings.is_empty(), "{warnings:?}");
        for (a, b) in set.meshes[0]
            .positions
            .iter()
            .zip(out.meshes[0].positions.iter())
        {
            assert!((b[2] - a[2] - 0.5).abs() < 1e-6, "normalized 0.5 along z");
        }
    }
}
