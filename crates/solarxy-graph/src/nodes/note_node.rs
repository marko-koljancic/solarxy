//! The `note` annotation (node catalog part II, section 12): a canvas
//! comment. No ports, no cook, `NotBypassable`, and excluded from
//! auto-layout (a frontend concern).

use super::common::{general_params, passive_cook};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::{BypassBehavior, Category, ContextMask, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    // Note carries only the general group plus its note group (no rendering
    // group: it produces no geometry).
    let mut params = general_params("Note");
    params.extend(vec![
        ParamSpec::new(
            "text",
            "Text",
            "note",
            ParamType::Text,
            ParamValue::Text(String::new()),
        ),
        ParamSpec::new(
            "color",
            "Color",
            "note",
            ParamType::Color,
            // #FDE68A (amber), linear-ish stored as sRGB bytes / 255.
            ParamValue::Color([0.992, 0.902, 0.541, 1.0]),
        ),
        ParamSpec::new(
            "width",
            "Width",
            "note",
            ParamType::Float,
            ParamValue::Float(160.0),
        )
        .hard(120.0, 800.0),
        ParamSpec::new(
            "height",
            "Height",
            "note",
            ParamType::Float,
            ParamValue::Float(80.0),
        )
        .hard(60.0, 600.0),
    ]);

    NodeTypeDescriptor {
        type_id: "note",
        version: 1,
        display_name: "Note",
        category: Category::Utility,
        contexts: ContextMask::BOTH,
        inputs: vec![],
        outputs: vec![],
        params,
        bypass: BypassBehavior::NotBypassable,
        doc: "A canvas annotation. Double-click to edit; resizable.",
        search_aliases: &["comment", "annotation", "sticky", "label"],
        glyph: "note",
        role: NodeRole::Note,
        cook: passive_cook,
        migrate: None,
    }
}
