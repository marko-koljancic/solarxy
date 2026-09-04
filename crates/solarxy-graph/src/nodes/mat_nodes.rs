//! The material context: the `matnet` root
//! container plus the mat-network node set. Inside a material network,
//! `DataType::Material` wires nodes together and the display node
//! publishes the network's material; across contexts materials travel by
//! path reference only, consumed by the geo-side
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
    PortSpec::single("material", "Material", DataType::Material, false)
        .default_port()
        .doc(
            "The material this node builds. Wire it into `mix_material`, or \
             designate this node as the network's display node so the \
             enclosing `matnet` publishes it to `material` nodes in \
             Reference mode. Being the default output, a drag from the \
             node's body wires from here.",
        )
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
    .doc(
        "What the material is called wherever it is listed. It has no effect \
         on the shading. Empty falls back to the node type's own name.",
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
        display_name: "Mat",
        category: Category::Container,
        contexts: ContextSet::OBJ,
        opens: Some(ContextKind::Mat),
        inputs: vec![],
        outputs: vec![],
        params: params_with("Mat", vec![]),
        bypass: BypassBehavior::NotBypassable,
        doc: "A container for a material network. Surface nodes cook inside \
              it, and whichever node you designate as the display node \
              publishes its material as the network's one result.\n\n\
              Add one per material you want to reuse. Dive in, build a \
              surface with `principled`, `matcap`, `toon` or `unlit`, and \
              combine surfaces with `mix_material`. Nothing leaves on a \
              wire: materials cross contexts by path only, so a geo-side \
              `material` node in Reference mode is what pulls the result \
              out, and any number of them can point at the same network.\n\n\
              It cooks nothing itself and cannot be bypassed. A network \
              with no display node designated publishes nothing at all, and \
              every `material` node referring to it fails its cook rather \
              than falling back to a default surface.",
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
    for (key, label, doc) in super::material_node::MAP_PORTS {
        inputs.push(PortSpec::single(key, label, DataType::Image, false).doc(doc));
    }
    NodeTypeDescriptor {
        type_id: "principled",
        // v2: the principled surface properties, which arrive here because
        // this node shares `factor_params` and `MAP_PORTS` with the
        // geo-side `material` node. Sharing them is the point: the two
        // cannot describe the same surface differently. Pure additions
        // filling from registry defaults, so no migration hook.
        version: 2,
        display_name: "Principled",
        category: Category::Shaders,
        contexts: ContextSet::MAT,
        opens: None,
        inputs,
        outputs: vec![material_out()],
        params: params_with("Principled", super::material_node::factor_params()),
        bypass: BypassBehavior::Mute,
        doc: "The physically-based metallic-roughness surface: base colour, \
              metallic, roughness and emissive as factors, each with an \
              optional texture map port that takes its channel over.\n\n\
              This is the surface to reach for first inside a `matnet`, and \
              the one the renderer's Cook-Torrance path with image-based \
              lighting exists for. Feed its map ports from `tex_ref` or an \
              `import_image`, then either designate it the network's \
              display node or run it into `mix_material` first.\n\n\
              The factor-and-map pairing is a hand-off, not a blend: \
              connecting a map sets its factor to the multiplicative \
              identity (white, or 1.0) so the map alone drives the channel, \
              and the parameter panel dims the factor to say so. Metallic \
              Roughness is one port for two channels and neutralizes both \
              at once. It is the same surface builder the geo-side \
              `material` node uses inline; the difference is only that this \
              one outputs a Material wire instead of assigning to geometry.",
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
        category: Category::Shaders,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![
            PortSpec::single("matcap", "Matcap Image", DataType::Image, false)
                .default_port()
                .doc(
                    "The matcap image: a sphere lit the way you want the \
                     surface to look. It is sampled at the view-space \
                     normal, not at the mesh's UVs, so the mesh needs no \
                     UVs at all. Left empty, the base-color slot falls back \
                     to white and the surface renders as the flat Tint.",
                ),
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
                )
                .doc(
                    "Multiplied into every matcap sample. White (the \
                     default) leaves the image exactly as authored; a \
                     colour recolours it without needing a second image. \
                     Its alpha is ignored -- matcap always renders opaque.",
                ),
                name_param(),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A material-capture surface: the connected image is sampled by \
              the view-space normal and returned as-is, with no lighting \
              whatsoever.\n\n\
              Reach for it for a sculpt preview, or for a stylized look \
              that must not depend on the scene's lights or HDRI -- the way \
              ZBrush and Blender's solid mode shade. Feed the image from \
              `tex_ref` or an `import_image` and designate this the \
              network's display node.\n\n\
              The matcap image IS the base-color texture role; no separate \
              matcap slot exists anywhere in the pipeline. The lighting is \
              baked into the image, so shading follows the CAMERA: orbit \
              around a fixed object and the highlights swing with you \
              rather than staying put. Alpha is forced opaque, and a \
              viewport material override (Clay, Chrome, Silhouette) wins \
              over this model entirely.",
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
        category: Category::Shaders,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![
            PortSpec::single("base_color_map", "Base Color Map", DataType::Image, false)
                .default_port()
                .doc(
                    "Optional albedo texture. Connecting it drives the \
                     colour entirely and neutralizes Base Color to white; \
                     the banding then applies to the sampled colour. Left \
                     empty, Base Color is the surface colour.",
                ),
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
                .driven_by_port("base_color_map")
                .doc(
                    "The colour the bands are cut from: each band is this \
                     colour scaled by its step, so a saturated colour gives \
                     saturated bands. A connected Base Color Map \
                     neutralizes this to white and bands the map instead.",
                ),
                ParamSpec::new(
                    "steps",
                    "Bands",
                    "material",
                    ParamType::Float,
                    ParamValue::Float(3.0),
                )
                .hard(2.0, 8.0)
                .step(1.0)
                .doc(
                    "How many flat bands each light's diffuse term is \
                     quantized into, 2 to 8. 2 is the hardest, most graphic \
                     split into lit and unlit; by 8 the steps are close \
                     enough together that the cel look mostly disappears. \
                     Only the direct lights read it, never the ambient \
                     term.",
                ),
                name_param(),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "Cel shading: the surface takes an ordinary base colour, but \
              each light's diffuse contribution is quantized into a fixed \
              number of flat bands instead of falling off smoothly.\n\n\
              Reach for it inside a `matnet` for a cartoon or \
              graphic-novel look. Give it a colour or a map, set the band \
              count, and designate it the network's display node. Fewer \
              bands read as more graphic; the hard edge between them is the \
              whole point.\n\n\
              Only the DIRECT lights are banded. Ambient image-based \
              lighting is still added smoothly on top, and because this \
              node exposes no roughness the ambient term sits at the \
              shader's 0.04 roughness floor, which reads as a sharp \
              environment reflection. A bright HDRI can therefore wash the \
              bands out; turn the environment down if the banding must \
              read. There is no outline either: this node shades, it does \
              not draw contours.",
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
        category: Category::Shaders,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![
            PortSpec::single("base_color_map", "Base Color Map", DataType::Image, false)
                .default_port()
                .doc(
                    "Optional texture, multiplied by Base Color and sent \
                     straight to the screen. Connecting it neutralizes Base \
                     Color to white. Left empty, Base Color alone is what \
                     you see.",
                ),
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
                .driven_by_port("base_color_map")
                .doc(
                    "The colour the surface renders at, with no lighting \
                     applied to it, so it lands on screen as close to what \
                     you typed as the tone mapping allows. A connected Base \
                     Color Map neutralizes this to white and is shown \
                     instead.",
                ),
                name_param(),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "Flat colour with no lighting at all: the base colour times the \
              base color map, straight to the screen.\n\n\
              Reach for it inside a `matnet` for surfaces that must read at \
              exactly the colour you typed: reference planes, backdrop \
              cards, UI-like panels, or a texture whose lighting is already \
              baked in. Wire the colour or a map in and designate it the \
              network's display node.\n\n\
              Its semantics come from glTF's `KHR_materials_unlit`. Nothing \
              else in the shading pipeline reaches it: no normal map, no \
              ambient occlusion, no image-based lighting, and it is not \
              darkened by shadows falling on it. It does still CAST \
              shadows, though -- the shadow pass never reads the shading \
              model, so an unlit object occludes light like any other.",
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
        category: Category::Shaders,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![
            PortSpec::single("a", "A", DataType::Material, true)
                .default_port()
                .doc(
                    "The first material, and what Factor 0 resolves to. It \
                     is also the dominant side BELOW Factor 0.5, so its \
                     maps and shading model are the ones used there. \
                     Bypassing the node passes this input straight through.",
                ),
            PortSpec::single("b", "B", DataType::Material, true).doc(
                "The second material, and what Factor 1 resolves to. It is \
                 the dominant side at Factor 0.5 AND ABOVE, so its maps and \
                 shading model take over from the midpoint on -- exactly \
                 0.5 already counts as B, not as a tie.",
            ),
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
                .step(0.01)
                .doc(
                    "Where between A (0) and B (1) the scalar factors land. \
                     It also picks the dominant side for the half of the \
                     material that does not blend: below 0.5 A's maps and \
                     shading model are used, at 0.5 and above B's. That \
                     switch is a hard cut, so sweeping this param pops at \
                     the midpoint whenever the two sides differ in their \
                     maps or their model.",
                ),
                name_param(),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "a".to_string(),
        },
        doc: "Blends two materials, but only partially. The scalar factors \
              -- base colour, metallic, roughness, emissive, toon bands -- \
              interpolate between A and B by Factor. Everything else does \
              NOT blend: the five texture maps, the shading model and the \
              alpha settings are taken wholesale from whichever side is \
              dominant, which is B at Factor 0.5 and above, A below it.\n\n\
              So it does what you expect for two untextured surfaces of the \
              same shading model: dialing between a rough dielectric and a \
              polished metal, or animating a preset-to-preset roughness \
              change. It misleads as soon as either side carries maps or a \
              different model, because those pop over at the midpoint \
              instead of crossfading. True map and shading-model blending \
              needs shader work and is on the milestone backlog; this node \
              is a documented approximation until then.\n\n\
              Put plainly: a sweep from 0 to 1 is smooth in the factors and \
              discontinuous at 0.5 in everything else. If what you actually \
              want is two materials on one object, assign them to different \
              meshes with two `material` nodes and their `target` filters \
              instead of mixing here. Both inputs are required: this node \
              fails its cook until each side has a material.",
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
        // The principled surface properties lerp with everything else. The
        // alternative is that they fall to the struct default, which would
        // turn a mix of two glass materials into two solids without saying
        // so. Their texture slots follow the dominant side, like the five
        // above them.
        ior: lerp(a.ior, b.ior),
        transmission: lerp(a.transmission, b.transmission),
        thickness: lerp(a.thickness, b.thickness),
        attenuation_color: [
            lerp(a.attenuation_color[0], b.attenuation_color[0]),
            lerp(a.attenuation_color[1], b.attenuation_color[1]),
            lerp(a.attenuation_color[2], b.attenuation_color[2]),
        ],
        attenuation_distance: lerp(a.attenuation_distance, b.attenuation_distance),
        clearcoat: lerp(a.clearcoat, b.clearcoat),
        clearcoat_roughness: lerp(a.clearcoat_roughness, b.clearcoat_roughness),
        sheen_color: [
            lerp(a.sheen_color[0], b.sheen_color[0]),
            lerp(a.sheen_color[1], b.sheen_color[1]),
            lerp(a.sheen_color[2], b.sheen_color[2]),
        ],
        sheen_roughness: lerp(a.sheen_roughness, b.sheen_roughness),
        iridescence: lerp(a.iridescence, b.iridescence),
        iridescence_ior: lerp(a.iridescence_ior, b.iridescence_ior),
        iridescence_thickness_min: lerp(a.iridescence_thickness_min, b.iridescence_thickness_min),
        iridescence_thickness_max: lerp(a.iridescence_thickness_max, b.iridescence_thickness_max),
        specular_intensity: lerp(a.specular_intensity, b.specular_intensity),
        specular_color: [
            lerp(a.specular_color[0], b.specular_color[0]),
            lerp(a.specular_color[1], b.specular_color[1]),
            lerp(a.specular_color[2], b.specular_color[2]),
        ],
        anisotropy: lerp(a.anisotropy, b.anisotropy),
        anisotropy_rotation: lerp(a.anisotropy_rotation, b.anisotropy_rotation),
        emissive_strength: lerp(a.emissive_strength, b.emissive_strength),
        transmission_texture_data: dominant.transmission_texture_data.clone(),
        thickness_texture_data: dominant.thickness_texture_data.clone(),
        clearcoat_texture_data: dominant.clearcoat_texture_data.clone(),
        clearcoat_roughness_texture_data: dominant.clearcoat_roughness_texture_data.clone(),
        clearcoat_normal_texture_data: dominant.clearcoat_normal_texture_data.clone(),
        sheen_color_texture_data: dominant.sheen_color_texture_data.clone(),
        sheen_roughness_texture_data: dominant.sheen_roughness_texture_data.clone(),
        iridescence_texture_data: dominant.iridescence_texture_data.clone(),
        iridescence_thickness_texture_data: dominant.iridescence_thickness_texture_data.clone(),
        specular_texture_data: dominant.specular_texture_data.clone(),
        specular_color_texture_data: dominant.specular_color_texture_data.clone(),
        anisotropy_texture_data: dominant.anisotropy_texture_data.clone(),
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
        outputs: vec![
            PortSpec::single("image", "Image", DataType::Image, false)
                .default_port()
                .doc(
                    "The fetched image. It emits nothing at all while \
                     Texture Network is unset, which a downstream map port \
                     reads as `no map connected` rather than as an error.",
                ),
        ],
        params: params_with(
            "Texture Reference",
            vec![
                ParamSpec::new(
                    "texture_path",
                    "Texture Network",
                    "object",
                    ParamType::NodePath {
                        accept: NodePathAccept::Opens(ContextKind::Tex),
                    },
                    ParamValue::NodeRef(None),
                )
                .doc(
                    "The `texnet` to fetch from; only containers that open a \
                     texture context can be picked. What arrives is that \
                     network's display node output, so re-designating the \
                     display node inside it changes every referrer at once. \
                     Left unset this node simply emits nothing, but a path \
                     aimed at a deleted node or at a network that publishes \
                     nothing fails the cook rather than yielding a blank \
                     image.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "Pulls the image a texture network publishes into this network \
              as an Image wire. It reads across contexts by path, so no wire \
              ever crosses a network boundary.\n\n\
              Point it at a `texnet` and feed the result into a map port on \
              `principled`, or into the geo-side `material` node -- it is \
              placeable in both Mat and Geo networks for exactly that \
              reason. One texture network can back any number of these, \
              which is how a texture gets authored once and used \
              everywhere; editing the network recooks every referrer.\n\n\
              This is the fetch pattern rather than a wire, so the \
              dependency is invisible on the canvas: nothing draws a line \
              from the `texnet` to here, and the only record of the link is \
              this node's Texture Network param. An unset path is harmless \
              (no output, read downstream as no map), but a path pointing \
              at a network with no display node is a cook error.",
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

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::cook::InputSlot;
    use std::collections::BTreeMap;

    fn resolved(factor: f64) -> ResolvedParams {
        let mut stored = BTreeMap::new();
        stored.insert(
            "factor".to_string(),
            crate::params::ParamSource::Literal(ParamValue::Float(factor)),
        );
        crate::registry::resolve::resolve_params(&stored, &mix_material_descriptor().params)
            .unwrap()
    }

    fn slot(key: &str, mat: RawMaterialData) -> (String, InputSlot) {
        (
            key.to_string(),
            InputSlot::Single(Value::Material(Arc::new(mat))),
        )
    }

    fn mix(a: RawMaterialData, b: RawMaterialData, factor: f64) -> Arc<RawMaterialData> {
        let params = resolved(factor);
        let assets = crate::assets::AssetTable::default();
        let mut cx = CookCtx::new(&assets, false);
        let inputs = Inputs::new(vec![slot("a", a), slot("b", b)].into_iter().collect());
        let CookOutcome::Done(outputs) = cook_mix_material(&params, &inputs, &mut cx).unwrap()
        else {
            panic!("mix cook is synchronous");
        };
        Arc::clone(outputs.get("material").unwrap().as_material().unwrap())
    }

    /// Mixing two glass materials must produce glass. Before the principled
    /// properties were added to the lerp list they fell through to the
    /// struct default, so a mix silently turned two panes of glass into two
    /// solids with nothing reported anywhere.
    #[test]
    fn mixing_carries_the_principled_properties_rather_than_zeroing_them() {
        let glass = |transmission: f32| RawMaterialData {
            name: "glass".to_string(),
            transmission,
            ior: 1.5,
            clearcoat: 1.0,
            sheen_color: [0.2, 0.2, 0.2],
            specular_intensity: 0.5,
            anisotropy: 0.4,
            emissive_strength: 2.0,
            ..RawMaterialData::default()
        };

        let m = mix(glass(1.0), glass(0.8), 0.5);
        assert!((m.transmission - 0.9).abs() < 1e-6, "lerped, not dropped");
        assert!((m.ior - 1.5).abs() < 1e-6, "ior");
        assert!((m.clearcoat - 1.0).abs() < 1e-6, "clearcoat");
        assert_eq!(m.sheen_color, [0.2, 0.2, 0.2]);
        assert!((m.specular_intensity - 0.5).abs() < 1e-6, "specular");
        assert!((m.anisotropy - 0.4).abs() < 1e-6, "anisotropy");
        assert!((m.emissive_strength - 2.0).abs() < 1e-6, "emissive");
    }

    /// The endpoints, which is where a wrong lerp direction shows up.
    #[test]
    fn mixing_at_the_endpoints_reproduces_each_side() {
        let a = RawMaterialData {
            transmission: 1.0,
            iridescence: 0.25,
            ..RawMaterialData::default()
        };
        let b = RawMaterialData {
            transmission: 0.0,
            iridescence: 0.75,
            ..RawMaterialData::default()
        };

        let at_a = mix(a.clone(), b.clone(), 0.0);
        assert!((at_a.transmission - 1.0).abs() < 1e-6);
        assert!((at_a.iridescence - 0.25).abs() < 1e-6);

        let at_b = mix(a, b, 1.0);
        assert!((at_b.transmission - 0.0).abs() < 1e-6);
        assert!((at_b.iridescence - 0.75).abs() < 1e-6);
    }

    /// Two materials differing only in a principled property are two
    /// materials. The merge dedup confirms equality inside a hash bucket, so
    /// this holds whether or not the hash covers the field, but a hash that
    /// ignored them would degrade the bucket to a linear scan.
    #[test]
    fn materials_differing_only_in_a_principled_property_do_not_dedup() {
        let base = RawMaterialData {
            name: "glass".to_string(),
            ..RawMaterialData::default()
        };
        let coated = RawMaterialData {
            clearcoat: 1.0,
            ..base.clone()
        };
        assert_ne!(base, coated);
        assert_ne!(
            solarxy_kernel::merge::material_content_hash(&base),
            solarxy_kernel::merge::material_content_hash(&coated),
        );
    }
}
