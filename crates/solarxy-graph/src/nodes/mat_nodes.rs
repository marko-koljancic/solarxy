//! The material context (context-expansion phase 20): the `matnet` root
//! container plus the mat-network node set. Inside a material network,
//! `DataType::Material` wires nodes together and the display node
//! publishes the network's material; across contexts materials travel by
//! path reference only (decision C-2), consumed by the geo-side
//! `material` node's Reference mode.
//!
//! Surface nodes: `principled` (the full metallic-roughness surface,
//! sharing the inline hybrid builder with the geo-side `material` node),
//! `matcap` (its image IS the base-color texture, sampled by view normal
//! in the shader), `toon` (banded diffuse), and `unlit` (flat color, glTF
//! `KHR_materials_unlit`). `mix_material` lerps the FACTOR channels and
//! takes textures and the shading model from the dominant side (a
//! documented v1 simplification: true map blending needs shader work).
//! `tex_ref` turns a texture-network path into an Image wire (the
//! Object-Merge pattern), placeable in Mat and Geo networks.

use std::sync::Arc;

use solarxy_core::RawMaterialData;
use solarxy_core::geometry::ShadingModel;

use super::common::{params_with, passive_cook};
use super::material_node::build_inline_material;
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::document::ContextKind;
use crate::params::ParamValue;
use crate::registry::coerce::{DataType, Value};
use crate::registry::param_spec::{NodePathAccept, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

fn material_out() -> PortSpec {
    PortSpec::single("material", "Material", DataType::Material, false).default_port()
}

fn material_outputs(m: RawMaterialData) -> Outputs {
    Outputs::single("material", Value::Material(Arc::new(m)))
}

fn name_param() -> ParamSpec {
    ParamSpec::new(
        "material_name",
        "Material Name",
        "material",
        ParamType::Text,
        ParamValue::Text(String::new()),
    )
}

fn named(p: &ResolvedParams, fallback: &str) -> String {
    match p.text("material_name") {
        "" => fallback.to_string(),
        n => n.to_string(),
    }
}

// ---- matnet ----

#[must_use]
pub fn matnet_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "matnet",
        version: 1,
        display_name: "Material Network",
        category: Category::Container,
        contexts: ContextSet::OBJ,
        opens: Some(ContextKind::Mat),
        inputs: vec![],
        outputs: vec![],
        params: params_with("Material Network", vec![]),
        bypass: BypassBehavior::NotBypassable,
        doc: "A material network: surface nodes cook inside it, and its display node publishes the network's material for `material` nodes in Reference mode.",
        search_aliases: &["matnet", "material", "shop", "shader network"],
        glyph: "matnet",
        role: NodeRole::Container,
        cook: passive_cook,
        migrate: None,
    }
}

// ---- principled ----

#[must_use]
pub fn principled_descriptor() -> NodeTypeDescriptor {
    // The same map ports and factor params as the geo-side material node's
    // inline half; only the output differs (a Material wire, not an
    // assignment).
    let mut inputs = Vec::new();
    for (key, label) in super::material_node::MAP_PORTS {
        inputs.push(PortSpec::single(key, label, DataType::Image, false));
    }
    NodeTypeDescriptor {
        type_id: "principled",
        version: 1,
        display_name: "Principled",
        category: Category::Modifiers,
        contexts: ContextSet::MAT,
        opens: None,
        inputs,
        outputs: vec![material_out()],
        params: params_with("Principled", super::material_node::factor_params()),
        bypass: BypassBehavior::Mute,
        doc: "The full metallic-roughness surface: factors plus five texture-role map ports (a connected map drives its channel and neutralizes the factor).",
        search_aliases: &["principled", "pbr", "surface", "standard"],
        glyph: "principled",
        role: NodeRole::Standard,
        cook: cook_principled,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_principled(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    Ok(CookOutcome::Done(material_outputs(build_inline_material(
        p, inputs,
    ))))
}

// ---- matcap ----

#[must_use]
pub fn matcap_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "matcap",
        version: 1,
        display_name: "MatCap",
        category: Category::Modifiers,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![
            PortSpec::single("matcap", "Matcap Image", DataType::Image, false).default_port(),
        ],
        outputs: vec![material_out()],
        params: params_with(
            "MatCap",
            vec![
                ParamSpec::new(
                    "tint",
                    "Tint",
                    "material",
                    ParamType::Color,
                    ParamValue::Color([1.0, 1.0, 1.0, 1.0]),
                ),
                name_param(),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A matcap surface: the image is sampled by the view-space normal, unlit. The image rides the base-color texture role; the tint multiplies it.",
        search_aliases: &["matcap", "material capture", "sculpt", "zbrush"],
        glyph: "matcap",
        role: NodeRole::Standard,
        cook: cook_matcap,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_matcap(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let m = RawMaterialData {
        name: named(p, "matcap"),
        diffuse_texture_data: inputs.image("matcap").cloned(),
        base_color_factor: p.color("tint"),
        shading_model: ShadingModel::Matcap,
        ..RawMaterialData::default()
    };
    Ok(CookOutcome::Done(material_outputs(m)))
}

// ---- toon ----

#[must_use]
pub fn toon_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "toon",
        version: 1,
        display_name: "Toon",
        category: Category::Modifiers,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![
            PortSpec::single("base_color_map", "Base Color Map", DataType::Image, false)
                .default_port(),
        ],
        outputs: vec![material_out()],
        params: params_with(
            "Toon",
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
                    "steps",
                    "Bands",
                    "material",
                    ParamType::Float,
                    ParamValue::Float(3.0),
                )
                .hard(2.0, 8.0)
                .step(1.0),
                name_param(),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "Cel shading: the diffuse term quantizes into bands.",
        search_aliases: &["toon", "cel", "cartoon", "banded"],
        glyph: "toon",
        role: NodeRole::Standard,
        cook: cook_toon,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_toon(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let map = inputs.image("base_color_map");
    let m = RawMaterialData {
        name: named(p, "toon"),
        diffuse_texture_data: map.cloned(),
        base_color_factor: if map.is_some() {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            p.color("base_color")
        },
        shading_model: ShadingModel::Toon,
        toon_steps: p.f32("steps"),
        ..RawMaterialData::default()
    };
    Ok(CookOutcome::Done(material_outputs(m)))
}

// ---- unlit ----

#[must_use]
pub fn unlit_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "unlit",
        version: 1,
        display_name: "Unlit",
        category: Category::Modifiers,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![
            PortSpec::single("base_color_map", "Base Color Map", DataType::Image, false)
                .default_port(),
        ],
        outputs: vec![material_out()],
        params: params_with(
            "Unlit",
            vec![
                ParamSpec::new(
                    "base_color",
                    "Base Color",
                    "material",
                    ParamType::Color,
                    ParamValue::Color([0.8, 0.8, 0.8, 1.0]),
                )
                .driven_by_port("base_color_map"),
                name_param(),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "Flat base color, no lighting (exports as glTF KHR_materials_unlit).",
        search_aliases: &["unlit", "flat", "constant", "emission"],
        glyph: "unlit",
        role: NodeRole::Standard,
        cook: cook_unlit,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_unlit(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let map = inputs.image("base_color_map");
    let m = RawMaterialData {
        name: named(p, "unlit"),
        diffuse_texture_data: map.cloned(),
        base_color_factor: if map.is_some() {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            p.color("base_color")
        },
        shading_model: ShadingModel::Unlit,
        ..RawMaterialData::default()
    };
    Ok(CookOutcome::Done(material_outputs(m)))
}

// ---- mix_material ----

#[must_use]
pub fn mix_material_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "mix_material",
        version: 1,
        display_name: "Mix Material",
        category: Category::Modifiers,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![
            PortSpec::single("a", "A", DataType::Material, true).default_port(),
            PortSpec::single("b", "B", DataType::Material, true),
        ],
        outputs: vec![material_out()],
        params: params_with(
            "Mix Material",
            vec![
                ParamSpec::new(
                    "factor",
                    "Factor",
                    "material",
                    ParamType::Float,
                    ParamValue::Float(0.5),
                )
                .hard(0.0, 1.0)
                .step(0.01),
                name_param(),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "a".to_string(),
        },
        doc: "Lerps the factor channels of two materials; textures and the shading model come from the dominant side (over 0.5 = B).",
        search_aliases: &["mix", "blend", "layer", "material"],
        glyph: "mix_material",
        role: NodeRole::Gather,
        cook: cook_mix_material,
        migrate: None,
    }
}

fn cook_mix_material(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let need = |key: &str| -> Result<Arc<RawMaterialData>, CookError> {
        inputs
            .material(key)
            .map(Arc::clone)
            .ok_or_else(|| CookError::Failed {
                message: format!("no material on input '{key}' yet"),
            })
    };
    let a = need("a")?;
    let b = need("b")?;
    let t = p.f32("factor");
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    // Factor channels lerp; textures and the shading model follow the
    // dominant side (a documented v1 simplification).
    let dominant = if t >= 0.5 { &b } else { &a };
    let m = RawMaterialData {
        name: named(p, "mix"),
        base_color_factor: [
            lerp(a.base_color_factor[0], b.base_color_factor[0]),
            lerp(a.base_color_factor[1], b.base_color_factor[1]),
            lerp(a.base_color_factor[2], b.base_color_factor[2]),
            lerp(a.base_color_factor[3], b.base_color_factor[3]),
        ],
        metallic_factor: lerp(a.metallic_factor, b.metallic_factor),
        roughness_factor: lerp(a.roughness_factor, b.roughness_factor),
        emissive_factor: [
            lerp(a.emissive_factor[0], b.emissive_factor[0]),
            lerp(a.emissive_factor[1], b.emissive_factor[1]),
            lerp(a.emissive_factor[2], b.emissive_factor[2]),
        ],
        toon_steps: lerp(a.toon_steps, b.toon_steps),
        diffuse_texture_data: dominant.diffuse_texture_data.clone(),
        normal_texture_data: dominant.normal_texture_data.clone(),
        metallic_roughness_texture_data: dominant.metallic_roughness_texture_data.clone(),
        occlusion_texture_data: dominant.occlusion_texture_data.clone(),
        emissive_texture_data: dominant.emissive_texture_data.clone(),
        shading_model: dominant.shading_model,
        alpha_mode: dominant.alpha_mode,
        alpha_cutoff: dominant.alpha_cutoff,
        ..RawMaterialData::default()
    };
    Ok(CookOutcome::Done(material_outputs(m)))
}

// ---- tex_ref ----

#[must_use]
pub fn tex_ref_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "tex_ref",
        version: 1,
        display_name: "Texture Reference",
        category: Category::Import,
        contexts: ContextSet::MAT.or(ContextSet::GEO),
        opens: None,
        inputs: vec![],
        outputs: vec![PortSpec::single("image", "Image", DataType::Image, false).default_port()],
        params: params_with(
            "Texture Reference",
            vec![ParamSpec::new(
                "texture_path",
                "Texture Network",
                "object",
                ParamType::NodePath {
                    accept: NodePathAccept::Opens(ContextKind::Tex),
                },
                ParamValue::NodeRef(None),
            )],
        ),
        bypass: BypassBehavior::Mute,
        doc: "Brings a texture network's published image in as an Image wire (the fetch pattern); editing the network recooks every referrer.",
        search_aliases: &["tex_ref", "fetch", "object merge", "texture", "reference"],
        glyph: "tex_ref",
        role: NodeRole::ImageSource,
        cook: cook_tex_ref,
        migrate: None,
    }
}

fn cook_tex_ref(
    p: &ResolvedParams,
    _in: &Inputs,
    cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    // Unset reference: no output at all (a downstream map port gathers an
    // Absent slot, the exact "no map" semantics import_image uses).
    let Some(target) = p.node_ref("texture_path") else {
        return Ok(CookOutcome::Done(Outputs::default()));
    };
    match cx.referenced(target).and_then(|v| v.as_image()) {
        Some(img) => Ok(CookOutcome::Done(Outputs::single(
            "image",
            Value::Image(Arc::clone(img)),
        ))),
        None => Err(CookError::Failed {
            message: format!("texture reference to node {} does not resolve", target.0),
        }),
    }
}
