//! The `texnet` root container (context-expansion phase 19): opens a
//! texture network (`ContextKind::Tex`) whose display node publishes the
//! network's image. Referenced by path from material map inputs (the
//! `tex_ref` pattern, phase 20) and previewed live in the texture viewer
//! pane; never a scene object, so it carries no transform and lowers to
//! nothing in the scene delta.

use super::common::{params_with, passive_cook};
use crate::document::ContextKind;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "texnet",
        version: 1,
        display_name: "Texture Network",
        category: Category::Container,
        contexts: ContextSet::OBJ,
        opens: Some(ContextKind::Tex),
        inputs: vec![],
        outputs: vec![],
        params: params_with("Texture Network", vec![]),
        bypass: BypassBehavior::NotBypassable,
        doc: "A texture network: image nodes cook inside it, and its display node publishes the network's image for material map references and the texture viewer.",
        search_aliases: &["texnet", "texture", "cop", "image network"],
        glyph: "texnet",
        role: NodeRole::Container,
        cook: passive_cook,
        migrate: None,
    }
}
