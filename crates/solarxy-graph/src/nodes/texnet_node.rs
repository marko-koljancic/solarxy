//! The `texnet` root container: opens a
//! texture network (`ContextKind::Tex`) whose display node publishes the
//! network's image. Referenced by path from material map inputs (the
//! `tex_ref` pattern) and previewed live in the texture viewer
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
        doc: "A container you dive into to build an image procedurally: \
              `constant`, `ramp`, `noise` and `import_image` as sources, \
              then the adjust, filter and composite nodes. Whichever node \
              inside carries the display flag publishes the network's \
              image.\n\n\
              Drop one at the root next to your `geo` and `matnet` nodes, \
              build the image inside, then consume it from a material \
              network with a `tex_ref`, whose Texture Network param points \
              at this container. The texture viewer pane previews the \
              published image live while you work, and editing anything \
              inside recooks every referrer.\n\n\
              The reference is a path, not a wire. This node has no ports \
              at all, so nothing connects to it on the canvas, and it is \
              not a scene object either -- no transform, nothing lowered \
              into the scene delta. A texnet nothing refers to still cooks \
              and still shows nothing.",
        search_aliases: &["texnet", "texture", "cop", "image network"],
        glyph: "texnet",
        role: NodeRole::Container,
        cook: passive_cook,
        migrate: None,
    }
}
