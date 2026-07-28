//! The `text` datablock: a named snippet of text or code stored in the
//! scene.
//!
//! A node rather than a new document section, deliberately. Persistence,
//! undo, copy/paste and `.slxy` round-tripping already work for every param
//! on every node, so a `Snippet` param IS a stored, undoable, saved text
//! buffer with nothing new written. A `text` section beside `review` would
//! have been roughly ten files reimplementing exactly that.
//!
//! The cost of the choice is that a snippet appears on the canvas like any
//! other node. That is answered by making it small and quiet
//! ([`NodeRole::Text`]) rather than by hiding it: a thing the scene carries
//! should be findable in the scene, and being selectable is what makes
//! delete and duplicate work without inventing panel-only commands for them.
//!
//! It computes nothing. `passive_cook` and no ports: a datablock is storage,
//! and wiring one into a graph would imply it participates in a cook.

use super::common::{general_params, passive_cook};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    let mut params = general_params("Text");
    params.extend(vec![
        ParamSpec::new(
            "body",
            "Body",
            "text",
            ParamType::Snippet,
            ParamValue::Text(String::new()),
        )
        .doc(
            "The snippet itself. Edited in the Text panel or here; either \
             way it is one parameter on one node, so it saves with the \
             scene and undoes in a single step.",
        ),
        ParamSpec::new(
            "language",
            "Language",
            "text",
            ParamType::Enum {
                variants: vec![
                    EnumVariant::new("plain", "Plain"),
                    EnumVariant::new("wrangle", "Wrangle"),
                ],
            },
            ParamValue::Enum("plain".into()),
        )
        .doc(
            "How the editor presents the snippet. **Wrangle** turns on \
             syntax highlighting, completions and bracket handling; \
             **Plain** leaves the text alone, which is what notes and \
             to-do lists want.\n\n\
             Presentation only. Solarxy does not run a snippet from here: \
             a wrangle snippet is something you paste into an \
             `attribute_wrangle`, and this node stores it rather than \
             executing it.",
        ),
    ]);

    NodeTypeDescriptor {
        type_id: "text",
        version: 1,
        display_name: "Text",
        category: Category::Utility,
        // Every network kind, like `note`: where you keep a snippet is your
        // business, and a scene's scripts are not a property of its geometry
        // network in particular.
        contexts: ContextSet::ALL,
        opens: None,
        inputs: vec![],
        outputs: vec![],
        params,
        // Nothing to bypass: it does not transform anything.
        bypass: BypassBehavior::NotBypassable,
        doc: "A named snippet of text or code, stored in the scene.\n\n\
              Somewhere to keep a wrangle program you are reusing, a note to \
              your future self, or a fragment you are still working out. The \
              Text panel lists every snippet in the document and gives them \
              a full editor; this node is where one actually lives.\n\n\
              It computes nothing and has no ports. Solarxy does not run \
              snippets: to use one, paste it into an `attribute_wrangle` or \
              an expression field.",
        search_aliases: &[
            "text",
            "script",
            "snippet",
            "code",
            "notepad",
            "scratch",
            "datablock",
        ],
        glyph: "text",
        role: NodeRole::Text,
        cook: passive_cook,
        migrate: None,
    }
}
