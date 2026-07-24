//! The `note` annotation: a canvas
//! comment. No ports, no cook, `NotBypassable`, and excluded from
//! auto-layout (a frontend concern).

use super::common::{general_params, passive_cook};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
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
        ParamSpec::new(
            "text_size",
            "Text Size",
            "note",
            ParamType::Enum {
                variants: vec![
                    EnumVariant::new("small", "Small"),
                    EnumVariant::new("medium", "Medium"),
                    EnumVariant::new("large", "Large"),
                ],
            },
            ParamValue::Enum("small".into()),
        )
        .doc(
            "The note text's size on the canvas. Small keeps annotations \
             quieter than the node labels around them and is the default; \
             Medium matches the pre-0.8.0 look; Large is for the one heading \
             a network deserves. Notes saved before this option exists open \
             as Small, the new default.",
        ),
    ]);

    NodeTypeDescriptor {
        type_id: "note",
        // v2 (0.8.0 Stage 8): added `text_size`. Purely additive, so no
        // migration hook: a v1 note loads with the param unset and resolves
        // to the descriptor default.
        version: 2,
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

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use crate::document::NodeId;
    use crate::migration::load_node;
    use crate::nodes::builtin_registry;
    use crate::params::{ParamSource, ParamValue};
    use crate::registry::resolve::resolve_params;

    /// The v1 -> v2 contract: `text_size` is purely additive, so a
    /// pre-0.8.0 note (every bundled sample scene has them) must load with
    /// ZERO warnings and resolve the new param to the Small default.
    #[test]
    fn a_v1_note_loads_clean_and_defaults_to_small_text() {
        let reg = builtin_registry().unwrap();
        let mut raw = Map::new();
        raw.insert("text".to_string(), serde_json::json!("older note"));
        raw.insert("width".to_string(), serde_json::json!(290.0));
        let loaded = load_node(&reg, NodeId(1), "note", 1, raw, [0.0; 2], false);
        assert!(loaded.node.placeholder.is_none());
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.node.type_version, 2);
        // The param is honestly unset in the document...
        assert!(!loaded.node.params.contains_key("text_size"));
        // ...and the resolver fills the Small default.
        let desc = reg.get("note").unwrap();
        let resolved = resolve_params(&loaded.node.params, &desc.params).unwrap();
        assert_eq!(resolved.enum_key("text_size"), "small");
        assert_eq!(
            loaded.node.params.get("text"),
            Some(&ParamSource::Literal(ParamValue::Text("older note".into())))
        );
    }

    /// A note from a future build refuses to cook rather than lose data.
    #[test]
    fn a_future_note_loads_as_placeholder() {
        let reg = builtin_registry().unwrap();
        let loaded = load_node(&reg, NodeId(1), "note", 3, Map::new(), [0.0; 2], false);
        assert!(loaded.node.placeholder.is_some());
        assert_eq!(loaded.node.type_version, 3);
    }
}
