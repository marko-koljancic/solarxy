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
        version: 1,
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
                    "width",
                    "Width",
                    "render",
                    ParamType::Int,
                    ParamValue::Int(1920),
                )
                .hard(16.0, 4096.0)
                .step(1.0)
                .doc(
                    "Output width in pixels. Together with Height it also \
                     fixes the aspect ratio the camera frames at, so changing \
                     it changes the composition, not just the file size.",
                ),
                ParamSpec::new(
                    "height",
                    "Height",
                    "render",
                    ParamType::Int,
                    ParamValue::Int(1080),
                )
                .hard(16.0, 4096.0)
                .step(1.0)
                .doc(
                    "Output height in pixels. Note that width times height is \
                     capped at 4 megapixels on the way to the GPU: ask for \
                     more and the capture is scaled down to fit, keeping the \
                     aspect ratio, and reports the size it actually produced.",
                ),
                ParamSpec::new(
                    "render",
                    "Render",
                    "render",
                    ParamType::Action,
                    ParamValue::Bool(false),
                )
                .doc(
                    "Jumps the active viewport to the chosen camera and opens \
                     the screenshot dialog at this resolution. Nothing is \
                     written until you save from there.",
                ),
            ],
        ),
        bypass: BypassBehavior::NotBypassable,
        doc: "Holds a render setup: which camera to shoot through and at what \
              resolution. It carries settings and nothing else -- no ports, \
              no cook, no output. Pressing Render jumps the active viewport to \
              the chosen camera and opens the screenshot dialog at this \
              resolution, where you review the frame and choose whether to \
              save it.\n\n\
              It lives at the object level beside the cameras and lights it \
              refers to, and it is how a shot stops being something you \
              re-find by orbiting. Point it at a `camera` node and the same \
              framing comes back every session; keep several around, one per \
              shot, each named for what it captures.\n\n\
              A capture is capped at 4 megapixels. The width and height go up \
              to 4096 each, but anything past the cap is scaled down to fit, \
              preserving aspect -- so asking for 3840x2160 does not get you a \
              4K frame, and the result tells you the size it really made. The \
              limit is deliberate: larger captures can lose the WebGPU device \
              outright, and there is no recovery from that on the web yet.",
        search_aliases: &["render", "rop", "output", "capture", "screenshot"],
        glyph: "render",
        role: NodeRole::Terminal,
        cook: passive_cook,
        migrate: None,
    }
}
