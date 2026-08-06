//! The `attribute_promote` modifier: converts a lane between the point
//! and primitive domains. The first primitive-domain producer, which is
//! what makes the spreadsheet's Primitive tab carry live data.

use solarxy_kernel::attribute_ops::{
    PromoteMethod, promote_point_to_primitive, promote_primitive_to_point,
};
use solarxy_kernel::{AttributeData, KernelMesh};

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::engine::attr_table::{LaneRef, resolve_lane};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

/// Materializes a resolved point lane as owned data: map lanes by `Arc`
/// bump, the fixed normal/uv buffers by copy (promotion needs a value
/// buffer to combine from either way).
fn lane_data(lane: LaneRef<'_>) -> AttributeData {
    match lane {
        LaneRef::Map(data) => data.clone(),
        LaneRef::Normals(v) => AttributeData::Vec3(std::sync::Arc::new(v.to_vec())),
        LaneRef::Uvs(v) => AttributeData::Vec2(std::sync::Arc::new(v.to_vec())),
    }
}

fn method_of(key: &str) -> PromoteMethod {
    match key {
        "min" => PromoteMethod::Min,
        "max" => PromoteMethod::Max,
        "first" => PromoteMethod::First,
        _ => PromoteMethod::Average,
    }
}

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "attribute_promote",
        version: 1,
        display_name: "Attribute Promote",
        category: Category::Attribute,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry whose lane changes domain."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Attribute Promote",
            vec![
                ParamSpec::new(
                    "attr_name",
                    "Name",
                    "attribute",
                    ParamType::AttributeName,
                    ParamValue::Text(String::new()),
                )
                .doc("The lane to promote, resolved in the source domain."),
                ParamSpec::new(
                    "direction",
                    "Direction",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("pointToPrimitive", "Point to Primitive"),
                            EnumVariant::new("primitiveToPoint", "Primitive to Point"),
                        ],
                    },
                    ParamValue::Enum("pointToPrimitive".into()),
                )
                .doc(
                    "Which way the lane moves: corner-point values combining \
                     into one value per primitive, or primitive values \
                     spreading onto their points.",
                ),
                ParamSpec::new(
                    "method",
                    "Method",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("average", "Average"),
                            EnumVariant::new("min", "Minimum"),
                            EnumVariant::new("max", "Maximum"),
                            EnumVariant::new("first", "First"),
                        ],
                    },
                    ParamValue::Enum("average".into()),
                )
                .doc(
                    "How the several source values landing on one destination \
                     element combine, component-wise for vector lanes.",
                ),
                ParamSpec::new(
                    "keep_original",
                    "Keep Original",
                    "attribute",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Keep the source-domain lane beside the promoted one. Off \
                     (the default), the promotion MOVES the lane. The fixed \
                     normal and uv buffers are never deleted by a promote: \
                     promoting `N` or `uv` copies out of them.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Converts an attribute lane between the point and primitive \
              domains: per-corner values combine into one value per triangle, \
              segment, or point primitive (Average / Min / Max / First), and \
              primitive values spread back onto their points, averaging where \
              a point belongs to several primitives.\n\n\
              By default the lane MOVES to the destination domain; Keep \
              Original leaves the source in place too. A point untouched by \
              any primitive receives zeros on a primitive-to-point promote. \
              The Attributes pane's Point and Primitive tabs show both \
              domains, which is the quickest way to watch this node work.",
        search_aliases: &["promote", "domain", "primitive", "point", "demote"],
        glyph: "attribute_promote",
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
    let input = &super::common::baked_input(input, cx)?;
    let name = p.text("attr_name").trim().to_string();
    if name.is_empty() {
        cx.warn("attribute_promote has no attribute name; the input passes through unchanged");
        return Ok(CookOutcome::Done(Outputs::geometry((**input).clone())));
    }
    let to_primitive = p.enum_key("direction") == "pointToPrimitive";
    let method = method_of(p.enum_key("method"));
    let keep = p.bool("keep_original");

    let mut missing = false;
    let mut mismatched = false;
    let meshes: Vec<KernelMesh> = input
        .meshes
        .iter()
        .map(|mesh| {
            let mut out = mesh.clone();
            if to_primitive {
                let Some(lane) = resolve_lane(mesh, &name) else {
                    missing = true;
                    return out;
                };
                if lane.len() != mesh.positions.len() {
                    mismatched = true;
                    return out;
                }
                let data = lane_data(lane);
                let promoted = promote_point_to_primitive(mesh, &data, method);
                out.primitive_attributes.insert(name.clone(), promoted);
                if !keep {
                    // Map lanes move; the fixed N/uv buffers stay (see the
                    // Keep Original doc).
                    out.attributes.remove(&name);
                }
            } else {
                let Some(data) = mesh.primitive_attributes.get(&name).cloned() else {
                    missing = true;
                    return out;
                };
                if data.len() != mesh.primitive_count() {
                    mismatched = true;
                    return out;
                }
                let promoted = promote_primitive_to_point(mesh, &data, method);
                out.attributes.insert(name.clone(), promoted);
                if !keep {
                    out.primitive_attributes.remove(&name);
                }
            }
            out
        })
        .collect();

    let domain = if to_primitive { "point" } else { "primitive" };
    if missing {
        cx.warn(format!(
            "no `{name}` lane in the {domain} domain on at least one mesh; \
             those meshes pass through unchanged"
        ));
    }
    if mismatched {
        cx.warn(format!(
            "`{name}` has the wrong element count on at least one mesh; \
             those meshes pass through unchanged"
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
    use solarxy_kernel::attribute_ops::{AttributeValue, attribute_create};
    use solarxy_kernel::primitives::generate_plane;
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
    fn moves_a_point_lane_to_the_primitive_domain() {
        let set = attribute_create(
            &GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)),
            "mass",
            AttributeValue::Float(2.5),
        );
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            lit(ParamValue::Text("mass".into())),
        );
        let (out, warnings) = run(stored, set);
        assert!(warnings.is_empty(), "{warnings:?}");
        let mesh = &out.meshes[0];
        assert!(!mesh.attributes.contains_key("mass"), "the lane moved");
        let Some(AttributeData::Float(lane)) = mesh.primitive_attributes.get("mass") else {
            panic!("primitive lane written");
        };
        assert_eq!(lane.len(), mesh.primitive_count());
        assert!(lane.iter().all(|&v| (v - 2.5).abs() < 1e-6));
    }

    #[test]
    fn round_trips_back_to_points_and_keep_original_keeps_both() {
        let set = attribute_create(
            &GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)),
            "mass",
            AttributeValue::Float(2.5),
        );
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            lit(ParamValue::Text("mass".into())),
        );
        let (up, _) = run(stored.clone(), set);
        stored.insert(
            "direction".to_string(),
            lit(ParamValue::Enum("primitiveToPoint".into())),
        );
        stored.insert("keep_original".to_string(), lit(ParamValue::Bool(true)));
        let (down, warnings) = run(stored, (*up).clone());
        assert!(warnings.is_empty(), "{warnings:?}");
        let mesh = &down.meshes[0];
        assert!(
            mesh.primitive_attributes.contains_key("mass"),
            "keep_original held the source"
        );
        let Some(AttributeData::Float(lane)) = mesh.attributes.get("mass") else {
            panic!("point lane written");
        };
        assert_eq!(lane.len(), mesh.positions.len());
        assert!(lane.iter().all(|&v| (v - 2.5).abs() < 1e-6));
    }

    #[test]
    fn promoting_the_fixed_normals_copies_without_clearing_them() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        assert!(set.meshes[0].normals.is_some(), "plane ships normals");
        let mut stored = BTreeMap::new();
        stored.insert("attr_name".to_string(), lit(ParamValue::Text("N".into())));
        let (out, warnings) = run(stored, set);
        assert!(warnings.is_empty(), "{warnings:?}");
        let mesh = &out.meshes[0];
        assert!(mesh.normals.is_some(), "the fixed buffer survives");
        let Some(AttributeData::Vec3(lane)) = mesh.primitive_attributes.get("N") else {
            panic!("promoted normals written");
        };
        assert_eq!(lane.len(), mesh.primitive_count());
    }

    #[test]
    fn a_missing_lane_warns_and_passes_through() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            lit(ParamValue::Text("nope".into())),
        );
        let (out, warnings) = run(stored, set);
        assert!(out.meshes[0].primitive_attributes.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn the_promoted_lane_reads_back_through_the_attribute_page() {
        // The first primitive-domain producer must surface in the
        // spreadsheet path end to end.
        use crate::engine::attr_table::{attribute_page, attribute_summary};
        let set = attribute_create(
            &GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)),
            "mass",
            AttributeValue::Float(2.5),
        );
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            lit(ParamValue::Text("mass".into())),
        );
        let (out, _) = run(stored, set);
        let summary = attribute_summary(&out);
        assert!(
            summary.primitive.iter().any(|l| l.name == "mass"),
            "the summary lists the primitive lane: {summary:?}"
        );
        let page = attribute_page(&out, solarxy_kernel::AttributeDomain::Primitive, 0, 8);
        assert_eq!(page.total, u64::from(page.offset) + page.rows.len() as u64);
        assert!(page.columns.iter().any(|c| c.key == "mass"));
        assert!(
            page.rows
                .iter()
                .all(|r| r.iter().all(std::option::Option::is_some)),
            "no nulls: the lane exists on every primitive"
        );
    }
}
