//! The texture-context composite, filter, and PBR-utility nodes: `mix`,
//! `blur`, `sharpen`, `pack_orm`, and `height_to_normal`.

use super::common::params_with;
use super::image_support::{image_in, image_out, image_outputs, require_image, working_image};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

// Mirrors the fields of NodeTypeDescriptor that vary per tex node; grouping
// them into a struct would just restate the descriptor itself.
#[allow(clippy::too_many_arguments)]
fn tex_node(
    type_id: &'static str,
    display_name: &'static str,
    inputs: Vec<PortSpec>,
    params: Vec<ParamSpec>,
    bypass: BypassBehavior,
    doc: &'static str,
    search_aliases: &'static [&'static str],
    role: NodeRole,
    cook: crate::cook::CookFn,
) -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id,
        version: 1,
        display_name,
        category: Category::TexComposite,
        contexts: ContextSet::TEX,
        opens: None,
        inputs,
        outputs: vec![image_out()],
        params: params_with(display_name, params),
        bypass,
        doc,
        search_aliases,
        glyph: type_id,
        role,
        cook,
        migrate: None,
    }
}

#[must_use]
pub fn mix_descriptor() -> NodeTypeDescriptor {
    tex_node(
        "mix",
        "Mix",
        vec![
            image_in(true).doc(
                "The base layer. The output takes this image's dimensions, \
                 every mode but Over keeps its alpha, and Factor always \
                 fades back toward it. Being the default input, a body drag \
                 wires here, and a bypass passes it straight through.",
            ),
            PortSpec::single("blend", "Blend", DataType::Image, false).doc(
                "The layer composited onto the base. Optional: leave it \
                 unconnected and the node passes the base through untouched, \
                 whatever Mode and Factor say. When its size differs from \
                 the base it is nearest-sampled to fit, so it need not \
                 match, but it will alias if it does not.",
            ),
        ],
        vec![
            ParamSpec::new(
                "mode",
                "Mode",
                "mix",
                ParamType::Enum {
                    variants: vec![
                        EnumVariant::new("normal", "Normal"),
                        EnumVariant::new("over", "Over"),
                        EnumVariant::new("multiply", "Multiply"),
                        EnumVariant::new("add", "Add"),
                        EnumVariant::new("screen", "Screen"),
                        EnumVariant::new("overlay", "Overlay"),
                    ],
                },
                ParamValue::Enum("normal".to_string()),
            )
            .doc(
                "How the two images combine before Factor fades the result. \
                 Normal replaces the base outright. Over does source-over \
                 alpha compositing and is the only mode that reads the blend \
                 image's alpha or writes a new one. Multiply darkens; Add \
                 and Screen brighten, Add clipping at 1 where Screen only \
                 approaches it; Overlay multiplies where the base is dark \
                 and screens where it is bright, pivoting at 0.5.",
            ),
            ParamSpec::new(
                "factor",
                "Factor",
                "mix",
                ParamType::Float,
                ParamValue::Float(1.0),
            )
            .hard(0.0, 1.0)
            .step(0.01)
            .doc(
                "Fades the blend in. At 0 the base passes through untouched, \
                 at 1 the mode applies at full strength. In Over it scales \
                 the blend image's alpha rather than lerping the color, so a \
                 half-factor Over of an opaque image is a half-opaque \
                 composite, not a half-blended one.",
            ),
        ],
        BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        "Composites the Blend image onto the Image input under one of six \
         modes, with Factor fading the result in. The output takes the base \
         input's dimensions, and the blend is nearest-sampled to fit when \
         the two differ.\n\n\
         The composite node: layers in a texture network stack through it. A \
         `constant` or `ramp` as the base, an `import_image` or `noise` as \
         the blend, Multiply or Overlay to combine them. Chain several to \
         build a surface up the way you would layers in a 2D editor.\n\n\
         The two inputs are not interchangeable. The output always takes the \
         base's size, every mode but Over keeps the base's alpha, and Factor \
         always fades back toward the base -- so swapping the wires is not a \
         no-op even in the symmetric modes. The resample is \
         nearest-neighbour with no filtering, so match sizes when it \
         matters.",
        &["mix", "blend", "composite", "over", "multiply", "screen"],
        NodeRole::Gather,
        cook_mix,
    )
}

fn cook_mix(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let a = require_image(inputs)?;
    // An unconnected blend input passes the base through untouched.
    let Some(b) = working_image(inputs, "blend") else {
        return Ok(CookOutcome::Done(image_outputs((*a).clone())));
    };
    let mode = match p.enum_key("mode") {
        "over" => solarxy_imaging::BlendMode::Over,
        "multiply" => solarxy_imaging::BlendMode::Multiply,
        "add" => solarxy_imaging::BlendMode::Add,
        "screen" => solarxy_imaging::BlendMode::Screen,
        "overlay" => solarxy_imaging::BlendMode::Overlay,
        _ => solarxy_imaging::BlendMode::Normal,
    };
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::mix(
        &a,
        &b,
        mode,
        p.f32("factor"),
    ))))
}

#[must_use]
pub fn blur_descriptor() -> NodeTypeDescriptor {
    tex_node(
        "blur",
        "Blur",
        vec![image_in(true)],
        vec![
            ParamSpec::new(
                "radius",
                "Radius",
                "filter",
                ParamType::Float,
                ParamValue::Float(4.0),
            )
            .hard(0.0, 64.0)
            .soft(0.0, 16.0)
            .step(0.5)
            .doc(
                "Gaussian radius in pixels of the working resolution; sigma \
                 is half of it. 0 is a pass-through. Cost climbs linearly, \
                 each of the two passes taking 2*radius+1 samples per pixel, \
                 which is why the slider stops at 16 even though you can \
                 type up to 64.",
            ),
        ],
        BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        "Separable Gaussian blur over all four channels: two 1D passes, \
         horizontal then vertical, with edge pixels clamping to the border \
         rather than wrapping.\n\n\
         The softener. Reach for it to take the hard edges off a `noise` \
         mask, to pre-soften a height field before `height_to_normal` so the \
         normals are not all needle, or to build a glow by blurring a bright \
         layer and adding it back through `mix`.\n\n\
         Alone among the image nodes it filters alpha as well as RGB, so \
         blurring a partly transparent image bleeds its edges. Radius counts \
         pixels of the WORKING resolution, not of the source, so an image \
         that got clamped on the way in blurs relatively harder than its \
         full-size original would. It is not free: at radius 64 each of the \
         two passes takes 129 samples per pixel.",
        &["blur", "gaussian", "soften"],
        NodeRole::Standard,
        cook_blur,
    )
}

fn cook_blur(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::blur(
        &img,
        p.f32("radius"),
    ))))
}

#[must_use]
pub fn sharpen_descriptor() -> NodeTypeDescriptor {
    tex_node(
        "sharpen",
        "Sharpen",
        vec![image_in(true)],
        vec![
            ParamSpec::new(
                "amount",
                "Amount",
                "filter",
                ParamType::Float,
                ParamValue::Float(1.0),
            )
            .hard(0.0, 4.0)
            .step(0.05)
            .doc(
                "Scales the high-pass detail added back, where 0 is a \
                 pass-through and 1 adds it at full strength. The radius \
                 that detail is extracted at is fixed, so this only controls \
                 how hard the fine detail is pushed. High values ring at \
                 high-contrast edges, the usual unsharp halo, and clip once \
                 the ring reaches black or white.",
            ),
        ],
        BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        "Unsharp mask: blurs a copy of the image at a fixed 1.5 px radius, \
         then adds the difference between the original and that blur back \
         onto the original, scaled by Amount. RGB only; alpha is \
         untouched.\n\n\
         The detail-recovery step, usually last in a chain. It earns its \
         place after a `blur`, after an image has been clamped down to the \
         working resolution, or on an imported image that reads soft. It is \
         the natural opposite of `blur`, though it does not undo one.\n\n\
         The blur radius is not exposed: 1.5 px is baked in, so this \
         sharpens fine detail only and cannot do the wide, halo-style \
         sharpen a variable-radius unsharp mask would. Amount runs to 4, but \
         the result clips at black and white, so anything already near an \
         end of the range flattens into a hard edge instead of getting \
         crisper. 0 is a pass-through.",
        &["sharpen", "unsharp", "detail"],
        NodeRole::Standard,
        cook_sharpen,
    )
}

fn cook_sharpen(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::sharpen(
        &img,
        p.f32("amount"),
    ))))
}

#[must_use]
pub fn pack_orm_descriptor() -> NodeTypeDescriptor {
    let fallback =
        |key: &'static str, label: &'static str, default: f64, port: &'static str, doc: &str| {
            ParamSpec::new(
                key,
                label,
                "channels",
                ParamType::Float,
                ParamValue::Float(default),
            )
            .hard(0.0, 1.0)
            .step(0.01)
            .driven_by_port(port)
            .doc(doc)
        };
    tex_node(
        "pack_orm",
        "Pack ORM",
        vec![
            PortSpec::single("occlusion", "Occlusion", DataType::Image, false)
                .default_port()
                .doc(
                    "An ambient-occlusion map; its RED channel becomes the \
                     output's red. Unconnected, the Occlusion constant fills \
                     that channel flat. Being both the default input and \
                     first in order, a body drop wires here, and when it is \
                     connected it sets the output's dimensions.",
                ),
            PortSpec::single("roughness", "Roughness", DataType::Image, false).doc(
                "A roughness map; its RED channel becomes the output's \
                 green. Unconnected, the Roughness constant fills that \
                 channel flat. Nearest-sampled if its size differs from \
                 whichever input set the output dimensions.",
            ),
            PortSpec::single("metallic", "Metallic", DataType::Image, false).doc(
                "A metallic map; its RED channel becomes the output's blue. \
                 Unconnected, the Metallic constant fills that channel flat, \
                 which is the common case: most surfaces are uniformly metal \
                 or uniformly not.",
            ),
        ],
        vec![
            fallback(
                "occlusion",
                "Occlusion",
                1.0,
                "occlusion",
                "The flat value packed into red when the Occlusion input is \
                 unconnected. 1 means no occlusion, which is why it is the \
                 default: an ORM map with no AO in it should not darken \
                 anything. Connecting the input neutralizes this.",
            ),
            fallback(
                "roughness",
                "Roughness",
                0.7,
                "roughness",
                "The flat value packed into green when the Roughness input \
                 is unconnected, where 0 is a mirror and 1 is fully diffuse. \
                 The 0.7 default is a plausibly matte surface. Connecting the \
                 input neutralizes this.",
            ),
            fallback(
                "metallic",
                "Metallic",
                0.0,
                "metallic",
                "The flat value packed into blue when the Metallic input is \
                 unconnected. It is effectively a binary choice: 0 for a \
                 dielectric (the default) and 1 for a metal, the values \
                 between only meaning anything where a map blends across the \
                 boundary. Connecting the input neutralizes this.",
            ),
        ],
        BypassBehavior::Mute,
        "Packs three grayscale maps into the one image the renderer consumes \
         for PBR, the glTF way: red carries occlusion, green carries \
         roughness, blue carries metallic. Each input contributes its RED \
         channel only, an unconnected input is filled with its constant \
         instead, and alpha is always opaque.\n\n\
         The last node before a texture network feeds a material's ORM slot. \
         Wire `noise`, `ramp` or `import_image` maps into the three inputs, \
         or leave one out and dial its constant -- Metallic at 0 with a \
         roughness map in green is the everyday dielectric case. Point the \
         network's display flag at this node, then reference the network \
         from a material by path with a `tex_ref`.\n\n\
         Only the red channel of each input is read, so a color image \
         silently contributes its red and its other channels are dropped. \
         The output takes the dimensions of the FIRST connected input in \
         occlusion, roughness, metallic order, and the other two are \
         nearest-sampled to fit; with nothing connected at all you get a \
         single 1x1 pixel of the three constants.",
        &["orm", "pack", "occlusion", "roughness", "metallic", "gltf"],
        NodeRole::Gather,
        cook_pack_orm,
    )
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_pack_orm(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let occlusion = working_image(inputs, "occlusion");
    let roughness = working_image(inputs, "roughness");
    let metallic = working_image(inputs, "metallic");
    Ok(CookOutcome::Done(image_outputs(solarxy_imaging::pack_orm(
        occlusion.as_deref(),
        roughness.as_deref(),
        metallic.as_deref(),
        p.f32("occlusion"),
        p.f32("roughness"),
        p.f32("metallic"),
    ))))
}

#[must_use]
pub fn height_to_normal_descriptor() -> NodeTypeDescriptor {
    tex_node(
        "height_to_normal",
        "Height to Normal",
        vec![image_in(true).doc(
            "The height field. Only the RED channel is read, as a height in \
             0..1; a color image works but silently uses its red. Being the \
             default input, a body drag wires here. Blur it first if the \
             field is noisy: a Sobel over raw noise gives needle-sharp \
             normals.",
        )],
        vec![
            ParamSpec::new(
                "strength",
                "Strength",
                "normal",
                ParamType::Float,
                ParamValue::Float(4.0),
            )
            .hard(0.0, 16.0)
            .step(0.1)
            .doc(
                "Scales the height slope before it becomes a normal: 0 emits \
                 a flat map whatever the input says, and higher values tilt \
                 the normals further from straight up. It is a gain on the \
                 gradient, not a height in metres, so the same value over a \
                 smooth field and a noisy one gives very different results. \
                 Tune it against the shaded preview.",
            ),
        ],
        BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        "Reads the input's red channel as a height field, takes its slope \
         with a 3x3 Sobel filter, and writes the resulting surface normal \
         out as a tangent-space normal map. Flat encodes as (128, 128, 255), \
         edge pixels clamp, and alpha is opaque.\n\n\
         The last step of a procedural bump chain: `noise` for the height, \
         `blur` and `levels` to shape it, then this, then the network's \
         display node so a material can reference it by path. Feeding it an \
         imported grayscale height map does the same job for scanned \
         detail.\n\n\
         The output is a normal map, not a color: do not run the adjust \
         nodes on it afterwards, because they operate on encoded color and \
         will denormalize the vectors. Bypassing this node is not a no-op \
         either -- it passes the raw HEIGHT image downstream, which a \
         material will happily read as a normal map and shade wrong.",
        &["normal", "height", "bump", "sobel"],
        NodeRole::Standard,
        cook_height_to_normal,
    )
}

fn cook_height_to_normal(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let img = require_image(inputs)?;
    Ok(CookOutcome::Done(image_outputs(
        solarxy_imaging::height_to_normal(&img, p.f32("strength")),
    )))
}
