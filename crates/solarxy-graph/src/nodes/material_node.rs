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
//! `occlusion_map` have no factor to neutralize. Alpha is Opaque
//! (alpha-mode control is a backlog note).
//!
//! v3 adds the principled surface properties: transmission and the volume
//! behind it, clearcoat, sheen, iridescence, specular tint, anisotropy,
//! index of refraction and emissive strength. They are grouped into Base,
//! Surface and Volume tabs with a labelled subsection per family, rather
//! than a tab per family, because the parameter panel's tab strip is a
//! single non-wrapping row that five more tabs would overflow.
//!
//! **None of them has a map port**, and that is a platform limit rather
//! than an omission: the main pass's fragment stage already binds 10 of
//! the 16 sampled textures core WebGPU guarantees, and the twelve
//! extension texture slots do not fit in the remainder. An imported
//! material still carries those maps and still exports them; what this
//! node cannot yet do is author one.

use std::sync::Arc;

use solarxy_core::RawMaterialData;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, NodePathAccept, ParamSpec, ParamType, Pred, Unit};
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
            .carries_placements()
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
        // v2: Reference mode + per-slot targeting. v3: the principled
        // surface properties. Both are pure additions, and an added param
        // fills from the registry default on load, so no migration hook is
        // needed and documents from either earlier version keep their exact
        // behavior. Every principled default is the identity of its effect,
        // which is what makes that true here.
        version: 3,
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
                    "base",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("inline", "Inline"),
                            EnumVariant::new("reference", "Reference"),
                        ],
                    },
                    ParamValue::Enum("inline".to_string()),
                )
                .subgroup("Assignment")
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
                    "base",
                    ParamType::NodePath {
                        accept: NodePathAccept::Opens(crate::document::ContextKind::Mat),
                    },
                    ParamValue::NodeRef(None),
                )
                .subgroup("Assignment")
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
                    "base",
                    ParamType::Text,
                    ParamValue::Text(String::new()),
                )
                .subgroup("Assignment")
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
    let mut specs = vec![
        ParamSpec::new(
            "base_color",
            "Base Color",
            "base",
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
            "base",
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
            "base",
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
            "base",
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
            "emissive_strength",
            "Emissive Strength",
            "base",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .hard(0.0, 100.0)
        .soft(0.0, 10.0)
        .step(0.01)
        .doc(
            "Multiplies Emissive, so emission can exceed the unit range a \
             colour can express. 1 is no change. Reach for it when a \
             surface should read as a light source rather than as a bright \
             material: past about 1 the tone mapper starts rolling it off, \
             which is what makes it bloom rather than clip.",
        ),
        ParamSpec::new(
            "material_name",
            "Material Name",
            "base",
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
    ];
    specs.extend(surface_params());
    specs.extend(volume_params());
    specs
}

/// The principled layers over the base surface, grouped into one tab with
/// a labelled division per family.
///
/// One tab per family would be the obvious shape and is the wrong one: the
/// tab strip is a single non-wrapping row, and five more tabs overflow the
/// panel at its usual width. Broad tabs with subgroup headings keep the
/// established underline-tab idiom and add no new widget.
fn surface_params() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "clearcoat",
            "Clearcoat",
            "surface",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .subgroup("Clearcoat")
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .doc(
            "A thin glossy layer over the surface, the lacquer on a car \
             panel or the varnish on wood. 0 is no coat. The coat adds its \
             own reflection and dims what is under it by whatever it \
             reflects away, so a coated surface reads slightly darker as \
             well as shinier. It is an analytic approximation in the \
             interactive viewport rather than a simulated layer.",
        ),
        ParamSpec::new(
            "clearcoat_roughness",
            "Clearcoat Roughness",
            "surface",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .subgroup("Clearcoat")
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .show_if("clearcoat", Pred::Truthy)
        .doc(
            "How polished the coat is, independent of the surface beneath \
             it. This is the point of a separate coat: a rough, worn base \
             under a mirror-smooth lacquer is a combination one roughness \
             value cannot express.",
        ),
        ParamSpec::new(
            "sheen_color",
            "Sheen Color",
            "surface",
            ParamType::Color,
            ParamValue::Color([0.0, 0.0, 0.0, 1.0]),
        )
        .subgroup("Sheen")
        .doc(
            "The colour of the soft retroreflective rim that fabric has, \
             brightest where the surface turns away from you. Black, the \
             default, is no sheen, which is why this one starts at black \
             rather than white. Velvet, satin and brushed cloth are what \
             it is for. It is an analytic approximation in the interactive \
             viewport.",
        ),
        ParamSpec::new(
            "sheen_roughness",
            "Sheen Roughness",
            "surface",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .subgroup("Sheen")
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .doc(
            "How wide the sheen band is. Low keeps it a tight rim at the \
             silhouette; high spreads it across the whole surface for a \
             dusty, powdery look.",
        ),
        ParamSpec::new(
            "iridescence",
            "Iridescence",
            "surface",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .subgroup("Iridescence")
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .doc(
            "Thin-film interference: the shifting colours of a soap \
             bubble, an oil slick or anodized metal. 0 is none. The hue \
             depends on viewing angle and on the film's thickness, so it \
             moves as the camera moves, which is the whole effect. It is \
             an analytic approximation in the interactive viewport.",
        ),
        ParamSpec::new(
            "iridescence_ior",
            "Film IOR",
            "surface",
            ParamType::Float,
            ParamValue::Float(1.3),
        )
        .subgroup("Iridescence")
        .hard(1.0, 3.0)
        .soft(1.0, 2.5)
        .step(0.01)
        .show_if("iridescence", Pred::Truthy)
        .doc(
            "Index of refraction of the film itself, not of the surface \
             under it. It sets how strongly the film bends light and so \
             how saturated the interference colours are.",
        ),
        ParamSpec::new(
            "iridescence_thickness_min",
            "Film Thickness Min",
            "surface",
            ParamType::Float,
            ParamValue::Float(100.0),
        )
        .subgroup("Iridescence")
        .hard(0.0, 2000.0)
        .soft(0.0, 1000.0)
        .step(1.0)
        .show_if("iridescence", Pred::Truthy)
        .doc(
            "The low end of the film thickness range, in nanometres. It \
             only matters once a thickness map varies the film across the \
             surface; with no map the maximum is used everywhere.",
        ),
        ParamSpec::new(
            "iridescence_thickness_max",
            "Film Thickness Max",
            "surface",
            ParamType::Float,
            ParamValue::Float(400.0),
        )
        .subgroup("Iridescence")
        .hard(0.0, 2000.0)
        .soft(0.0, 1000.0)
        .step(1.0)
        .show_if("iridescence", Pred::Truthy)
        .doc(
            "The high end of the film thickness range, in nanometres, and \
             the thickness used everywhere when no thickness map is \
             present. This is the dial that chooses which colours appear: \
             a few hundred nanometres is where visible light interferes, \
             and sweeping it walks the whole rainbow.",
        ),
        ParamSpec::new(
            "specular_intensity",
            "Specular",
            "surface",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
        .subgroup("Specular and anisotropy")
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .doc(
            "Scales the reflectance a dielectric has when you look \
             straight at it, the value IOR derives. 1 leaves it alone. \
             Lower it to dull a surface's reflections without making it \
             rougher, which roughness alone cannot do. It has no effect on \
             metals, whose reflectance is the base colour.",
        ),
        ParamSpec::new(
            "specular_color",
            "Specular Color",
            "surface",
            ParamType::Color,
            ParamValue::Color([1.0, 1.0, 1.0, 1.0]),
        )
        .subgroup("Specular and anisotropy")
        .doc(
            "Tints that same head-on reflectance. White is untinted and is \
             what almost every real dielectric wants; this exists for the \
             ones that do not, and for matching a reference that was \
             authored with it.",
        ),
        ParamSpec::new(
            "anisotropy",
            "Anisotropy",
            "surface",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .subgroup("Specular and anisotropy")
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .doc(
            "Stretches the specular highlight along one direction instead \
             of leaving it round: brushed metal, hair, the bottom of a \
             saucepan. 0 is isotropic. It follows the surface's tangents, \
             so it needs UVs to point anywhere meaningful.",
        ),
        ParamSpec::new(
            "anisotropy_rotation",
            "Anisotropy Rotation",
            "surface",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .subgroup("Specular and anisotropy")
        .hard(-360.0, 360.0)
        .soft(0.0, 360.0)
        .step(1.0)
        .unit(Unit::Degrees)
        .show_if("anisotropy", Pred::Truthy)
        .doc(
            "Turns the stretch direction within the surface, for when the \
             brushing runs across the UVs rather than along them.",
        ),
    ]
}

/// Transmission and the volume behind it.
fn volume_params() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new(
            "transmission",
            "Transmission",
            "volume",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .hard(0.0, 1.0)
        .soft(0.0, 1.0)
        .step(0.01)
        .doc(
            "How much light passes through the surface instead of \
             scattering back off it: glass, water, a thin plastic. 0 is \
             opaque. This is not the same as alpha, which makes a surface \
             partly absent; transmission keeps the surface and its \
             reflections and lets light through it.\n\n\
             In the interactive viewport it refracts the environment, not \
             the objects behind the surface. Glass reads correctly against \
             an environment and shows nothing of what is behind it.",
        ),
        ParamSpec::new(
            "ior",
            "IOR",
            "volume",
            ParamType::Float,
            ParamValue::Float(1.5),
        )
        .hard(1.0, 3.0)
        .soft(1.0, 2.5)
        .step(0.01)
        .doc(
            "Index of refraction: how strongly the material bends light, \
             and with it how much reflects at a glancing angle. 1.5 is \
             window glass and the default every surface used before this \
             was exposed. Water is about 1.33, diamond about 2.42. It \
             drives reflectance whether or not the surface transmits.",
        ),
        ParamSpec::new(
            "thickness",
            "Thickness",
            "volume",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .hard(0.0, 1000.0)
        .soft(0.0, 10.0)
        .step(0.01)
        .show_if("transmission", Pred::Truthy)
        .doc(
            "How far light travels through the interior, in world units. \
             0 means the surface is thin-walled, a bubble or a pane with \
             no volume behind it. It only matters alongside Attenuation \
             Distance, which is what turns distance travelled into colour.",
        ),
        ParamSpec::new(
            "attenuation_color",
            "Attenuation Color",
            "volume",
            ParamType::Color,
            ParamValue::Color([1.0, 1.0, 1.0, 1.0]),
        )
        .subgroup("Absorption")
        .show_if("transmission", Pred::Truthy)
        .doc(
            "The colour light becomes after travelling Attenuation \
             Distance through the interior. White is no tint. This is why \
             thick green glass is green at its edge and clear at its face: \
             the colour is a property of the distance travelled, not of \
             the surface.",
        ),
        ParamSpec::new(
            "attenuation_distance",
            "Attenuation Distance",
            "volume",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
        .subgroup("Absorption")
        .hard(0.0, 1000.0)
        .soft(0.0, 10.0)
        .step(0.01)
        .show_if("transmission", Pred::Truthy)
        .doc(
            "The distance over which light reaches Attenuation Color. \
             Shorter absorbs faster, so a small value tints even thin \
             glass strongly. 0 disables absorption entirely and is the \
             default, which is why a transmissive surface starts out \
             water-clear.",
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
        // The principled layers. No neutralization pairs with these: none
        // of them has a map port, because the fragment stage is at 10 of
        // the 16 sampled textures core WebGPU guarantees and their twelve
        // slots do not fit in the remainder. An imported material still
        // carries those maps and still exports them; what this node cannot
        // yet do is author one.
        ior: p.f32("ior"),
        transmission: p.f32("transmission"),
        thickness: p.f32("thickness"),
        attenuation_color: {
            let c = p.color("attenuation_color");
            [c[0], c[1], c[2]]
        },
        attenuation_distance: p.f32("attenuation_distance"),
        clearcoat: p.f32("clearcoat"),
        clearcoat_roughness: p.f32("clearcoat_roughness"),
        sheen_color: {
            let c = p.color("sheen_color");
            [c[0], c[1], c[2]]
        },
        sheen_roughness: p.f32("sheen_roughness"),
        iridescence: p.f32("iridescence"),
        iridescence_ior: p.f32("iridescence_ior"),
        iridescence_thickness_min: p.f32("iridescence_thickness_min"),
        iridescence_thickness_max: p.f32("iridescence_thickness_max"),
        specular_intensity: p.f32("specular_intensity"),
        specular_color: {
            let c = p.color("specular_color");
            [c[0], c[1], c[2]]
        },
        anisotropy: p.f32("anisotropy"),
        anisotropy_rotation: p.f32("anisotropy_rotation"),
        emissive_strength: p.f32("emissive_strength"),
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

    #[test]
    fn the_principled_defaults_are_the_identity_of_every_effect() {
        // The load-bearing property of the whole set: a material that
        // touches none of these must shade exactly as it did before they
        // existed. The renderer holds the other half of this (its golden
        // captures did not move); this is the engine half.
        let p = resolved(&[]);
        let set = cook_with(&p, vec![geo_input()]);
        let m = &set.materials[0];

        assert_eq!(m.ior, 1.5);
        assert_eq!(m.specular_intensity, 1.0);
        assert_eq!(m.specular_color, [1.0, 1.0, 1.0]);
        assert_eq!(m.emissive_strength, 1.0);
        assert_eq!(m.attenuation_color, [1.0, 1.0, 1.0]);
        assert_eq!(m.iridescence_ior, 1.3);
        assert_eq!(m.transmission, 0.0);
        assert_eq!(m.thickness, 0.0);
        assert_eq!(m.attenuation_distance, 0.0);
        assert_eq!(m.clearcoat, 0.0);
        assert_eq!(m.sheen_color, [0.0, 0.0, 0.0]);
        assert_eq!(m.iridescence, 0.0);
        assert_eq!(m.anisotropy, 0.0);
    }

    #[test]
    fn every_principled_param_reaches_the_material() {
        // One assertion per param, because the cook writes them by hand and
        // a copy-paste that reads the neighbouring key would otherwise be
        // invisible: both values are plausible floats.
        let p = resolved(&[
            ("ior", ParamValue::Float(1.7)),
            ("transmission", ParamValue::Float(0.9)),
            ("thickness", ParamValue::Float(2.5)),
            ("attenuation_color", ParamValue::Color([0.8, 0.2, 0.1, 1.0])),
            ("attenuation_distance", ParamValue::Float(3.0)),
            ("clearcoat", ParamValue::Float(0.75)),
            ("clearcoat_roughness", ParamValue::Float(0.25)),
            ("sheen_color", ParamValue::Color([0.4, 0.5, 0.6, 1.0])),
            ("sheen_roughness", ParamValue::Float(0.35)),
            ("iridescence", ParamValue::Float(0.5)),
            ("iridescence_ior", ParamValue::Float(1.8)),
            ("iridescence_thickness_min", ParamValue::Float(200.0)),
            ("iridescence_thickness_max", ParamValue::Float(600.0)),
            ("specular_intensity", ParamValue::Float(0.6)),
            ("specular_color", ParamValue::Color([0.9, 0.8, 0.7, 1.0])),
            ("anisotropy", ParamValue::Float(0.65)),
            ("emissive_strength", ParamValue::Float(4.0)),
        ]);
        let set = cook_with(&p, vec![geo_input()]);
        let m = &set.materials[0];

        assert_eq!(m.ior, 1.7);
        assert_eq!(m.transmission, 0.9);
        assert_eq!(m.thickness, 2.5);
        assert_eq!(m.attenuation_color, [0.8, 0.2, 0.1]);
        assert_eq!(m.attenuation_distance, 3.0);
        assert_eq!(m.clearcoat, 0.75);
        assert_eq!(m.clearcoat_roughness, 0.25);
        assert_eq!(m.sheen_color, [0.4, 0.5, 0.6]);
        assert_eq!(m.sheen_roughness, 0.35);
        assert_eq!(m.iridescence, 0.5);
        assert_eq!(m.iridescence_ior, 1.8);
        assert_eq!(m.iridescence_thickness_min, 200.0);
        assert_eq!(m.iridescence_thickness_max, 600.0);
        assert_eq!(m.specular_intensity, 0.6);
        assert_eq!(m.specular_color, [0.9, 0.8, 0.7]);
        assert_eq!(m.anisotropy, 0.65);
        assert_eq!(m.emissive_strength, 4.0);
    }

    #[test]
    fn the_angle_param_is_stored_in_degrees_and_resolves_to_radians() {
        // Unit::Degrees drives both the panel suffix and the resolver
        // conversion, so the stored 90 must arrive as a quarter turn.
        let p = resolved(&[("anisotropy_rotation", ParamValue::Float(90.0))]);
        let set = cook_with(&p, vec![geo_input()]);
        let expected = std::f32::consts::FRAC_PI_2;
        assert!((set.materials[0].anisotropy_rotation - expected).abs() < 1e-6);
    }

    #[test]
    fn the_material_and_principled_nodes_declare_the_same_surface() {
        // They share MAP_PORTS and factor_params precisely so the two
        // cannot describe the same surface differently. The material node
        // adds its assignment params on top, so compare the shared subset.
        let mat = descriptor();
        let pr = crate::nodes::mat_nodes::principled_descriptor();
        let assignment = ["mode", "material_path", "target"];
        let shared: Vec<_> = mat
            .params
            .iter()
            .filter(|p| !assignment.contains(&p.key.as_str()))
            .map(|p| (p.key.as_str(), p.group.as_str(), p.subgroup.clone()))
            .collect();
        let theirs: Vec<_> = pr
            .params
            .iter()
            .map(|p| (p.key.as_str(), p.group.as_str(), p.subgroup.clone()))
            .collect();
        assert_eq!(shared, theirs);
    }
}
