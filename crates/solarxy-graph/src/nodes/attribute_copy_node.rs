//! The `attribute_copy` modifier: copies (or, with delete-source, renames)
//! a lane under a new name, optionally converting its type. The bridge
//! that turns arbitrary data into the reserved lanes' contracts, e.g. any
//! vec3 into `color` to drive vertex-color display.

use solarxy_kernel::attribute_ops::{LaneType, convert_lane};
use solarxy_kernel::{AttributeData, AttributeDomain, KernelMesh};

use super::common::{
    geometry_output, params_with, warn_input_lane_type_replaced, warn_reserved_lane_mismatch,
};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::engine::attr_table::{LaneRef, resolve_lane};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

/// Materializes a resolved point lane as owned data (map lanes by `Arc`
/// bump, fixed buffers by copy).
fn lane_data(lane: LaneRef<'_>) -> AttributeData {
    match lane {
        LaneRef::Map(data) => data.clone(),
        LaneRef::Normals(v) => AttributeData::Vec3(std::sync::Arc::new(v.to_vec())),
        LaneRef::Uvs(v) => AttributeData::Vec2(std::sync::Arc::new(v.to_vec())),
    }
}

fn lane_ty_key(data: &AttributeData) -> &'static str {
    match data {
        AttributeData::Float(_) => "float",
        AttributeData::Vec2(_) => "vec2",
        AttributeData::Vec3(_) => "vec3",
        AttributeData::Vec4(_) => "vec4",
    }
}

fn target_of(key: &str, source: &AttributeData) -> LaneType {
    match key {
        "float" => LaneType::Float,
        "vec2" => LaneType::Vec2,
        "vec3" => LaneType::Vec3,
        "vec4" => LaneType::Vec4,
        // auto: keep the source type.
        _ => match source {
            AttributeData::Float(_) => LaneType::Float,
            AttributeData::Vec2(_) => LaneType::Vec2,
            AttributeData::Vec3(_) => LaneType::Vec3,
            AttributeData::Vec4(_) => LaneType::Vec4,
        },
    }
}

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "attribute_copy",
        version: 1,
        display_name: "Attribute Copy",
        category: Category::Attribute,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry carrying the lane."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Attribute Copy",
            vec![
                ParamSpec::new(
                    "source",
                    "Source",
                    "attribute",
                    ParamType::AttributeName,
                    ParamValue::Text(String::new()),
                )
                .doc("The lane to copy, resolved in the chosen domain."),
                ParamSpec::new(
                    "dest",
                    "Destination",
                    "attribute",
                    ParamType::Text,
                    ParamValue::Text(String::new()),
                )
                .doc(
                    "The name the copy is written under, replacing any lane \
                     already there. Reserved names activate their contracts: \
                     `color` (vec4) displays as vertex color immediately.",
                ),
                ParamSpec::new(
                    "domain",
                    "Domain",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("point", "Point"),
                            EnumVariant::new("primitive", "Primitive"),
                        ],
                    },
                    ParamValue::Enum("point".into()),
                )
                .doc(
                    "Which domain both names live in. Use \
                     `attribute_promote` to move a lane between domains.",
                ),
                ParamSpec::new(
                    "target_type",
                    "Type",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("auto", "Same as source"),
                            EnumVariant::new("float", "Float"),
                            EnumVariant::new("vec2", "Vec2"),
                            EnumVariant::new("vec3", "Vec3"),
                            EnumVariant::new("vec4", "Vec4"),
                        ],
                    },
                    ParamValue::Enum("auto".into()),
                )
                .doc(
                    "The copy's type. Widening pads (a vec4's w fills with \
                     1.0, the color case); narrowing to Float takes the \
                     magnitude; other narrowing drops trailing components.",
                ),
                ParamSpec::new(
                    "delete_source",
                    "Delete Source",
                    "attribute",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Remove the source lane after copying, making this a \
                     rename. Deleting the reserved `N` or `uv` clears the \
                     mesh's FIXED normal/uv buffer when no map lane shadows \
                     it, which changes shading and texturing downstream.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Copies an attribute lane under a new name, optionally \
              converting its type, in the point or primitive domain. With \
              Delete Source it is a rename.\n\n\
              The headline use is feeding the reserved lanes: copy any vec3 \
              into `color` and the geometry displays vertex-colored (w pads \
              to 1.0, opaque); copy a vec3 into `N` to override normals. The \
              other direction works too: narrow a `color` to a float \
              magnitude lane and drive `displace` with it.",
        search_aliases: &["copy", "rename", "attribute", "convert", "cast", "color"],
        glyph: "attribute_copy",
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
    let source = p.text("source").trim().to_string();
    let dest = p.text("dest").trim().to_string();
    if source.is_empty() || dest.is_empty() {
        cx.warn("attribute_copy needs both a source and a destination name; the input passes through unchanged");
        return Ok(CookOutcome::Done(Outputs::geometry((**input).clone())));
    }
    let point_domain = p.enum_key("domain") == "point";
    let target_key = p.enum_key("target_type").to_string();
    let delete_source = p.bool("delete_source");

    let mut missing = false;
    let mut written_ty: Option<&'static str> = None;
    let meshes: Vec<KernelMesh> = input
        .meshes
        .iter()
        .map(|mesh| {
            let mut out = mesh.clone();
            let data = if point_domain {
                resolve_lane(mesh, &source).map(lane_data)
            } else {
                mesh.primitive_attributes.get(&source).cloned()
            };
            let Some(data) = data else {
                missing = true;
                return out;
            };
            let converted = convert_lane(&data, target_of(&target_key, &data));
            written_ty = Some(lane_ty_key(&converted));
            let domain = if point_domain {
                AttributeDomain::Point
            } else {
                AttributeDomain::Primitive
            };
            out.domain_attributes_mut(domain)
                .insert(dest.clone(), converted);
            if delete_source && source != dest {
                let map = out.domain_attributes_mut(domain);
                if map.remove(&source).is_none() && point_domain {
                    // The source was a fixed-buffer pseudo-lane; deleting
                    // it clears that buffer (documented on the param).
                    if source == solarxy_kernel::reserved::NORMAL {
                        out.normals = None;
                    } else if source == solarxy_kernel::reserved::UV {
                        out.tex_coords = None;
                    }
                }
            }
            out
        })
        .collect();

    if let Some(ty) = written_ty
        && point_domain
    {
        warn_reserved_lane_mismatch(cx, &dest, ty);
        warn_input_lane_type_replaced(cx, input, &dest, ty);
    }
    if missing {
        let domain = if point_domain { "point" } else { "primitive" };
        cx.warn(format!(
            "no `{source}` lane in the {domain} domain on at least one mesh; \
             those meshes pass through unchanged"
        ));
    }
    Ok(CookOutcome::Done(Outputs::geometry(
        solarxy_kernel::GeometrySet::from_parts(meshes, input.materials.clone()),
    )))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values copied between lanes, not computed

    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::attribute_ops::{AttributeValue, attribute_create};
    use solarxy_kernel::primitives::{generate_box, generate_plane};
    use solarxy_kernel::reserved;
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

    fn stored(pairs: &[(&str, ParamValue)]) -> BTreeMap<String, ParamSource> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), lit(v.clone())))
            .collect()
    }

    #[test]
    fn copies_a_vec3_into_color_and_reaches_the_renderer() {
        let set = attribute_create(
            &GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1)),
            "velocity",
            AttributeValue::Vec3([0.5, 0.25, 1.0]),
        );
        let (out, warnings) = run(
            stored(&[
                ("source", ParamValue::Text("velocity".into())),
                ("dest", ParamValue::Text(reserved::COLOR.into())),
                ("target_type", ParamValue::Enum("vec4".into())),
            ]),
            set,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let mesh = &out.meshes[0];
        assert!(mesh.attributes.contains_key("velocity"), "source kept");
        let Some(AttributeData::Vec4(lane)) = mesh.attributes.get(reserved::COLOR) else {
            panic!("vec4 color lane written");
        };
        assert!(lane.iter().all(|&c| c == [0.5, 0.25, 1.0, 1.0]));
        let cooked = out.to_cooked();
        assert!(
            cooked.meshes[0].colors.is_some(),
            "the color lane crossed the renderer contract"
        );
    }

    #[test]
    fn delete_source_renames_and_auto_keeps_the_type() {
        let set = attribute_create(
            &GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)),
            "a",
            AttributeValue::Float(3.0),
        );
        let (out, warnings) = run(
            stored(&[
                ("source", ParamValue::Text("a".into())),
                ("dest", ParamValue::Text("b".into())),
                ("delete_source", ParamValue::Bool(true)),
            ]),
            set,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let mesh = &out.meshes[0];
        assert!(!mesh.attributes.contains_key("a"), "renamed away");
        let Some(AttributeData::Float(lane)) = mesh.attributes.get("b") else {
            panic!("float lane under the new name");
        };
        assert!(lane.iter().all(|&v| (v - 3.0).abs() < 1e-6));
    }

    #[test]
    fn copying_the_fixed_normals_out_and_deleting_clears_the_buffer() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        assert!(set.meshes[0].normals.is_some());
        let (out, _) = run(
            stored(&[
                ("source", ParamValue::Text(reserved::NORMAL.into())),
                ("dest", ParamValue::Text("old_N".into())),
                ("delete_source", ParamValue::Bool(true)),
            ]),
            set,
        );
        let mesh = &out.meshes[0];
        assert!(mesh.normals.is_none(), "the fixed buffer cleared");
        assert!(matches!(
            mesh.attributes.get("old_N"),
            Some(AttributeData::Vec3(_))
        ));
    }

    #[test]
    fn a_wrong_typed_reserved_destination_warns() {
        let set = attribute_create(
            &GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)),
            "a",
            AttributeValue::Float(1.0),
        );
        let (_, warnings) = run(
            stored(&[
                ("source", ParamValue::Text("a".into())),
                ("dest", ParamValue::Text(reserved::COLOR.into())),
            ]),
            set,
        );
        assert!(
            warnings.iter().any(|w| w.contains("vec4")),
            "reserved contract warning: {warnings:?}"
        );
    }

    #[test]
    fn a_missing_source_warns_and_passes_through() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let (out, warnings) = run(
            stored(&[
                ("source", ParamValue::Text("ghost".into())),
                ("dest", ParamValue::Text("b".into())),
            ]),
            set,
        );
        assert!(out.meshes[0].attributes.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
