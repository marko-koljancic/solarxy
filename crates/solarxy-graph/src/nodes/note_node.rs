//! The `note` annotation: a canvas
//! comment. No ports, no cook, `NotBypassable`, and excluded from
//! auto-layout (a frontend concern).

use super::common::{general_params, passive_cook};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

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
        )
        .doc(
            "The note's body text, the whole point of the node. Edit it in \
             place by double-clicking the note on the canvas rather than \
             through this field. Empty by default: a new note is a blank \
             sticky waiting to be typed into.",
        ),
        ParamSpec::new(
            "color",
            "Color",
            "note",
            ParamType::Color,
            // #FDE68A (amber), linear-ish stored as sRGB bytes / 255.
            ParamValue::Color([0.992, 0.902, 0.541, 1.0]),
        )
        .doc(
            "The sticky's fill colour, amber by default. The note's corner \
             swatch cycles a set of pastels; this field takes any colour. \
             Nothing but the canvas reads it, so it is free to carry whatever \
             convention you like -- one colour for TODOs, another for \
             warnings to the next person in the file.",
        ),
        ParamSpec::new(
            "width",
            "Width",
            "note",
            ParamType::Float,
            ParamValue::Float(160.0),
        )
        .hard(120.0, 800.0)
        .doc(
            "The sticky's width on the canvas, in canvas units, which are \
             screen pixels at 100% zoom. Usually set by dragging the note's \
             corner rather than typed here. Text wraps to it.",
        ),
        ParamSpec::new(
            "height",
            "Height",
            "note",
            ParamType::Float,
            ParamValue::Float(80.0),
        )
        .hard(60.0, 600.0)
        .doc(
            "The sticky's height on the canvas, in the same units as Width, \
             and likewise usually dragged rather than typed. It does not grow \
             to fit: text longer than the box is clipped, so size the note to \
             its contents.",
        ),
    ]);

    NodeTypeDescriptor {
        type_id: "note",
        version: 1,
        display_name: "Note",
        category: Category::Utility,
        contexts: ContextSet::ALL,
        opens: None,
        inputs: vec![],
        outputs: vec![],
        params,
        bypass: BypassBehavior::NotBypassable,
        doc: "A sticky note on the canvas. It has no ports, does not cook, and \
              produces nothing -- it exists to be read by whoever opens the \
              file next, including you in six months.\n\n\
              Use it to leave the reasoning that the graph itself cannot \
              carry: why this branch is bypassed, what the magic number in \
              that param came from, which half of the network is still a work \
              in progress. It is the one node allowed in every network kind, \
              object through texture, because every network eventually needs a \
              comment. Double-click to edit (Esc reverts, Ctrl+Enter or \
              clicking away commits) and drag its corner to resize.\n\n\
              It cannot be bypassed, since there is nothing to switch off, and \
              it is the single node the frontend draws with a bespoke \
              component instead of the standard registry-driven one -- a note \
              is a sticky, not a box with ports. Its edits are ordinary param \
              commands underneath, so notes undo, redo, and save into a \
              `.slxy` like any other node.",
        search_aliases: &["comment", "annotation", "sticky", "label"],
        glyph: "note",
        role: NodeRole::Note,
        cook: passive_cook,
        migrate: None,
    }
}
