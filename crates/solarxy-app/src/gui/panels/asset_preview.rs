//! The asset preview panel: two-dimensional pan and zoom for an image, an
//! orbiting view for a model, and a plain statement for anything else.
//!
//! **The image half is the panel's own.** It decodes the staged bytes on
//! a worker, holds one texture for the asset it shows, and pans and zooms
//! it with the browser's rules: the wheel scales by `exp(-dy / 1000)`
//! between five percent and fifty times, a drag pans, a double-click
//! resets, and scale one is the image fitted inside ninety percent of the
//! panel and never enlarged past its own size.
//!
//! **The model half is the state layer's.** It renders on demand into a
//! texture of its own through `solarxy_host::preview`, and this panel
//! draws the handle, reports its size, and raises a gesture per drag and
//! per wheel notch. The size travels as an observation rather than an
//! intent, because a size is a standing fact and the queue takes only
//! responses.

use std::sync::{Arc, mpsc};

use solarxy_studio::assets::{AssetKind, asset_kind};

use super::assets::{AssetsSource, thumbnail_size};
use crate::gui::intent::{Intents, PanelIntent};
use crate::gui::theme::Theme;
use crate::state::preview::PreviewGesture;

/// What the state layer holds for the model half.
#[derive(Clone, Copy, Default)]
pub(crate) struct PreviewView<'a> {
    /// The rendered model, and the size it was rendered at.
    pub texture: Option<(egui::TextureId, [u32; 2])>,
    /// A parse is still running.
    pub loading: bool,
    /// Why there is no model to show.
    pub error: Option<&'a str>,
}

/// The wheel's zoom bounds.
pub(super) const MIN_ZOOM: f32 = 0.05;
pub(super) const MAX_ZOOM: f32 = 50.0;
/// The image's longest edge is capped here, since the texture is the
/// image at full size.
pub(super) const IMAGE_MAX_EDGE: u32 = 4096;
/// The browser fits the image inside this fraction of the stage.
pub(super) const FIT_FRACTION: f32 = 0.9;

/// The zoom after one wheel notch of `dy`, the browser's rule.
pub(super) fn zoom_after_wheel(zoom: f32, dy: f32) -> f32 {
    (zoom * (-dy * 0.001).exp()).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// The scale at which an image fits ninety percent of the stage without
/// being enlarged past its own size: what zoom one means.
pub(super) fn fit_scale(stage: egui::Vec2, image: egui::Vec2) -> f32 {
    let x = FIT_FRACTION * stage.x / image.x.max(1.0);
    let y = FIT_FRACTION * stage.y / image.y.max(1.0);
    x.min(y).min(1.0)
}

/// The wheel's dolly for the model half.
pub(super) fn wheel_to_dolly(dy: f32) -> f32 {
    -dy * 0.01
}

/// The readout under an image.
pub(super) fn hud(zoom: f32) -> String {
    format!(
        "{}% (drag to pan, wheel to zoom, double-click to reset)",
        (zoom * 100.0).round() as i32
    )
}

struct Decoded {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

enum ImageStage {
    Pending(mpsc::Receiver<Option<Decoded>>),
    Ready(egui::TextureHandle),
    Failed,
}

/// The image half's state: the one decoded image, and its view.
pub(crate) struct AssetPreviewState {
    image: Option<(String, ImageStage)>,
    zoom: f32,
    offset: egui::Vec2,
}

impl Default for AssetPreviewState {
    fn default() -> Self {
        Self {
            image: None,
            zoom: 1.0,
            offset: egui::Vec2::ZERO,
        }
    }
}

impl AssetPreviewState {
    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.offset = egui::Vec2::ZERO;
    }
}

fn spawn_decode(bytes: Arc<Vec<u8>>) -> mpsc::Receiver<Option<Decoded>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let decoded = solarxy_formats::decode_image_bytes(&bytes)
            .ok()
            .and_then(|raw| {
                let (w, h) = thumbnail_size(raw.width, raw.height, IMAGE_MAX_EDGE);
                let image = image::RgbaImage::from_raw(raw.width, raw.height, raw.pixels)?;
                let rgba = if (w, h) == (raw.width, raw.height) {
                    image.into_raw()
                } else {
                    image::imageops::thumbnail(&image, w, h).into_raw()
                };
                Some(Decoded {
                    width: w,
                    height: h,
                    rgba,
                })
            });
        let _ = tx.send(decoded);
    });
    rx
}

/// Render the preview panel into `ui`.
#[allow(clippy::too_many_arguments)]
pub(in crate::gui) fn draw_asset_preview_content(
    ui: &mut egui::Ui,
    asset: Option<&(String, String)>,
    assets: AssetsSource<'_>,
    model: PreviewView<'_>,
    state: &mut AssetPreviewState,
    size_out: &mut Option<(u32, u32)>,
    intents: &mut Intents,
    theme: Theme,
) {
    let Some((hash, name)) = asset else {
        state.image = None;
        return placeholder(ui, "Double-click an asset in the Assets panel.", theme);
    };
    match asset_kind(name) {
        AssetKind::Image => draw_image(ui, hash, assets, state, theme),
        AssetKind::Model => draw_model(ui, model, size_out, intents, theme),
        AssetKind::Hdri | AssetKind::Other => {
            state.image = None;
            placeholder(
                ui,
                &format!("No preview for this file type ({name})."),
                theme,
            );
        }
    }
}

fn draw_image(
    ui: &mut egui::Ui,
    hash: &str,
    assets: AssetsSource<'_>,
    state: &mut AssetPreviewState,
    theme: Theme,
) {
    // A different asset than last frame: start its decode and reset the view.
    if state.image.as_ref().is_none_or(|(h, _)| h != hash) {
        let AssetsSource::Scene { table } = assets else {
            return placeholder(ui, "Asset bytes unavailable.", theme);
        };
        let Some(entry) = table.get(&solarxy_graph::params::AssetId(hash.to_string())) else {
            return placeholder(ui, "Asset bytes unavailable.", theme);
        };
        state.image = Some((
            hash.to_string(),
            ImageStage::Pending(spawn_decode(Arc::clone(&entry.bytes))),
        ));
        state.reset_view();
    }
    // Move a finished decode into a texture.
    if let Some((_, stage @ ImageStage::Pending(_))) = state.image.as_mut() {
        let ImageStage::Pending(rx) = stage else {
            unreachable!()
        };
        let result = match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(None),
            Err(mpsc::TryRecvError::Empty) => None,
        };
        if let Some(result) = result {
            *stage = match result {
                Some(d) => ImageStage::Ready(ui.ctx().load_texture(
                    format!("solarxy_asset_preview_{hash}"),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [d.width as usize, d.height as usize],
                        &d.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                )),
                None => ImageStage::Failed,
            };
        }
    }

    let texture = match state.image.as_ref() {
        Some((_, ImageStage::Ready(t))) => t.clone(),
        Some((_, ImageStage::Failed)) => return placeholder(ui, "Asset bytes unavailable.", theme),
        _ => return placeholder(ui, "Decoding\u{2026}", theme),
    };

    let available = ui.available_size();
    let stage_size = egui::vec2(available.x, (available.y - 18.0).max(32.0));
    let (stage, response) = ui.allocate_exact_size(stage_size, egui::Sense::click_and_drag());
    paint_checker(ui.painter(), stage, theme);

    if response.dragged() {
        state.offset += response.drag_delta();
    }
    if response.double_clicked() {
        state.reset_view();
    }
    if response.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            state.zoom = zoom_after_wheel(state.zoom, -dy);
        }
    }

    let image_size = texture.size_vec2();
    let scale = fit_scale(stage.size(), image_size) * state.zoom;
    let drawn = egui::Rect::from_center_size(stage.center() + state.offset, image_size * scale);
    let clipped = ui.painter().with_clip_rect(stage);
    clipped.image(
        texture.id(),
        drawn,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::from_gray(255),
    );
    ui.label(
        egui::RichText::new(hud(state.zoom))
            .color(theme.muted)
            .size(10.0),
    );
}

fn draw_model(
    ui: &mut egui::Ui,
    model: PreviewView<'_>,
    size_out: &mut Option<(u32, u32)>,
    intents: &mut Intents,
    theme: Theme,
) {
    let available = ui.available_size();
    let ppp = ui.ctx().pixels_per_point();
    let physical = |v: f32| (v * ppp).round().max(1.0) as u32;
    *size_out = Some((physical(available.x), physical(available.y)));

    if let Some(error) = model.error {
        return placeholder(ui, &format!("Could not preview: {error}"), theme);
    }
    let Some((texture, size)) = model.texture else {
        return placeholder(
            ui,
            if model.loading {
                "Loading\u{2026}"
            } else {
                "Preparing the preview\u{2026}"
            },
            theme,
        );
    };
    let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
    // Drawn at the rect: the texture was rendered at this rect's physical
    // size, so points map to pixels one to one. A frame where the two
    // disagree stretches for that frame and the next render fixes it.
    let _ = size;
    ui.painter().image(
        texture,
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::from_gray(255),
    );
    if response.dragged() {
        let d = response.drag_delta() * ppp;
        if d != egui::Vec2::ZERO {
            intents.panel(PanelIntent::Preview(PreviewGesture::Orbit(d.x, d.y)));
        }
    }
    if response.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            intents.panel(PanelIntent::Preview(PreviewGesture::Zoom(wheel_to_dolly(
                -dy,
            ))));
        }
    }
    response.on_hover_text("Drag to orbit, wheel to zoom");
}

/// A checker under an image so its transparency reads.
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

fn placeholder(ui: &mut egui::Ui, text: &str, theme: Theme) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).color(theme.muted));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wheel scales by the browser's factor and stops at its bounds.
    #[test]
    fn the_wheel_zooms_by_the_browsers_factor_within_its_bounds() {
        let z = zoom_after_wheel(1.0, -100.0);
        assert!((z - (0.1_f32).exp()).abs() < 1e-5, "a notch in: {z}");
        let z = zoom_after_wheel(1.0, 100.0);
        assert!((z - (-0.1_f32).exp()).abs() < 1e-5, "a notch out: {z}");
        assert!((zoom_after_wheel(0.06, 10_000.0) - MIN_ZOOM).abs() < 1e-6);
        assert!((zoom_after_wheel(40.0, -10_000.0) - MAX_ZOOM).abs() < 1e-6);
    }

    /// Scale one fits the image inside ninety percent of the stage and
    /// never enlarges a small image past its own size.
    #[test]
    fn scale_one_is_the_fit_and_never_an_enlargement() {
        let stage = egui::vec2(1000.0, 500.0);
        assert!(
            (fit_scale(stage, egui::vec2(2000.0, 500.0)) - 0.45).abs() < 1e-6,
            "width bound"
        );
        assert!(
            (fit_scale(stage, egui::vec2(500.0, 1000.0)) - 0.45).abs() < 1e-6,
            "height bound"
        );
        assert!(
            (fit_scale(stage, egui::vec2(100.0, 100.0)) - 1.0).abs() < 1e-6,
            "not enlarged"
        );
    }

    /// The readout and the dolly are the browser's.
    #[test]
    fn the_readout_and_the_dolly_are_the_browsers() {
        assert_eq!(
            hud(1.0),
            "100% (drag to pan, wheel to zoom, double-click to reset)"
        );
        assert_eq!(
            hud(0.456),
            "46% (drag to pan, wheel to zoom, double-click to reset)"
        );
        assert!((wheel_to_dolly(100.0) + 1.0).abs() < 1e-6);
    }
}
