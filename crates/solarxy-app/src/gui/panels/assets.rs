//! The Assets panel: a tile per staged asset, read from the engine's asset
//! table.
//!
//! **A consumer, not a feature.** The table is engine truth and this reads
//! it each frame the tab is up: staging happens through commands and
//! imports, so a tile appears or goes without a refresh. Every name an
//! entry is known by gets a tile, as the manifest lists one row per name:
//! bytes staged twice under two file names are one entry, and a model
//! naming either companion has to find it.
//!
//! **Thumbnails are decoded off the main thread and cached by content
//! hash.** A document can carry many large images and the panel may be
//! opened at any time, so the first sight of an image tile spawns a decode
//! that downsamples to a small square and hands back pixels; the frame
//! that finds them uploads a texture. A hash never changes meaning, so a
//! thumbnail is kept for the session, which is the browser's cache too.
//!
//! **What a tile shows is what the browser's shows**: a thumbnail for a
//! texture and a drawn placeholder for anything else, the name, and the
//! kind's caption. Size and whether anything references the asset are not
//! shown, by the ruling of 2026-09-11 that the browser wins; both are
//! filed as browser gaps.

use std::collections::HashMap;
use std::sync::{Arc, mpsc};

use solarxy_graph::assets::AssetTable;
use solarxy_studio::assets::{AssetKind, asset_kind};

use crate::gui::intent::{Intents, PanelIntent};
use crate::gui::theme::Theme;

/// What the panel draws: the staged assets, or nothing.
#[derive(Clone, Copy)]
pub(crate) enum AssetsSource<'a> {
    Empty,
    Scene { table: &'a AssetTable },
}

/// The longest edge a thumbnail is downsampled to.
pub(super) const THUMB_MAX_EDGE: u32 = 128;
/// A tile's width; the grid wraps as many as fit.
const TILE_WIDTH: f32 = 108.0;
/// The square a thumbnail or placeholder draws in.
const THUMB_SIDE: f32 = 92.0;

/// The browser's model placeholder: a wireframe cube with a spine.
const MODEL_GLYPH: &str = "M12 2 L21 7 V17 L12 22 L3 17 V7 Z M12 2 V12 M3 7 L12 12 L21 7";
/// The browser's file placeholder: a dog-eared page.
const FILE_GLYPH: &str = "M6 2 H14 L18 6 V22 H6 Z M14 2 V6 H18";
/// Both placeholders are drawn in a 24-unit box.
const GLYPH_VIEW_BOX: f32 = 24.0;

/// Pixels a decode hands back for upload.
struct Decoded {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// One content hash's thumbnail, at whatever stage it is.
enum Thumb {
    /// A decode is running on its own thread.
    Pending(mpsc::Receiver<Option<Decoded>>),
    Ready(egui::TextureHandle),
    /// The bytes did not decode; the placeholder stands in.
    Failed,
}

/// The panel's own state: the thumbnail cache.
#[derive(Default)]
pub(crate) struct AssetsState {
    thumbs: HashMap<String, Thumb>,
}

/// One tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Tile {
    pub hash: String,
    pub name: String,
    pub kind: AssetKind,
}

/// Every tile, one per name, sorted by name without regard to case and
/// then by hash, so two names that differ only in case keep a stable order.
pub(super) fn tiles(table: &AssetTable) -> Vec<Tile> {
    let mut out: Vec<Tile> = table
        .entries()
        .flat_map(|(id, entry)| {
            entry.names().map(move |name| Tile {
                hash: id.0.clone(),
                name: name.to_string(),
                kind: asset_kind(name),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.hash.cmp(&b.hash))
    });
    out
}

/// The tile's hover text, as the browser writes it.
pub(super) fn tooltip(tile: &Tile) -> String {
    let short: String = tile.hash.chars().take(12).collect();
    format!("{}\n{short}...  (double-click to preview)", tile.name)
}

/// The size a thumbnail is downsampled to: the longest edge at most
/// `max`, the other in proportion, never below one pixel.
pub(super) fn thumbnail_size(width: u32, height: u32, max: u32) -> (u32, u32) {
    let longest = width.max(height).max(1);
    if longest <= max {
        return (width.max(1), height.max(1));
    }
    let scale = f64::from(max) / f64::from(longest);
    let w = (f64::from(width) * scale).round() as u32;
    let h = (f64::from(height) * scale).round() as u32;
    (w.max(1), h.max(1))
}

/// Decode and downsample on a worker, since a large image decodes in
/// tens of milliseconds and the panel must not stall a frame per tile.
fn spawn_decode(bytes: Arc<Vec<u8>>) -> mpsc::Receiver<Option<Decoded>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let decoded = solarxy_formats::decode_image_bytes(&bytes)
            .ok()
            .and_then(|raw| {
                let (w, h) = thumbnail_size(raw.width, raw.height, THUMB_MAX_EDGE);
                let image = image::RgbaImage::from_raw(raw.width, raw.height, raw.pixels)?;
                let small = image::imageops::thumbnail(&image, w, h);
                Some(Decoded {
                    width: w,
                    height: h,
                    rgba: small.into_raw(),
                })
            });
        let _ = tx.send(decoded);
    });
    rx
}

impl AssetsState {
    /// Move every finished decode into a texture, and drop a finished
    /// failure to the placeholder.
    fn collect(&mut self, ctx: &egui::Context) {
        let mut done: Vec<(String, Option<Decoded>)> = Vec::new();
        for (hash, thumb) in &self.thumbs {
            if let Thumb::Pending(rx) = thumb {
                match rx.try_recv() {
                    Ok(result) => done.push((hash.clone(), result)),
                    Err(mpsc::TryRecvError::Disconnected) => done.push((hash.clone(), None)),
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        }
        for (hash, result) in done {
            let thumb = match result {
                Some(d) => Thumb::Ready(ctx.load_texture(
                    format!("solarxy_asset_thumb_{hash}"),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [d.width as usize, d.height as usize],
                        &d.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                )),
                None => Thumb::Failed,
            };
            self.thumbs.insert(hash, thumb);
        }
    }

    /// The thumbnail for `hash`, starting its decode on first sight.
    fn thumb(&mut self, hash: &str, bytes: &Arc<Vec<u8>>) -> Option<&egui::TextureHandle> {
        if !self.thumbs.contains_key(hash) {
            self.thumbs.insert(
                hash.to_string(),
                Thumb::Pending(spawn_decode(Arc::clone(bytes))),
            );
        }
        match self.thumbs.get(hash) {
            Some(Thumb::Ready(handle)) => Some(handle),
            _ => None,
        }
    }
}

/// Render the Assets panel into `ui` (the `egui_dock` tab supplies the `Ui`).
pub(in crate::gui) fn draw_assets_content(
    ui: &mut egui::Ui,
    source: AssetsSource<'_>,
    state: &mut AssetsState,
    intents: &mut Intents,
    theme: Theme,
) {
    let AssetsSource::Scene { table } = source else {
        return draw_empty(ui, theme);
    };
    let tiles = tiles(table);
    if tiles.is_empty() {
        return draw_empty(ui, theme);
    }
    state.collect(ui.ctx());

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            for tile in &tiles {
                draw_tile(ui, tile, table, state, intents, theme);
            }
        });
        ui.add_space(8.0);
    });
}

fn draw_tile(
    ui: &mut egui::Ui,
    tile: &Tile,
    table: &AssetTable,
    state: &mut AssetsState,
    intents: &mut Intents,
    theme: Theme,
) {
    let id = solarxy_graph::params::AssetId(tile.hash.clone());
    let thumb = match (tile.kind, table.get(&id)) {
        (AssetKind::Image, Some(entry)) => state.thumb(&tile.hash, &entry.bytes).cloned(),
        _ => None,
    };
    let response = ui
        .allocate_ui(egui::vec2(TILE_WIDTH, THUMB_SIDE + 34.0), |ui| {
            ui.vertical(|ui| {
                let (rect, _) = ui
                    .allocate_exact_size(egui::vec2(TILE_WIDTH, THUMB_SIDE), egui::Sense::hover());
                ui.painter().rect_filled(rect, 4.0, theme.bg_elevated);
                let square = egui::Rect::from_center_size(
                    rect.center(),
                    egui::Vec2::splat(THUMB_SIDE - 8.0),
                );
                match (&thumb, tile.kind) {
                    (Some(texture), _) => {
                        // Cover, as the browser's tile does: the shorter edge
                        // fills the square and the longer is cropped.
                        let size = texture.size_vec2();
                        let scale = (square.width() / size.x).max(square.height() / size.y);
                        let drawn = egui::Rect::from_center_size(square.center(), size * scale);
                        let uv = egui::Rect::from_min_max(
                            egui::pos2(
                                (square.min.x - drawn.min.x) / drawn.width(),
                                (square.min.y - drawn.min.y) / drawn.height(),
                            ),
                            egui::pos2(
                                (square.max.x - drawn.min.x) / drawn.width(),
                                (square.max.y - drawn.min.y) / drawn.height(),
                            ),
                        );
                        ui.put(
                            square,
                            egui::Image::from_texture((texture.id(), square.size())).uv(uv),
                        );
                    }
                    (None, AssetKind::Model) => {
                        let glyph =
                            egui::Rect::from_center_size(square.center(), egui::Vec2::splat(40.0));
                        super::nodes::paint_path(
                            ui.painter(),
                            MODEL_GLYPH,
                            GLYPH_VIEW_BOX,
                            glyph,
                            theme.muted,
                            1.2,
                        );
                    }
                    (None, AssetKind::Image) => {
                        // Decoding, or undecodable: the page stands in.
                        let glyph =
                            egui::Rect::from_center_size(square.center(), egui::Vec2::splat(40.0));
                        super::nodes::paint_path(
                            ui.painter(),
                            FILE_GLYPH,
                            GLYPH_VIEW_BOX,
                            glyph,
                            theme.muted,
                            1.2,
                        );
                    }
                    (None, AssetKind::Hdri | AssetKind::Other) => {
                        let glyph =
                            egui::Rect::from_center_size(square.center(), egui::Vec2::splat(40.0));
                        super::nodes::paint_path(
                            ui.painter(),
                            FILE_GLYPH,
                            GLYPH_VIEW_BOX,
                            glyph,
                            theme.muted,
                            1.2,
                        );
                    }
                }
                ui.add(
                    egui::Label::new(egui::RichText::new(&tile.name).size(11.0).color(theme.fg))
                        .truncate(),
                );
                ui.label(
                    egui::RichText::new(tile.kind.label().to_uppercase())
                        .size(9.0)
                        .color(theme.muted),
                );
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_text(tooltip(tile));
    if response.double_clicked() {
        intents.panel(PanelIntent::PreviewAsset {
            hash: tile.hash.clone(),
            name: tile.name.clone(),
        });
    }
}

fn draw_empty(ui: &mut egui::Ui, theme: Theme) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("No assets staged yet.").weak());
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Drop a model or add an Import node; its files appear here.")
                .color(theme.muted)
                .size(10.0),
        );
    });
}

/// The preview tab before the preview exists: what the browser's shows
/// with nothing chosen, and the chosen asset's name once one is.
pub(in crate::gui) fn draw_asset_preview_content(
    ui: &mut egui::Ui,
    preview: Option<&(String, String)>,
    theme: Theme,
) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| match preview {
        None => {
            ui.label(egui::RichText::new("Double-click an asset in the Assets panel.").weak());
        }
        Some((_, name)) => {
            ui.label(egui::RichText::new(name).color(theme.fg));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("No preview for this file type ({name})."))
                    .color(theme.muted)
                    .size(10.0),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> AssetTable {
        let mut table = AssetTable::new();
        let png = table.stage("Wood.png", "image/png", vec![1, 2, 3]);
        table.add_alias(&png, "wood_copy.png");
        table.stage("knot.obj", "model/obj", vec![4, 5]);
        table.stage("studio.hdr", "image/vnd.radiance", vec![6]);
        table.stage("readme.txt", "text/plain", vec![7]);
        table
    }

    /// One tile per name, aliases included, sorted without regard to case,
    /// each in the bin its extension puts it in.
    #[test]
    fn every_name_gets_a_tile_in_its_bin_sorted_by_name() {
        let tiles = tiles(&table());
        let names: Vec<&str> = tiles.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "knot.obj",
                "readme.txt",
                "studio.hdr",
                "Wood.png",
                "wood_copy.png"
            ]
        );
        let kinds: Vec<AssetKind> = tiles.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [
                AssetKind::Model,
                AssetKind::Other,
                AssetKind::Hdri,
                AssetKind::Image,
                AssetKind::Image
            ]
        );
        assert_eq!(
            tiles[3].hash, tiles[4].hash,
            "an alias shares its entry's hash"
        );
    }

    /// The hover text is the browser's: the name, the hash's first twelve
    /// characters, and the gesture.
    #[test]
    fn the_tooltip_is_the_browsers() {
        let tile = Tile {
            hash: "0123456789abcdef0123".to_string(),
            name: "wood.png".to_string(),
            kind: AssetKind::Image,
        };
        assert_eq!(
            tooltip(&tile),
            "wood.png\n0123456789ab...  (double-click to preview)"
        );
    }

    /// A thumbnail keeps its proportions, never exceeds the edge, and
    /// never collapses to zero.
    #[test]
    fn a_thumbnail_keeps_its_proportions_under_the_edge() {
        assert_eq!(thumbnail_size(2048, 1024, 128), (128, 64));
        assert_eq!(thumbnail_size(100, 300, 128), (43, 128));
        assert_eq!(
            thumbnail_size(64, 64, 128),
            (64, 64),
            "small images are not enlarged"
        );
        assert_eq!(thumbnail_size(4096, 1, 128), (128, 1));
        assert_eq!(thumbnail_size(0, 0, 128), (1, 1));
    }
}
