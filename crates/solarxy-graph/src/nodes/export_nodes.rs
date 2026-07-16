//! The output nodes (context-expansion phase 21, decisions C-7 and C-8):
//! `geo_export` (Geo), `image_export` (Tex), and the root `render`
//! configuration node.
//!
//! Export nodes are save-only and PASS THROUGH: they sit in a chain
//! without changing it, and their Save button (a `ParamType::Action`)
//! routes through `Engine::invoke_action`, which encodes the node's
//! committed output via `solarxy-formats`' writers and hands bytes to the
//! host for saving. The `render` node is a config carrier (camera by
//! path, resolution); its Render button is HOST-interpreted (the engine
//! cannot render), driving the existing screenshot path.

use super::common::{geometry_output, params_with, passive_cook};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::document::ContextKind;
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
    .doc("Without extension; the format decides it.")
}

fn save_action() -> ParamSpec {
    ParamSpec::new(
        "save",
        "Save to Disk",
        "output",
        ParamType::Action,
        ParamValue::Bool(false),
    )
}

#[must_use]
pub fn geo_export_descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "geo_export",
        version: 1,
        display_name: "Export Geometry",
        category: Category::Utility,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true).default_port(),
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
                ),
                filename_param("export"),
                save_action(),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Saves the input geometry to disk (obj, stl, ply, glb); passes it through unchanged. Materials do not export yet.",
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
        category: Category::Utility,
        contexts: ContextSet::TEX,
        opens: None,
        inputs: vec![PortSpec::single("image", "Image", DataType::Image, true).default_port()],
        outputs: vec![PortSpec::single("image", "Image", DataType::Image, false).default_port()],
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
                .show_if("format", Pred::Eq(ParamValue::Enum("jpg".to_string()))),
                filename_param("texture"),
                save_action(),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "image".to_string(),
        },
        doc: "Saves the input image to disk (png, jpg); passes it through unchanged. Exports at the cooked working resolution.",
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
        category: Category::Utility,
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
                .doc("Unset renders from the current view."),
                ParamSpec::new(
                    "width",
                    "Width",
                    "render",
                    ParamType::Int,
                    ParamValue::Int(1920),
                )
                .hard(16.0, 4096.0)
                .step(1.0),
                ParamSpec::new(
                    "height",
                    "Height",
                    "render",
                    ParamType::Int,
                    ParamValue::Int(1080),
                )
                .hard(16.0, 4096.0)
                .step(1.0),
                ParamSpec::new(
                    "render",
                    "Render",
                    "render",
                    ParamType::Action,
                    ParamValue::Bool(false),
                ),
            ],
        ),
        bypass: BypassBehavior::NotBypassable,
        doc: "Render configuration: a camera (by path; unset uses the current view) and a resolution. The Render button captures through the existing screenshot path; the same settings feed the turntable export.",
        search_aliases: &["render", "rop", "output", "capture", "screenshot"],
        glyph: "render",
        role: NodeRole::Terminal,
        cook: passive_cook,
        migrate: None,
    }
}
