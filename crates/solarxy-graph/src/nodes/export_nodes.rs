//! The output nodes:
//! `geo_export` (Geo), `image_export` (Tex), and the root `render`
//! configuration node.
//!
//! Export nodes are save-only and PASS THROUGH: they sit in a chain
//! without changing it, and their Save button (a `ParamType::Action`)
//! routes through `Engine::invoke_action`, which encodes the node's
//! committed output via `solarxy-formats`' writers and hands bytes to the
//! host for saving. Neither writer carries materials: `ExportMesh` has no
//! material field, so every geometry format exports bare geometry (a
//! recorded follow-up, not a format limitation).
//!
//! The `render` node is a config carrier (camera by path, resolution); its
//! Render button is HOST-interpreted (the engine cannot render) and never
//! reaches `invoke_action`, driving the existing screenshot path instead.

use super::common::{geometry_output, params_with, passive_cook};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::{DataType, Value};
use crate::registry::param_spec::{EnumVariant, NodePathAccept, ParamSpec, ParamType, Pred};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

/// Which renderer a still is drawn with, as the node declares it.
///
/// Named here rather than in a host because the choice is authored on the node
/// and saved with the document. A host maps it onto its own backend; the engine
/// does not know what a backend is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderEngine {
    #[default]
    Raster,
    PathTraced,
}

/// What a `render` node says, resolved.
///
/// One answer for every host. The rule each field is read by is the registry's
/// own: the node's literal if it has one, else the type's declared default,
/// which is what makes a document saved before the node's second version render
/// with the current defaults rather than with zeroes. An engine of nothing
/// renders nothing and a bounce budget of zero ends every path before it
/// starts, so "or zero" would not be a degraded picture but no picture.
///
/// Deliberately neutral about rendering: no pixel formats, no backend types, no
/// sample chunking. It reports what was authored and leaves every host to map
/// it, which is what keeps renderer vocabulary out of the engine.
///
/// Not `Eq`: the clamp and the denoiser's tolerances are floats. Nothing
/// compares two of these whole, so the derive was only ever available rather
/// than used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    /// The camera the shot is taken through, or `None` to use whatever view
    /// the host would otherwise render. Never a path: references cross
    /// contexts by stable id so a rename cannot break one.
    pub camera: Option<crate::document::NodeId>,
    /// Already resolved: a size preset and its orientation have been applied,
    /// so a host renders these numbers and never learns that a preset list
    /// exists.
    pub width: u32,
    pub height: u32,
    pub engine: RenderEngine,
    /// Samples per pixel, from the quality preset or, when the preset is the
    /// exact one, from the count beside it.
    pub samples: u32,
    pub bounces: u32,
    pub transmissive_bounces: u32,
    /// The ceiling on a single sample's indirect contribution. Zero disables
    /// the clamp entirely, which is the kernel's own reading of the value
    /// rather than a convention invented here.
    pub firefly_clamp: f32,
    /// What the sampling sequence is drawn from. Fixed rather than
    /// time-varying, so a render repeats for a given scene, size, sample count
    /// and device on the surface that rendered it.
    pub seed: u32,
    pub denoise: bool,
    /// A multiplier on the denoiser's colour tolerance. One is the measured
    /// default; below one keeps grain and detail, above one is smoother and
    /// softer.
    pub denoise_strength: f32,
    /// The sample count past which denoising stops. Zero means it never stops.
    pub denoise_until_samples: u32,
    /// The four values that steer the filter. Defaults are the measured ones;
    /// see the denoiser for the sweeps behind them.
    pub denoise_sigma_color: f32,
    pub denoise_normal_power: f32,
    pub denoise_sigma_albedo: f32,
    pub denoise_level_falloff: f32,
    /// Whether the environment lights the scene without being photographed
    /// into the frame, leaving a matte behind it.
    pub transparent_background: bool,
    /// The auxiliary passes the run writes beside the image. Producing a pass
    /// and displaying one are separate choices; these are the production half.
    pub aov_albedo: bool,
    pub aov_normal: bool,
    pub aov_depth: bool,
}

impl RenderSettings {
    /// What a scene with no render node renders at.
    ///
    /// Read from the node type's own descriptor through the same resolver a
    /// real node goes through, so the two answers cannot drift. Both graphical
    /// shells and the headless command used to carry a hand-written copy of
    /// this, pinned to each other only by a test comparing values one at a
    /// time; a field added to one and forgotten in the other would have
    /// rendered differently depending on which shell you opened.
    #[must_use]
    pub fn defaults() -> Self {
        let desc = render_descriptor();
        // An empty store means every key falls to its spec default, which is
        // the resolver's own documented behaviour and the whole mechanism this
        // reads through.
        let resolved = crate::registry::resolve::resolve_params(
            &std::collections::BTreeMap::new(),
            &desc.params,
        )
        .unwrap_or_default();
        render_settings_from(&resolved)
    }
}

/// One `render` node's resolved params, read into the settings every host
/// receives.
///
/// The single reader. [`RenderSettings::defaults`] runs it over the
/// descriptor's defaults and `Engine::render_settings` runs it over a real
/// node, so a field added here reaches both by construction.
pub(crate) fn render_settings_from(p: &ResolvedParams) -> RenderSettings {
    let quality = p.enum_key("quality");
    let (width, height) = match resolution_preset_size(p.enum_key("resolution_preset")) {
        Some(size) => orient(size, p.enum_key("orientation")),
        // Custom, and anything a document names that this build has never
        // heard of: the authored numbers stand. A preset that cannot be
        // resolved must not silently resize somebody's shot.
        None => (p.u32("width"), p.u32("height")),
    };
    RenderSettings {
        camera: p.node_ref("camera_path"),
        width,
        height,
        engine: if p.enum_key("engine") == "traced" {
            RenderEngine::PathTraced
        } else {
            RenderEngine::Raster
        },
        samples: if quality == CUSTOM_QUALITY {
            p.u32("samples").max(1)
        } else {
            quality_samples(quality)
                .or_else(|| quality_samples(DEFAULT_QUALITY))
                .unwrap_or(64)
        },
        bounces: p.u32("bounces"),
        transmissive_bounces: p.u32("transmissive_bounces"),
        firefly_clamp: p.f32("firefly_clamp"),
        seed: p.u32("seed"),
        denoise: p.bool("denoise"),
        denoise_strength: p.f32("denoise_strength"),
        denoise_until_samples: p.u32("denoise_until_samples"),
        denoise_sigma_color: p.f32("denoise_sigma_color"),
        denoise_normal_power: p.f32("denoise_normal_power"),
        denoise_sigma_albedo: p.f32("denoise_sigma_albedo"),
        denoise_level_falloff: p.f32("denoise_level_falloff"),
        transparent_background: p.bool("transparent_background"),
        aov_albedo: p.bool("aov_albedo"),
        aov_normal: p.bool("aov_normal"),
        aov_depth: p.bool("aov_depth"),
    }
}

/// The sample count a quality preset means.
///
/// Beside the enum that declares the presets, so the two cannot drift, and
/// asserted against it by `every_quality_preset_has_a_sample_count`. It lived
/// in the frontend until this release, where nothing could check it against the
/// node and a preset added in Rust would have rendered at the fallback.
///
/// An unknown key answers `None`, and the caller substitutes the default
/// preset: the node stays authoritative and a host degrades rather than
/// refusing, which is what a registry-driven consumer does with anything it has
/// not been taught.
#[must_use]
pub fn quality_samples(key: &str) -> Option<u32> {
    match key {
        "draft" => Some(16),
        "good" => Some(64),
        "high" => Some(256),
        "reference" => Some(1024),
        _ => None,
    }
}

/// The preset every unknown key falls back to.
pub const DEFAULT_QUALITY: &str = "good";

/// The quality preset that takes its count from the `samples` param instead of
/// from the table above.
///
/// The preset steps are wide on purpose, because four times the samples is half
/// the noise. Someone who does not care should still meet four named choices;
/// someone who has found that their scene is clean at ninety should be able to
/// say ninety. Adding a variant preserves both, where retyping the param into a
/// number would have taken the four choices away.
pub const CUSTOM_QUALITY: &str = "custom";

/// The pixel size a named output size means, or `None` for the custom entry and
/// for any key this build does not know.
///
/// Beside the sample-count table so both kinds of preset resolve in the same
/// place, and in the engine rather than in a shell so every surface that renders
/// resolves a name to the same numbers.
///
/// Every pair is stated wide edge first. Orientation is then one rule applied to
/// all of them rather than a property each entry carries, which is what lets the
/// vertical delivery sizes fall out of the list instead of doubling it: the
/// story and reel size is this table's high definition entry in portrait.
///
/// Print sizes are stated in pixels with their density in the label. The
/// alternative is a dots-per-inch concept the node does not have, that nothing
/// in the renderer would read, and that would have to be persisted and migrated
/// for no gain.
#[must_use]
pub fn resolution_preset_size(key: &str) -> Option<(u32, u32)> {
    match key {
        "hd" => Some((1920, 1080)),
        "uhd_4k" => Some((3840, 2160)),
        "uhd_8k" => Some((7680, 4320)),
        "dci_2k" => Some((2048, 1080)),
        "dci_4k" => Some((4096, 2160)),
        "square" => Some((1080, 1080)),
        "social_5x4" => Some((1350, 1080)),
        "a4_300" => Some((3508, 2480)),
        "a3_300" => Some((4961, 3508)),
        "letter_300" => Some((3300, 2550)),
        _ => None,
    }
}

/// The output-size preset a document that has never heard of one resolves to.
///
/// Custom, and the reason is migration rather than taste: every existing
/// document carries an explicit width and height, and any other default would
/// open it with a preset selected that may not match them.
pub const DEFAULT_RESOLUTION_PRESET: &str = "custom";

/// A preset's stated pair turned the way round the orientation asks for.
///
/// Applied to the resolved pair rather than stored, so the table stays one
/// entry per size instead of one per size per orientation.
fn orient((a, b): (u32, u32), orientation: &str) -> (u32, u32) {
    let (wide, narrow) = (a.max(b), a.min(b));
    if orientation == "portrait" {
        (narrow, wide)
    } else {
        (wide, narrow)
    }
}

fn filename_param(default: &str) -> ParamSpec {
    ParamSpec::new(
        "filename",
        "File Name",
        "output",
        ParamType::Text,
        ParamValue::Text(default.to_string()),
    )
    .doc(
        "The base name for the saved file, without an extension -- the chosen \
         format supplies that. Left empty it falls back to 'export'.",
    )
}

fn save_action() -> ParamSpec {
    ParamSpec::new(
        "save",
        "Save to Disk",
        "output",
        ParamType::Action,
        ParamValue::Bool(false),
    )
    .doc(
        "Encodes what this node last cooked and hands it to the browser's \
         save dialog. It is a button, not a setting: nothing is stored, and \
         nothing is written until you press it and pick a destination. It \
         exports the current cooked result, so a node that has not cooked yet \
         reports that there is nothing to export rather than writing an empty \
         file.",
    )
}

#[must_use]
pub fn geo_export_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "geo_export",
        // v2 (0.8.0): materials and colors export; `include_materials`
        // arrives by pure registry default-fill, no hook needed.
        version: 2,
        display_name: "Export Geometry",
        category: Category::Export,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .carries_placements()
                .doc(
                    "The geometry to write out. It also leaves by the output \
                     port untouched, so this node taps a chain rather than \
                     ending it. Unconnected, there is nothing to export.",
                ),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Export Geometry",
            vec![
                ParamSpec::new(
                    "format",
                    "Format",
                    "output",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("obj", "Wavefront OBJ"),
                            EnumVariant::new("stl", "STL (binary)"),
                            EnumVariant::new("ply", "PLY (ascii)"),
                            EnumVariant::new("glb", "glTF Binary"),
                        ],
                    },
                    ParamValue::Enum("glb".to_string()),
                )
                .doc(
                    "Which writer encodes the file. glTF Binary is the \
                     default and keeps the most: one file with positions, \
                     normals, UVs, vertex colors, materials with embedded \
                     textures, and true point/line primitives. OBJ keeps \
                     geometry as text with `p`/`l`/`f` records; with \
                     materials it becomes an OBJ + MTL + textures archive. \
                     PLY merges every mesh into one vertex/face list, writes \
                     normals, UVs, and colors when every mesh has them, and \
                     a pure point cloud exports face-less. STL is the \
                     lossiest: triangle facets and nothing else, so point \
                     and line geometry is skipped.",
                ),
                ParamSpec::new(
                    "include_materials",
                    "Include Materials",
                    "output",
                    ParamType::Bool,
                    ParamValue::Bool(true),
                )
                .doc(
                    "Whether materials leave with the geometry. On, GLB \
                     embeds the material table with its textures and OBJ \
                     delivers the MTL sidecar archive. Off exports bare \
                     geometry in every format: smaller files, single-file \
                     OBJ, and no texture re-encoding.",
                ),
                filename_param("export"),
                save_action(),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Writes the geometry reaching it out to a file -- OBJ, STL, PLY, \
              or GLB -- and passes that same geometry on unchanged. The \
              output is the input by refcount, not a copy.\n\n\
              Because it passes through, it taps a chain rather than \
              terminating one: drop it partway down and the nodes after it \
              never notice. That makes it cheap to leave several in a network \
              at the points worth exporting, each with its own format and \
              name, and press whichever you need. Saving is a button, not a \
              side effect of cooking -- the node never writes a file on its \
              own.\n\n\
              Materials and vertex colors travel with the geometry: GLB \
              embeds the full material table with its textures and carries \
              colors as COLOR_0, OBJ with materials arrives as an \
              OBJ + MTL + textures archive, and PLY writes color properties. \
              Point clouds and polylines export as true point and line \
              primitives in GLB and OBJ and as face-less vertices in PLY; \
              STL is triangle facets only and skips them. Include Materials \
              turns the material side off for a bare-geometry file. The \
              button exports what the node last cooked, so it reports having \
              nothing to export rather than writing an empty file.",
        search_aliases: &[
            "export", "save", "file", "obj", "stl", "ply", "gltf", "glb", "rop",
        ],
        glyph: "geo_export",
        role: NodeRole::Terminal,
        cook: cook_passthrough_geometry,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_passthrough_geometry(
    _p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    // Refcount bump, not a copy: export is a chain tap.
    Ok(CookOutcome::Done(Outputs::single(
        "geometry",
        Value::Geometry(std::sync::Arc::clone(input)),
    )))
}

#[must_use]
pub fn image_export_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "image_export",
        version: 1,
        display_name: "Export Image",
        category: Category::Export,
        contexts: ContextSet::TEX,
        opens: None,
        inputs: vec![
            PortSpec::single("image", "Image", DataType::Image, true)
                .default_port()
                .doc(
                    "The image to write out. It also leaves by the output \
                     port untouched, so this node taps a texture chain rather \
                     than ending it. Unconnected, there is nothing to export.",
                ),
        ],
        outputs: vec![
            PortSpec::single("image", "Image", DataType::Image, false)
                .default_port()
                .doc("The input image, passed straight through."),
        ],
        params: params_with(
            "Export Image",
            vec![
                ParamSpec::new(
                    "format",
                    "Format",
                    "output",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("png", "PNG"),
                            EnumVariant::new("jpg", "JPEG"),
                        ],
                    },
                    ParamValue::Enum("png".to_string()),
                )
                .doc(
                    "PNG is lossless and keeps the alpha channel: the right \
                     default for a map that will drive a material. JPEG is \
                     lossy and has no alpha -- transparency is flattened away \
                     on write -- so keep it for reference images and previews \
                     rather than working textures.",
                ),
                ParamSpec::new(
                    "quality",
                    "Quality",
                    "output",
                    ParamType::Int,
                    ParamValue::Int(90),
                )
                .hard(1.0, 100.0)
                .step(1.0)
                .show_if("format", Pred::Eq(ParamValue::Enum("jpg".to_string())))
                .doc(
                    "The JPEG quality factor, 1 to 100: higher is a bigger \
                     file with fewer compression artifacts. Only read when \
                     Format is JPEG, and hidden otherwise, since PNG has no \
                     equivalent knob.",
                ),
                filename_param("texture"),
                save_action(),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        doc: "Writes the image reaching it out to a PNG or JPEG file, and \
              passes that same image on unchanged. The counterpart to \
              `geo_export`, for texture networks.\n\n\
              It taps a chain rather than terminating one, so it drops in \
              partway down a texture network without disturbing what follows: \
              leave one at each stage worth baking out. Saving is a button, \
              never a side effect of cooking, so a texture network does not \
              litter your disk while you tweak it.\n\n\
              It exports at whatever resolution the chain cooked at, which is \
              the working resolution and not necessarily the one you want on \
              disk -- if you need a specific size, set it upstream. Choosing \
              JPEG discards the alpha channel outright, flattening \
              transparency, which matters when the map you are baking uses it.",
        search_aliases: &["export", "save", "file", "png", "jpeg", "image"],
        glyph: "image_export",
        role: NodeRole::Terminal,
        cook: cook_passthrough_image,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook_passthrough_image(
    _p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.image("image") else {
        return Ok(CookOutcome::Done(Outputs::default()));
    };
    Ok(CookOutcome::Done(Outputs::single(
        "image",
        Value::Image(std::sync::Arc::clone(input)),
    )))
}

#[must_use]
pub fn render_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "render",
        // v3 (0.9.0) carries this release's whole render-control surface in one
        // bump: an exact sample count beside the presets, the tracer's clamp
        // and its seed, the denoiser's strength, stopping point and four
        // steering values, named output sizes with an orientation, and the
        // film-back and auxiliary-pass options that the transparent-background
        // and cross-shell render work read.
        //
        // One bump rather than three. Three separate bodies of work add
        // parameters to this node in this release, and every bump is a
        // compatibility event whether or not anything needs migrating, so
        // landing them apart would mean three migrations, three registry
        // snapshots and three catalog amendments for one release. The
        // consequence is deliberate: some of these are read by nothing until
        // later in the release, and a parameter that is present and inert is
        // the cheaper half of that trade.
        //
        // Still a pure addition, like the camera node's v2 and v3: nothing
        // changes type, default or meaning, a parameter a document has never
        // heard of resolves to its descriptor default, so there is nothing for
        // a migration to do and the round-trip test is what says so.
        //
        // v2 added the engine choice, the quality preset, the bounce budgets
        // and the denoise toggle, and raised the resolution clamp.
        version: 3,
        display_name: "Render",
        category: Category::Export,
        contexts: ContextSet::OBJ,
        opens: None,
        inputs: vec![],
        outputs: vec![],
        params: params_with(
            "Render",
            vec![
                ParamSpec::new(
                    "camera_path",
                    "Camera",
                    "render",
                    ParamType::NodePath {
                        accept: NodePathAccept::TypeIs("camera".to_string()),
                    },
                    ParamValue::NodeRef(None),
                )
                .doc(
                    "Which camera to render through, picked by path from the \
                     cameras in the scene -- a reference, not a wire. Left \
                     unset the render comes from the current viewport view, \
                     wherever you last orbited it to, which is convenient but \
                     not repeatable. Point it at a camera to pin the shot \
                     down.",
                ),
                ParamSpec::new(
                    "resolution_preset",
                    "Output Size",
                    "render",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("custom", "Custom"),
                            EnumVariant::new("hd", "HD (1920 x 1080)"),
                            EnumVariant::new("uhd_4k", "UHD 4K (3840 x 2160)"),
                            EnumVariant::new("uhd_8k", "UHD 8K (7680 x 4320)"),
                            EnumVariant::new("dci_2k", "DCI 2K (2048 x 1080)"),
                            EnumVariant::new("dci_4k", "DCI 4K (4096 x 2160)"),
                            EnumVariant::new("square", "Square (1080 x 1080)"),
                            EnumVariant::new("social_5x4", "Social 5:4 (1350 x 1080)"),
                            EnumVariant::new("a4_300", "A4 at 300 dpi (3508 x 2480)"),
                            EnumVariant::new("a3_300", "A3 at 300 dpi (4961 x 3508)"),
                            EnumVariant::new("letter_300", "Letter at 300 dpi (3300 x 2550)"),
                        ],
                    },
                    ParamValue::Enum(DEFAULT_RESOLUTION_PRESET.to_string()),
                )
                .doc(
                    "The output size, chosen by name. Custom is the default \
                     and reveals Width and Height for a size that is not on \
                     the list.\n\n\
                     Choosing a preset sets the size rather than describing \
                     it. Width and Height together fix the aspect the camera \
                     frames at, so a preset changes the composition, not just \
                     the file size.\n\n\
                     Every entry states its pixel size, and the print entries \
                     state the density that size assumes, so nothing here \
                     depends on a dots-per-inch setting the renderer would \
                     ignore. Sizes are listed wide edge first; Orientation \
                     turns them.",
                ),
                ParamSpec::new(
                    "orientation",
                    "Orientation",
                    "render",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("landscape", "Landscape"),
                            EnumVariant::new("portrait", "Portrait"),
                        ],
                    },
                    ParamValue::Enum("landscape".to_string()),
                )
                .show_if(
                    "resolution_preset",
                    Pred::Neq(ParamValue::Enum(DEFAULT_RESOLUTION_PRESET.to_string())),
                )
                .doc(
                    "Which way round the chosen size is: Landscape puts the \
                     wide edge across, Portrait puts it up.\n\n\
                     It turns the size, and the camera frames to whatever \
                     aspect that gives it, so switching orientation reframes \
                     what is in view rather than only rotating the file.\n\n\
                     The vertical delivery sizes come from here rather than \
                     from entries of their own, which is what keeps the list \
                     short: HD in Portrait is 1080 x 1920, the story and reel \
                     size, and A4 in Portrait is 2480 x 3508.",
                ),
                ParamSpec::new(
                    "width",
                    "Width",
                    "render",
                    ParamType::Int,
                    ParamValue::Int(1920),
                )
                .hard(16.0, 8192.0)
                .step(1.0)
                .show_if(
                    "resolution_preset",
                    Pred::Eq(ParamValue::Enum(DEFAULT_RESOLUTION_PRESET.to_string())),
                )
                .doc(
                    "Output width in pixels. Together with Height it also \
                     fixes the aspect ratio the camera frames at, so changing \
                     it changes the composition, not just the file size.\n\n\
                     Read only when Output Size is Custom, and hidden \
                     otherwise, since a preset states its own size in its \
                     name.",
                ),
                ParamSpec::new(
                    "height",
                    "Height",
                    "render",
                    ParamType::Int,
                    ParamValue::Int(1080),
                )
                .hard(16.0, 8192.0)
                .step(1.0)
                .show_if(
                    "resolution_preset",
                    Pred::Eq(ParamValue::Enum(DEFAULT_RESOLUTION_PRESET.to_string())),
                )
                .doc(
                    "Output height in pixels. Large renders are drawn in \
                     tiles, each inside the four-megapixel budget a browser \
                     reliably survives, and assembled afterwards -- so the \
                     size you ask for is the size you get, however long it \
                     takes.\n\n\
                     Read only when Output Size is Custom, and hidden \
                     otherwise.",
                ),
                ParamSpec::new(
                    "engine",
                    "Engine",
                    "render",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("raster", "Rasterized"),
                            EnumVariant::new("traced", "Path traced"),
                        ],
                    },
                    ParamValue::Enum("raster".to_string()),
                )
                .doc(
                    "Which renderer draws the still. Rasterized is the \
                     viewport's own renderer: fast, and its shadows, ambient \
                     occlusion and reflections are approximations. Path traced \
                     follows light through the scene instead, so shadows, \
                     bounced colour and soft reflections come out of the same \
                     calculation rather than being added on top -- and it \
                     takes as long as it takes.\n\n\
                     A traced still shows what the tracer integrates: the \
                     environment where a ray leaves the scene, and no grid, \
                     gizmo or overlay. It is a photograph of the scene rather \
                     than a screenshot of the viewport.",
                ),
                ParamSpec::new(
                    "render",
                    "Render Still",
                    "render",
                    ParamType::Action,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Renders the still and opens a dialog showing it arrive, \
                     tile by tile, with a running count and a cancel. Nothing \
                     is written until you save from there.\n\n\
                     The viewport is left where it is. The shot comes from the \
                     camera above, not from what you happen to be looking at, \
                     so pressing this never moves your view.",
                ),
                ParamSpec::new(
                    "quality",
                    "Quality",
                    "quality",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("draft", "Draft (16 samples)"),
                            EnumVariant::new("good", "Good (64 samples)"),
                            EnumVariant::new("high", "High (256 samples)"),
                            EnumVariant::new("reference", "Reference (1024 samples)"),
                            EnumVariant::new("custom", "Exact count"),
                        ],
                    },
                    ParamValue::Enum(DEFAULT_QUALITY.to_string()),
                )
                .doc(
                    "How many samples each pixel averages, and so how much \
                     grain is left. Four times the samples is half the noise, \
                     not a quarter, which is why the steps are wide: Draft to \
                     Good is a visible improvement and Draft to Reference is \
                     sixty-four times the wait.\n\n\
                     Exact count reveals a Samples field for a scene that is \
                     not where the presets are. The wide steps are worth \
                     keeping for everything else: most shots want one of four \
                     answers, not a number to pick.\n\n\
                     Path traced only. A rasterized still draws each pixel \
                     once.",
                ),
                ParamSpec::new(
                    "samples",
                    "Samples",
                    "quality",
                    ParamType::Int,
                    ParamValue::Int(64),
                )
                .hard(1.0, 8192.0)
                .soft(1.0, 1024.0)
                .step(1.0)
                .show_if(
                    "quality",
                    Pred::Eq(ParamValue::Enum(CUSTOM_QUALITY.to_string())),
                )
                .doc(
                    "The exact number of samples each pixel averages. Read \
                     only when Quality is Exact count, and hidden otherwise, \
                     since the named presets carry their own counts.\n\n\
                     Reach for it when a preset is not where your scene is: a \
                     shot that is clean at ninety would otherwise mean either \
                     shipping the grain at sixty-four or waiting four times as \
                     long for two hundred and fifty-six.\n\n\
                     Path traced only.",
                ),
                ParamSpec::new(
                    "bounces",
                    "Bounces",
                    "quality",
                    ParamType::Int,
                    ParamValue::Int(6),
                )
                .hard(1.0, 32.0)
                .step(1.0)
                .doc(
                    "How many times light may scatter before a path is given \
                     up on. Higher opens up interiors and deep folds, where \
                     most of the light arrives after several bounces; an \
                     exterior on a bright day is usually finished by four.\n\n\
                     Path traced only.",
                ),
                ParamSpec::new(
                    "transmissive_bounces",
                    "Glass Bounces",
                    "quality",
                    ParamType::Int,
                    ParamValue::Int(4),
                )
                .hard(0.0, 32.0)
                .step(1.0)
                .doc(
                    "How many of the bounces above may additionally pass \
                     through transmissive surfaces. Counted separately so a \
                     pane of glass does not spend a whole path's budget \
                     getting through it: a window is two surfaces, a tumbler \
                     is four, and running out ends the path rather than \
                     turning the glass opaque.\n\n\
                     Path traced only.",
                ),
                ParamSpec::new(
                    "firefly_clamp",
                    "Bright Sample Limit",
                    "quality",
                    ParamType::Float,
                    ParamValue::Float(16.0),
                )
                .hard(0.0, 1000.0)
                .soft(0.0, 64.0)
                .doc(
                    "A ceiling on how much light one sample may contribute \
                     after it has bounced. A rare path that finds a bright \
                     source through a mirror or a tight caustic comes back \
                     hundreds of times the average, and a single one of those \
                     leaves a lone bright pixel that thousands of ordinary \
                     samples cannot average away.\n\n\
                     What it costs is energy. Clamping discards the part above \
                     the ceiling rather than redistributing it, so the image \
                     gets darker exactly where the clamp acts, and a scene lit \
                     mostly through those rare paths gets darker overall. \
                     Lower it to suppress more of them, raise it to let \
                     brighter contributions through, and set it to zero to \
                     turn the clamp off and keep every last one.\n\n\
                     Path traced only.",
                ),
                ParamSpec::new(
                    "seed",
                    "Seed",
                    "quality",
                    ParamType::Int,
                    ParamValue::Int(2_654_435_769),
                )
                .hard(0.0, 4_294_967_295.0)
                .step(1.0)
                .doc(
                    "What the sampling sequence is drawn from. The same seed \
                     gives the same image for the same scene, size and sample \
                     count on the same device, which is what makes a \
                     comparison between two settings a comparison rather than \
                     two different grain patterns.\n\n\
                     The promise stops at the surface that rendered it. The \
                     browser and the command line accumulate in different \
                     chunk sizes, each for a reason sound on its own surface, \
                     and floating-point addition is not invariant to the \
                     grouping, so the same seed does not give the same bytes \
                     across them.\n\n\
                     Changing it changes the grain and not the answer: two \
                     seeds at a high sample count converge to the same image.\n\n\
                     Path traced only.",
                ),
                ParamSpec::new(
                    "denoise",
                    "Denoise",
                    "denoise",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Smooths the remaining grain, steered by what each pixel's \
                     surface looks like so material boundaries survive.\n\n\
                     Off by default, and that is the right default for a \
                     finished still: at a high sample count there is little \
                     grain left to remove and a filter can only take detail \
                     away. Turn it on for a Draft, where the grain is the \
                     thing standing between you and seeing the shot.\n\n\
                     This is the still's own setting. The viewport's traced \
                     preview keeps a separate one, in preferences, because a \
                     preview and a delivered frame want different answers.",
                ),
                ParamSpec::new(
                    "denoise_strength",
                    "Strength",
                    "denoise",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .hard(0.0, 4.0)
                .soft(0.0, 2.0)
                .show_if("denoise", Pred::Truthy)
                .doc(
                    "How hard the filter works, as a multiple of the colour \
                     tolerance it was measured at. One is that measured \
                     setting.\n\n\
                     Below one the image keeps more grain and more detail, and \
                     material boundaries stay crisp. Above one it is smoother \
                     and softer, and fine texture starts to go with the noise. \
                     It steers the value that most changes the outcome rather \
                     than being a fifth independent number, so Colour \
                     Tolerance under Advanced remains the thing this \
                     multiplies.",
                ),
                ParamSpec::new(
                    "denoise_until_samples",
                    "Stop After",
                    "denoise",
                    ParamType::Int,
                    ParamValue::Int(0),
                )
                .hard(0.0, 8192.0)
                .step(1.0)
                .show_if("denoise", Pred::Truthy)
                .doc(
                    "The sample count past which the filter stops. Zero means \
                     it never stops.\n\n\
                     A still that starts noisy and converges does not need at \
                     the end the filtering it needed at the start, and a \
                     converged image still being smoothed is losing detail for \
                     grain that is no longer there. Set this where your scene \
                     stops looking noisy and the filter steps out of the way \
                     after it.\n\n\
                     The filter already relaxes on its own as a render \
                     converges, because its colour tolerance is divided by the \
                     square root of the sample count. This sharpens a \
                     behaviour that exists rather than introducing one.",
                ),
                ParamSpec::new(
                    "denoise_sigma_color",
                    "Colour Tolerance",
                    "denoise",
                    ParamType::Float,
                    ParamValue::Float(1.2),
                )
                .hard(0.01, 10.0)
                .soft(0.1, 4.0)
                .subgroup("Advanced")
                .show_if("denoise", Pred::Truthy)
                .doc(
                    "How different in brightness two neighbouring pixels may \
                     be and still be averaged together. Larger reaches across \
                     those differences and removes more noise; smaller keeps \
                     detail and leaves more grain.\n\n\
                     The default is measured rather than chosen: it came from \
                     an error sweep against a reference, scored both for error \
                     and for how much of a material step survived. Strength \
                     above multiplies this value.\n\n\
                     Divided by the square root of the sample count inside the \
                     filter, so it tightens on its own as a render converges.",
                ),
                ParamSpec::new(
                    "denoise_normal_power",
                    "Normal Sharpness",
                    "denoise",
                    ParamType::Float,
                    ParamValue::Float(128.0),
                )
                .hard(1.0, 1024.0)
                .soft(8.0, 256.0)
                .subgroup("Advanced")
                .show_if("denoise", Pred::Truthy)
                .doc(
                    "How closely two pixels' surface directions must agree \
                     before they are averaged together. The default \
                     corresponds to about ten degrees.\n\n\
                     Higher keeps creases and curvature crisp and filters less \
                     across them; lower lets the filter reach around a curve, \
                     which is smoother and takes the edge off a bevel. This is \
                     the value that stops geometry melting.",
                ),
                ParamSpec::new(
                    "denoise_sigma_albedo",
                    "Albedo Tolerance",
                    "denoise",
                    ParamType::Float,
                    ParamValue::Float(0.08),
                )
                .hard(0.001, 1.0)
                .soft(0.01, 0.5)
                .subgroup("Advanced")
                .show_if("denoise", Pred::Truthy)
                .doc(
                    "How different two pixels' base colours may be before the \
                     filter treats them as different materials. Smaller keeps \
                     the boundary between two materials sharp; larger lets one \
                     bleed into the next.\n\n\
                     Much tighter than the colour tolerance on purpose: base \
                     colour is noise-free where brightness is not, so it is \
                     the most reliable thing the filter has to steer by.",
                ),
                ParamSpec::new(
                    "denoise_level_falloff",
                    "Scale Falloff",
                    "denoise",
                    ParamType::Float,
                    ParamValue::Float(2.0),
                )
                .hard(1.0, 8.0)
                .soft(1.0, 4.0)
                .subgroup("Advanced")
                .show_if("denoise", Pred::Truthy)
                .doc(
                    "How much the tolerances tighten at each coarser pass. The \
                     filter runs at five scales and each one divides its \
                     tolerances by this number.\n\n\
                     Higher makes the coarse passes conservative, keeping \
                     large-scale detail and removing less of the broad \
                     blotching; lower lets them reach further and flattens \
                     wide areas.",
                ),
                ParamSpec::new(
                    "transparent_background",
                    "Transparent Background",
                    "output",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Renders with nothing behind the subject. The environment \
                     still lights the scene exactly as it did, but it is not \
                     photographed into the frame, and what comes out carries a \
                     matte: opaque where the camera found a surface, clear \
                     where it found sky, and fractional along every \
                     silhouette.\n\n\
                     That is what makes a render an element rather than a \
                     picture. The alternative is rendering against a colour \
                     and keying it by hand, which fails the moment the subject \
                     is glossy, because the background is in its reflections.",
                ),
                ParamSpec::new(
                    "aov_albedo",
                    "Albedo Pass",
                    "output",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Writes the base colour each pixel saw as a file beside \
                     the image: surface colour before any lighting, which is \
                     what a compositor re-grades or relights against.\n\n\
                     Producing a pass and displaying one are separate choices. \
                     This asks for the file; which pass the render window \
                     shows is chosen there, while it converges.\n\n\
                     Path traced only. A rasterized still writes no auxiliary \
                     passes.",
                ),
                ParamSpec::new(
                    "aov_normal",
                    "Normal Pass",
                    "output",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Writes the surface direction each pixel saw as a file \
                     beside the image, encoded as colour. It is what a \
                     compositor relights with, and it is also what the \
                     denoiser steers by, so it is the pass to look at when a \
                     denoised result has lost an edge it should have kept.\n\n\
                     Path traced only.",
                ),
                ParamSpec::new(
                    "aov_depth",
                    "Depth Pass",
                    "output",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Writes how far away each pixel is as a file beside the \
                     image. It is what depth of field, fog and atmospheric \
                     grading are built from in a compositor, and what tells \
                     you whether the shot has the depth separation you thought \
                     it had.\n\n\
                     Path traced only.",
                ),
            ],
        ),
        bypass: BypassBehavior::NotBypassable,
        doc: "Holds a render setup: which camera to shoot through, at what \
              resolution, with which renderer and how much patience. It \
              carries settings and nothing else -- no ports, no cook, no \
              output. Pressing Render Still renders it and opens a dialog \
              showing it arrive, where you review the frame and choose whether \
              to save it.\n\n\
              It lives at the object level beside the cameras and lights it \
              refers to, and it is how a shot stops being something you \
              re-find by orbiting. Point it at a `camera` node and the same \
              framing comes back every session; keep several around, one per \
              shot, each named for what it captures.\n\n\
              Output size is either a named delivery size or your own two \
              numbers, and an orientation turns a preset without retyping it. \
              Up to 8192 pixels an edge. Anything larger than a browser draws \
              in one pass is rendered in tiles and assembled, so the size you \
              ask for is the size you get -- what changes with resolution is \
              how long it takes, not whether it works.\n\n\
              The tabs are the decisions rather than the fields: Render is the \
              shot, Quality is how long you are willing to wait and what the \
              tracer is allowed to do while it waits, Denoise is what happens \
              to the grain that is left, and Output is what leaves besides the \
              picture. Everything under Quality and Denoise is path traced \
              only; a rasterized still draws each pixel once.\n\n\
              Depth of field belongs to the camera, not here: aperture, focus \
              distance and blade count live on the `camera` node this points \
              at, so the same lens applies wherever that camera is used.",
        search_aliases: &["render", "rop", "output", "capture", "screenshot"],
        glyph: "render",
        role: NodeRole::Terminal,
        cook: passive_cook,
        migrate: None,
    }
}
