//! The texture viewer: the image network's published output, live.
//!
//! **A consumer of one engine query.** `Engine::display_image` answers with
//! the image the network's display node last published, as a shared
//! handle; this panel asks each frame it is up, which is an `Arc` clone and
//! a hash compare, and uploads a texture only when the hash moved. Cooked
//! pixels never ride the event stream on either shell, and asking is what
//! keeps this a mirror rather than a cache that can go stale.
//!
//! **It does not force a cook by being open.** The query reads what the
//! last cook stored; a network with nothing published says so rather than
//! showing the last image, because a stale image looks like a cook that
//! succeeded.
//!
//! **Which network:** the open canvas when it is an image network, else the
//! first image container at the root. The browser's rule, in
//! `web/src/components/TextureViewer.tsx`, and its status strings.
//!
//! The image letterboxes inside the panel, never enlarged past its own
//! size, and does not pan or zoom, which is what the browser's does; the
//! asset preview is the surface that pans.

use std::sync::Arc;

use solarxy_core::RawImageData;
use solarxy_graph::document::{ContextKind, Document, GraphContext, NodeId};
use solarxy_graph::registry::Registry;

use crate::gui::theme::Theme;

/// The network the panel shows, when the document has one.
#[derive(Clone, Copy)]
pub(crate) struct OwnerView<'a> {
    /// The container's display name.
    pub label: &'a str,
    /// What its display node published, if anything.
    pub image: Option<&'a Arc<RawImageData>>,
}

/// What the panel draws.
#[derive(Clone, Copy)]
pub(crate) enum TextureSource<'a> {
    /// No document.
    Empty,
    /// A document, with or without an image network.
    Scene { owner: Option<OwnerView<'a>> },
}

/// The panel's own state: the uploaded texture and the hash it came from.
#[derive(Default)]
pub(crate) struct TextureState {
    cached: Option<(u64, egui::TextureHandle)>,
}

impl TextureState {
    /// The texture for `image`, uploading only when its hash is not the
    /// one already uploaded, and dropping the upload when there is nothing
    /// to show.
    fn sync(
        &mut self,
        ctx: &egui::Context,
        image: Option<&Arc<RawImageData>>,
    ) -> Option<&egui::TextureHandle> {
        match image {
            None => {
                self.cached = None;
                None
            }
            Some(image) => {
                if self
                    .cached
                    .as_ref()
                    .is_none_or(|(hash, _)| *hash != image.hash)
                {
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width as usize, image.height as usize],
                        &image.pixels,
                    );
                    let handle = ctx.load_texture(
                        "solarxy_texture_viewer",
                        color,
                        egui::TextureOptions::LINEAR,
                    );
                    self.cached = Some((image.hash, handle));
                }
                self.cached.as_ref().map(|(_, handle)| handle)
            }
        }
    }

    #[cfg(test)]
    fn cached_id(&self) -> Option<egui::TextureId> {
        self.cached.as_ref().map(|(_, h)| h.id())
    }
}

/// The network to show: the open canvas when it is an image network, else
/// the first image container at the root.
pub(crate) fn texture_owner(
    doc: &Document,
    registry: &Registry,
    current: GraphContext,
) -> Option<NodeId> {
    if let GraphContext::Subflow(owner) = current
        && doc.graph(current).is_ok_and(|g| g.kind == ContextKind::Cop)
    {
        return Some(owner);
    }
    doc.graph(GraphContext::Root)
        .ok()?
        .nodes()
        .find(|n| registry.opens(&n.type_id) == Some(ContextKind::Cop))
        .map(|n| n.id)
}

/// The status line, as the browser writes it.
pub(super) fn status(owner: Option<(&str, Option<(u32, u32)>)>) -> String {
    match owner {
        None => "No texture network in the scene.".to_string(),
        Some((label, Some((w, h)))) => format!("{label} \u{00b7} {w} \u{00d7} {h}"),
        Some((label, None)) => format!("{label} \u{00b7} no image published (set a display node)"),
    }
}

/// The scale that letterboxes the image inside the stage without ever
/// enlarging it past its own size.
pub(super) fn contain_scale(stage: egui::Vec2, image: egui::Vec2) -> f32 {
    (stage.x / image.x.max(1.0))
        .min(stage.y / image.y.max(1.0))
        .min(1.0)
}

/// Render the texture viewer into `ui`.
pub(in crate::gui) fn draw_texture_content(
    ui: &mut egui::Ui,
    source: TextureSource<'_>,
    state: &mut TextureState,
    theme: Theme,
) {
    let owner = match source {
        TextureSource::Empty => None,
        TextureSource::Scene { owner } => owner,
    };
    let dims = owner
        .and_then(|o| o.image)
        .map(|image| (image.width, image.height));
    ui.label(
        egui::RichText::new(status(owner.map(|o| (o.label, dims))))
            .color(theme.muted)
            .size(10.0),
    );

    let texture = state.sync(ui.ctx(), owner.and_then(|o| o.image)).cloned();
    let Some(texture) = texture else {
        return;
    };
    let stage_size = ui.available_size();
    let (stage, _) = ui.allocate_exact_size(stage_size, egui::Sense::hover());
    paint_checker(ui.painter(), stage, theme);
    let size = texture.size_vec2();
    let scale = contain_scale(stage.size(), size);
    let drawn = egui::Rect::from_center_size(stage.center(), size * scale);
    ui.painter().with_clip_rect(stage).image(
        texture.id(),
        drawn,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::from_gray(255),
    );
}

/// A checker under the image so its transparency reads.
fn paint_checker(painter: &egui::Painter, rect: egui::Rect, theme: Theme) {
    const CELL: f32 = 16.0;
    painter.rect_filled(rect, 0.0, theme.bg);
    let cols = (rect.width() / CELL).ceil() as i32;
    let rows = (rect.height() / CELL).ceil() as i32;
    let clipped = painter.with_clip_rect(rect);
    for row in 0..rows {
        for col in 0..cols {
            if (row + col) % 2 == 0 {
                let min = rect.min + egui::vec2(col as f32 * CELL, row as f32 * CELL);
                clipped.rect_filled(
                    egui::Rect::from_min_size(min, egui::Vec2::splat(CELL)),
                    0.0,
                    theme.bg_elevated,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::engine::{Engine, EngineEvent};
    use solarxy_graph::Command;

    fn add(engine: &mut Engine, ctx: GraphContext, ty: &str) -> NodeId {
        let batch = engine
            .apply(Command::AddNode {
                ctx,
                node_type: ty.to_string(),
                position: [0.0, 0.0],
            })
            .expect("the node adds");
        batch
            .events
            .iter()
            .find_map(|ev| match ev {
                EngineEvent::NodeAdded { node, .. } => Some(node.id),
                _ => None,
            })
            .expect("a node was added")
    }

    /// The owner is the open canvas when it is an image network, else the
    /// first image container at the root, and nothing when there is none.
    #[test]
    fn the_owner_is_the_open_image_network_else_the_first_at_the_root() {
        let mut engine = Engine::new().expect("engine");
        let sop = add(&mut engine, GraphContext::Root, "sopnet");
        let (doc, registry) = (engine.document(), engine.registry());
        assert_eq!(texture_owner(doc, registry, GraphContext::Root), None);
        assert_eq!(
            texture_owner(doc, registry, GraphContext::Subflow(sop)),
            None
        );

        let first = add(&mut engine, GraphContext::Root, "copnet");
        let second = add(&mut engine, GraphContext::Root, "copnet");
        let (doc, registry) = (engine.document(), engine.registry());
        assert_eq!(
            texture_owner(doc, registry, GraphContext::Root),
            Some(first)
        );
        assert_eq!(
            texture_owner(doc, registry, GraphContext::Subflow(sop)),
            Some(first),
            "inside a geometry network the first image container still shows"
        );
        assert_eq!(
            texture_owner(doc, registry, GraphContext::Subflow(second)),
            Some(second),
            "inside an image network, that one"
        );
    }

    /// The three status strings are the browser's.
    #[test]
    fn the_status_strings_are_the_browsers() {
        assert_eq!(status(None), "No texture network in the scene.");
        assert_eq!(
            status(Some(("copnet1", Some((512, 256))))),
            "copnet1 \u{00b7} 512 \u{00d7} 256"
        );
        assert_eq!(
            status(Some(("copnet1", None))),
            "copnet1 \u{00b7} no image published (set a display node)"
        );
    }

    /// The image letterboxes and is never enlarged.
    #[test]
    fn the_image_letterboxes_and_is_never_enlarged() {
        let stage = egui::vec2(400.0, 300.0);
        assert!((contain_scale(stage, egui::vec2(800.0, 300.0)) - 0.5).abs() < 1e-6);
        assert!((contain_scale(stage, egui::vec2(400.0, 900.0)) - (1.0 / 3.0)).abs() < 1e-6);
        assert!((contain_scale(stage, egui::vec2(100.0, 50.0)) - 1.0).abs() < 1e-6);
    }

    /// One upload per published image: the same hash keeps its texture, a
    /// new hash replaces it, and nothing published drops it.
    #[test]
    fn a_texture_is_uploaded_once_per_hash_and_dropped_when_nothing_is_published() {
        let ctx = egui::Context::default();
        let mut state = TextureState::default();
        let a = Arc::new(RawImageData::new(vec![255; 16], 2, 2));
        let a_again = Arc::new(RawImageData::new(vec![255; 16], 2, 2));
        let b = Arc::new(RawImageData::new(vec![0; 16], 2, 2));
        assert_ne!(a.hash, b.hash, "the fixtures differ");
        assert_eq!(a.hash, a_again.hash, "the fixtures agree");

        assert!(state.sync(&ctx, Some(&a)).is_some());
        let first = state.cached_id().expect("uploaded");
        state.sync(&ctx, Some(&a_again));
        assert_eq!(
            state.cached_id(),
            Some(first),
            "the same hash keeps its upload"
        );
        state.sync(&ctx, Some(&b));
        assert_ne!(state.cached_id(), Some(first), "a new hash replaces it");
        assert!(state.sync(&ctx, None).is_none());
        assert_eq!(state.cached_id(), None, "nothing published drops it");
    }
}
