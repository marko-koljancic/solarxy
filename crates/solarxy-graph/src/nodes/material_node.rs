//! The hybrid `material` node: assigns a PBR material to the input
//! geometry, either built INLINE from its own factors and map ports
//! or REFERENCED from a material network by path
//!. v2 also discharges the per-slot-targeting backlog
//! note: an empty `target` overrides every mesh (the v1 behavior); a
//! non-empty target assigns only meshes whose name contains it, leaving
//! the rest on their existing materials.
//!
//! Factor params (base color, metallic, roughness, emissive) drive their
//! channels alone until the matching Image map port is connected; a
//! connected map writes its texture role AND neutralizes the corresponding
//! factor (white base color and emissive, 1.0 metallic and roughness), so
//! the map fully drives the channel through the renderer's factor-times-map
//! math. `metallic_roughness_map` follows glTF packing (G roughness, B
//! metallic) and neutralizes both scalars; `normal_map` and
//! `occlusion_map` have no factor to neutralize. Alpha is Opaque in v1
//! (alpha-mode control is a backlog note).

use std::sync::Arc;

use solarxy_core::RawMaterialData;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, NodePathAccept, ParamSpec, ParamType, Pred};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

/// The five optional map ports, one per `RawMaterialData` texture role:
/// `(key, label, doc)`. Shared with the mat-context `principled` node, so
/// the two cannot describe the same port differently.
pub(super) const MAP_PORTS: [(&str, &str, &str); 5] = [
    (
        "base_color_map",
        "Base Color Map",
        "The albedo texture, read as sRGB and multiplied by the Base Color \
         factor. Connecting it neutralizes that factor to white, so the map \
         alone drives the colour. Left empty, the factor is the colour.",
    ),
    (
        "normal_map",
        "Normal Map",
        "A tangent-space normal map, read as linear data. It has no factor \
         to neutralize, so nothing dims when you connect it. Left empty, \
         the surface samples a flat normal and shades from the mesh normals \
         alone.",
    ),
    (
        "metallic_roughness_map",
        "Metallic Roughness Map",
        "glTF-packed: roughness in G, metallic in B. Connecting it \
         neutralizes BOTH the Metallic and Roughness factors to 1.0, so one \
         port takes over two channels at once -- there is no way to map one \
         of them and keep the scalar on the other.",
    ),
    (
        "occlusion_map",
        "Occlusion Map",
        "Baked ambient occlusion, read from R and composited into the packed \
         ORM texture. It only reaches the renderer when a Metallic Roughness \
         Map is connected too AND the two images have identical dimensions; \
         connected alone, or at a mismatched size, it is silently dropped.",
    ),
    (
        "emissive_map",
        "Emissive Map",
        "Light the surface emits by itself, read as sRGB and multiplied by \
         the Emissive factor. Connecting it neutralizes that factor to \
         white. Left empty, the factor alone decides the emission.",
    ),
];

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    let mut inputs = vec![
        PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
            .default_port()
            .doc(
                "The geometry to dress. Required: this node only rewrites \
                 the material table and each mesh's material index, so it \
                 has nothing to assign to without an input. Points, normals \
                 and UVs pass through untouched.",
            ),
    ];
    for (key, label, doc) in MAP_PORTS {
        inputs.push(PortSpec::single(key, label, DataType::Image, false).doc(doc));
    }

    NodeTypeDescriptor {
        type_id: "material",
        // v2: Reference mode + per-slot targeting. Added
        // params fill from defaults on load (registry-default migration),
        // so v1 documents keep their exact inline behavior.
        version: 2,
        display_name: "Material",
        category: Category::Shaders,
        contexts: ContextSet::GEO,
        opens: None,
        inputs,
        outputs: vec![geometry_output()],
        params: params_with(
            "Material",
            vec![
                ParamSpec::new(
                    "mode",
                    "Mode",
                    "material",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("inline", "Inline"),
                            EnumVariant::new("reference", "Reference"),
                        ],
                    },
                    ParamValue::Enum("inline".to_string()),
                )
                .doc(
                    "Inline builds the surface from this node's own factors \
                     and map ports. Reference ignores both and assigns the \
                     material a `matnet` publishes instead. Switching to \
                     Reference hides the factors but keeps their values, so \
                     switching back restores exactly what you had.",
                ),
                ParamSpec::new(
                    "material_path",
                    "Material Network",
                    "material",
                    ParamType::NodePath {
                        accept: NodePathAccept::Opens(crate::document::ContextKind::Mat),
                    },
                    ParamValue::NodeRef(None),
                )
                .show_if("mode", Pred::Eq(ParamValue::Enum("reference".to_string())))
                .doc(
                    "The `matnet` to take the material from. What arrives is \
                     whatever that network's display node publishes, so \
                     re-designating the display node inside it re-points \
                     every referrer at once. In Reference mode this is \
                     required: unset, dangling, or aimed at a network that \
                     publishes nothing all fail the cook rather than \
                     quietly assigning a default surface.",
                ),
                ParamSpec::new(
                    "target",
                    "Target Meshes",
                    "material",
                    ParamType::Text,
                    ParamValue::Text(String::new()),
                )
                .doc(
                    "A case-sensitive substring matched against mesh names. \
                     Empty is the override-all default: the material table \
                     collapses to this one material and every mesh takes it. \
                     Non-empty appends the material and re-points only the \
                     matching meshes, leaving the rest on whatever they \
                     already had, so several `material` nodes in a row can \
                     dress different parts of one merged object. Primitives \
                     are named after their type (`box`, `sphere`); imported \
                     meshes keep the names from the file.",
                ),
            ],
        )
        .into_iter()
        .chain(factor_params().into_iter().map(|spec| {
            // The factors drive only the INLINE mode; Reference mode
            // hides them (the referenced network owns the surface).
            if spec.key == "material_name" {
                spec
            } else {
                spec.show_if("mode", Pred::Eq(ParamValue::Enum("inline".to_string())))
            }
        }))
        .collect(),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Assigns one material to the meshes of the input geometry. The \
              material is either built INLINE from this node's own factors \
              and map ports, or taken by REFERENCE from a `matnet` \
              elsewhere in the scene.\n\n\
              Drop it at the tail of a geo network, after the modelling and \
              the UV work: it only rewrites the material table and each \
              mesh's material index, so points, normals and UVs pass \
              through untouched. Reach for Inline for a one-off surface \
              nothing else needs. Reach for Reference once a material is \
              shared: point `material_path` at a `matnet` and one edit \
              inside that network updates every object referring to it.\n\n\
              `target` decides how much this node claims. Empty, it \
              overrides everything -- the material table collapses to this \
              one material and every mesh points at it. Non-empty, it \
              appends instead and re-points only the meshes whose name \
              contains that substring. Note that Reference mode hides the \
              factor params but NOT the five map ports: they stay on the \
              node and are ignored, because the referenced network owns the \
              whole surface.",
        search_aliases: &["material", "pbr", "texture", "shader", "color"],
        glyph: "material",
        role: NodeRole::Standard,
        cook: cook_material,
        migrate: None,
    }
}

/// The inline hybrid surface's factor params (plus the name), shared
/// with the mat-context `principled` node, which uses them WITHOUT the
/// material node's mode gating.
pub(super) fn factor_params() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "base_color",
            "Base Color",
            "material",
            ParamType::Color,
            ParamValue::Color([0.8, 0.8, 0.8, 1.0]),
        )
        .driven_by_port("base_color_map")
        .doc(
            "The surface colour of a dielectric, or the reflectance tint of \
             a metal, multiplied into the base-color sample. Connecting a \
             Base Color Map neutralizes this to white so the map alone \
             drives the channel; the value you set is kept for when the map \
             comes off again. Alpha is carried, but these nodes only build \
             Opaque materials today.",
        ),
        ParamSpec::new(
            "metallic",
            "Metallic",
            "material",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .driven_by_port("metallic_roughness_map")
        .doc(
            "How metallic the surface is. 0 is a dielectric: coloured \
             diffuse plus an uncoloured specular highlight. 1 is bare \
             metal: no diffuse at all, and the reflection takes the base \
             colour. Values in between are not physical -- reach for them \
             for a worn or corroded edge, not as a dial for shininess \
             (that is Roughness).",
        ),
        ParamSpec::new(
            "roughness",
            "Roughness",
            "material",
            ParamType::Float,
            ParamValue::Float(0.5),
        )
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .driven_by_port("metallic_roughness_map")
        .doc(
            "Microsurface scatter, which sets how wide the specular lobe \
             is: 0 is a mirror, 1 is fully diffuse. The shader clamps the \
             low end to 0.04, so a perfect mirror is not reachable and \
             highlights never collapse into a single aliased pixel.",
        ),
        ParamSpec::new(
            "emissive",
            "Emissive",
            "material",
            ParamType::Color,
            ParamValue::Color([0.0, 0.0, 0.0, 1.0]),
        )
        .driven_by_port("emissive_map")
        .doc(
            "Light the surface emits on its own, added on top of the lit \
             result, so an emissive surface stays visible in shadow. Black \
             (the default) is no emission. It lights nothing else: there is \
             no emissive bounce, so a glowing panel does not brighten the \
             wall behind it.",
        ),
        ParamSpec::new(
            "material_name",
            "Material Name",
            "material",
            ParamType::Text,
            ParamValue::Text(String::new()),
        )
        .doc(
            "What the material is called wherever it is listed. It has no \
             effect on the shading. Empty falls back to `material`. The \
             geo-side `material` node keeps this visible in Reference mode \
             but ignores it there: a referenced network's material carries \
             its own name.",
        ),
    ]
}

fn cook_material(
    p: &ResolvedParams,
    inputs: &Inputs,
    cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    // The required-input guard already ran in the driver; a connected but
    // empty upstream flows here as None and yields empty (keep-last-good).
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    let material = if p.enum_key("mode") == "reference" {
        // Reference mode: the material network's published
        // value, pre-resolved by the driver. A set-but-unresolvable path
        // is a hard error badge, never a silent fallback material.
        let Some(target) = p.node_ref("material_path") else {
            return Err(CookError::Failed {
                message: "reference mode needs a material network".to_string(),
            });
        };
        match cx.referenced(target).and_then(|v| v.as_material()) {
            Some(m) => std::sync::Arc::clone(m),
            None => {
                return Err(CookError::Failed {
                    message: format!("material reference to node {} does not resolve", target.0),
                });
            }
        }
    } else {
        std::sync::Arc::new(build_inline_material(p, inputs))
    };

    Ok(CookOutcome::Done(Outputs::geometry(assign_material(
        input,
        &material,
        p.text("target"),
    ))))
}

/// Builds the inline hybrid material from the node's own factors and map
/// ports (the ratified decision-4 semantics, verbatim from v1). Shared
/// with the mat-context `principled` node.
pub(super) fn build_inline_material(p: &ResolvedParams, inputs: &Inputs) -> RawMaterialData {
    let base_color_map = inputs.image("base_color_map");
    let normal_map = inputs.image("normal_map");
    let mr_map = inputs.image("metallic_roughness_map");
    let occlusion_map = inputs.image("occlusion_map");
    let emissive_map = inputs.image("emissive_map");

    let name = match p.text("material_name") {
        "" => "material".to_string(),
        n => n.to_string(),
    };

    // Neutralization: a connected map's factor becomes the multiplicative
    // identity so the map alone drives the channel.
    let base_color = if base_color_map.is_some() {
        [1.0, 1.0, 1.0, 1.0]
    } else {
        p.color("base_color")
    };
    let (metallic, roughness) = if mr_map.is_some() {
        (1.0, 1.0)
    } else {
        (p.f32("metallic"), p.f32("roughness"))
    };
    let emissive = if emissive_map.is_some() {
        [1.0, 1.0, 1.0]
    } else {
        let e = p.color("emissive");
        [e[0], e[1], e[2]]
    };

    RawMaterialData {
        name,
        diffuse_texture_data: base_color_map.cloned(),
        normal_texture_data: normal_map.cloned(),
        metallic_roughness_texture_data: mr_map.cloned(),
        occlusion_texture_data: occlusion_map.cloned(),
        emissive_texture_data: emissive_map.cloned(),
        roughness_factor: roughness,
        metallic_factor: metallic,
        emissive_factor: emissive,
        base_color_factor: base_color,
        alpha_cutoff: 0.5,
        ..RawMaterialData::default()
    }
}

/// Assigns the material to the input's meshes. An empty target is the v1
/// override-all (one-entry material table); a non-empty target appends
/// the material and points only name-matching meshes at it, leaving the
/// rest on their existing materials (per-slot targeting).
fn assign_material(
    input: &Arc<solarxy_kernel::GeometrySet>,
    material: &Arc<RawMaterialData>,
    target: &str,
) -> solarxy_kernel::GeometrySet {
    // Mesh attribute buffers stay Arc-shared; bounds are untouched.
    let mut set = (**input).clone();
    if target.is_empty() {
        set.materials = vec![Arc::clone(material)];
        for mesh in &mut set.meshes {
            mesh.material_index = Some(0);
        }
    } else {
        set.materials.push(Arc::clone(material));
        let idx = set.materials.len() - 1;
        for mesh in &mut set.meshes {
            if mesh.name.contains(target) {
                mesh.material_index = Some(idx);
            }
        }
    }
    set
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::cook::InputSlot;
    use crate::registry::coerce::Value;
    use solarxy_core::RawImageData;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::{generate_box, generate_plane};
    use std::collections::BTreeMap;

    fn resolved(overrides: &[(&str, ParamValue)]) -> ResolvedParams {
        let mut stored = BTreeMap::new();
        for (k, v) in overrides {
            stored.insert(
                (*k).to_string(),
                crate::params::ParamSource::Literal(v.clone()),
            );
        }
        crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap()
    }

    fn geo_input() -> (String, InputSlot) {
        let set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        (
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(set))),
        )
    }

    fn image(px: [u8; 4]) -> Arc<RawImageData> {
        Arc::new(RawImageData::new(px.to_vec(), 1, 1))
    }

    fn cook_with(
        params: &ResolvedParams,
        slots: Vec<(String, InputSlot)>,
    ) -> solarxy_kernel::GeometrySet {
        let assets = crate::assets::AssetTable::default();
        let mut cx = CookCtx::new(&assets, false);
        let inputs = Inputs::new(slots.into_iter().collect());
        let CookOutcome::Done(outputs) = cook_material(params, &inputs, &mut cx).unwrap() else {
            panic!("material cook is synchronous");
        };
        (**outputs.get("geometry").unwrap().as_geometry().unwrap()).clone()
    }

    #[test]
    fn params_only_factors_drive_channels() {
        let p = resolved(&[
            ("base_color", ParamValue::Color([0.2, 0.4, 0.6, 1.0])),
            ("metallic", ParamValue::Float(0.9)),
            ("roughness", ParamValue::Float(0.3)),
            ("emissive", ParamValue::Color([0.1, 0.2, 0.3, 1.0])),
            ("material_name", ParamValue::Text("painted".into())),
        ]);
        let set = cook_with(&p, vec![geo_input()]);
        assert_eq!(set.materials.len(), 1);
        let m = &set.materials[0];
        assert_eq!(m.name, "painted");
        assert_eq!(m.base_color_factor, [0.2, 0.4, 0.6, 1.0]);
        assert!((m.metallic_factor - 0.9).abs() < 1e-6);
        assert!((m.roughness_factor - 0.3).abs() < 1e-6);
        assert_eq!(m.emissive_factor, [0.1, 0.2, 0.3]);
        assert!(m.diffuse_texture_data.is_none());
        assert!(set.meshes.iter().all(|me| me.material_index == Some(0)));
    }

    #[test]
    fn connected_maps_neutralize_their_factors() {
        let p = resolved(&[
            ("base_color", ParamValue::Color([0.2, 0.4, 0.6, 1.0])),
            ("metallic", ParamValue::Float(0.9)),
            ("roughness", ParamValue::Float(0.3)),
            ("emissive", ParamValue::Color([0.1, 0.2, 0.3, 1.0])),
        ]);
        let base = image([255, 0, 0, 255]);
        let mr = image([0, 128, 64, 255]);
        let em = image([9, 9, 9, 255]);
        let set = cook_with(
            &p,
            vec![
                geo_input(),
                (
                    "base_color_map".into(),
                    InputSlot::Single(Value::Image(Arc::clone(&base))),
                ),
                (
                    "metallic_roughness_map".into(),
                    InputSlot::Single(Value::Image(Arc::clone(&mr))),
                ),
                (
                    "emissive_map".into(),
                    InputSlot::Single(Value::Image(Arc::clone(&em))),
                ),
            ],
        );
        let m = &set.materials[0];
        // Maps landed in their roles (same Arc, no pixel copy)...
        assert!(Arc::ptr_eq(m.diffuse_texture_data.as_ref().unwrap(), &base));
        assert!(Arc::ptr_eq(
            m.metallic_roughness_texture_data.as_ref().unwrap(),
            &mr
        ));
        assert!(Arc::ptr_eq(m.emissive_texture_data.as_ref().unwrap(), &em));
        // ...and the corresponding factors are neutral identities.
        assert_eq!(m.base_color_factor, [1.0, 1.0, 1.0, 1.0]);
        assert!((m.metallic_factor - 1.0).abs() < 1e-6);
        assert!((m.roughness_factor - 1.0).abs() < 1e-6);
        assert_eq!(m.emissive_factor, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn mixed_configuration_neutralizes_only_mapped_channels() {
        let p = resolved(&[
            ("base_color", ParamValue::Color([0.2, 0.4, 0.6, 1.0])),
            ("metallic", ParamValue::Float(0.9)),
            ("roughness", ParamValue::Float(0.3)),
        ]);
        let normal = image([128, 128, 255, 255]);
        let set = cook_with(
            &p,
            vec![
                geo_input(),
                (
                    "normal_map".into(),
                    InputSlot::Single(Value::Image(Arc::clone(&normal))),
                ),
            ],
        );
        let m = &set.materials[0];
        // Normal map has no factor: everything else keeps its param value.
        assert!(m.normal_texture_data.is_some());
        assert_eq!(m.base_color_factor, [0.2, 0.4, 0.6, 1.0]);
        assert!((m.metallic_factor - 0.9).abs() < 1e-6);
        assert!((m.roughness_factor - 0.3).abs() < 1e-6);
    }

    #[test]
    fn override_all_replaces_a_multi_material_table() {
        let mut mesh_a = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        mesh_a.material_index = Some(0);
        let mut mesh_b = generate_plane(1.0, 1.0, 1, 1);
        mesh_b.material_index = Some(1);
        let set_in = GeometrySet::from_parts(
            vec![mesh_a, mesh_b],
            vec![
                Arc::new(RawMaterialData {
                    name: "old_a".into(),
                    ..Default::default()
                }),
                Arc::new(RawMaterialData {
                    name: "old_b".into(),
                    ..Default::default()
                }),
            ],
        );
        let p = resolved(&[("material_name", ParamValue::Text("override".into()))]);
        let set = cook_with(
            &p,
            vec![(
                "geometry".to_string(),
                InputSlot::Single(Value::Geometry(Arc::new(set_in))),
            )],
        );
        assert_eq!(set.materials.len(), 1, "override-all: one material");
        assert_eq!(set.materials[0].name, "override");
        assert!(set.meshes.iter().all(|m| m.material_index == Some(0)));
    }

    #[test]
    fn geometry_passes_through_untouched() {
        let input = GeometrySet::from_mesh(generate_box(2.0, 1.0, 1.0, 1, 1, 1));
        let bounds_in = input.bounds;
        let positions_in = Arc::clone(&input.meshes[0].positions);
        let p = resolved(&[]);
        let set = cook_with(
            &p,
            vec![(
                "geometry".to_string(),
                InputSlot::Single(Value::Geometry(Arc::new(input))),
            )],
        );
        assert!(Arc::ptr_eq(&set.meshes[0].positions, &positions_in));
        assert_eq!(set.bounds.min, bounds_in.min);
        assert_eq!(set.bounds.max, bounds_in.max);
    }
}
