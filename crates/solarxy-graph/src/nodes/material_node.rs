//! The hybrid `material` node (Phase 14, ratified decision 4; v2 in the
//! context expansion's phase 20): assigns a PBR material to the input
//! geometry, either built INLINE from its own factors and map ports
//! (decision 4, unchanged) or REFERENCED from a material network by path
//! (decision C-2). v2 also discharges the per-slot-targeting backlog
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

/// The five optional map ports, one per `RawMaterialData` texture role.
/// Shared with the mat-context `principled` node (phase 20).
pub(super) const MAP_PORTS: [(&str, &str); 5] = [
    ("base_color_map", "Base Color Map"),
    ("normal_map", "Normal Map"),
    ("metallic_roughness_map", "Metallic Roughness Map"),
    ("occlusion_map", "Occlusion Map"),
    ("emissive_map", "Emissive Map"),
];

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    let mut inputs =
        vec![PortSpec::single("geometry", "Geometry", DataType::Geometry, true).default_port()];
    for (key, label) in MAP_PORTS {
        inputs.push(PortSpec::single(key, label, DataType::Image, false));
    }

    NodeTypeDescriptor {
        type_id: "material",
        // v2 (phase 20): Reference mode + per-slot targeting. Added
        // params fill from defaults on load (registry-default migration),
        // so v1 documents keep their exact inline behavior.
        version: 2,
        display_name: "Material",
        category: Category::Modifiers,
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
                .show_if("mode", Pred::Eq(ParamValue::Enum("reference".to_string()))),
                ParamSpec::new(
                    "target",
                    "Target Meshes",
                    "material",
                    ParamType::Text,
                    ParamValue::Text(String::new()),
                )
                .doc("Substring filter on mesh names; empty assigns every mesh."),
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
        doc: "Assigns a PBR material to every mesh of the input; connected \
              maps drive their channels, factors drive the rest.",
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
        .driven_by_port("base_color_map"),
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
        .driven_by_port("metallic_roughness_map"),
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
        .driven_by_port("metallic_roughness_map"),
        ParamSpec::new(
            "emissive",
            "Emissive",
            "material",
            ParamType::Color,
            ParamValue::Color([0.0, 0.0, 0.0, 1.0]),
        )
        .driven_by_port("emissive_map"),
        ParamSpec::new(
            "material_name",
            "Material Name",
            "material",
            ParamType::Text,
            ParamValue::Text(String::new()),
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
        // Reference mode (phase 20): the material network's published
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
/// rest on their existing materials (per-slot targeting, phase 20).
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
