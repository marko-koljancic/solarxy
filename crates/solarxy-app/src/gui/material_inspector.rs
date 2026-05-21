//! View-only Material Inspector — a master/detail panel.
//!
//! Toggled via `Window → Material Inspector` (menu-only — no global
//! keyboard shortcut; `M` and `Shift+M` are already bound to the
//! material override cycle, and the comprehensive-plan decision log
//! D1 ruled out a new modifier-key binding).
//!
//! Layout adapts to the dock shape so the panel works equally well as a
//! tall sidebar, a short bottom bar, or a square float:
//!
//! - A **compact material list** (the picker): one selectable row per
//!   material — base-color swatch, name, and a five-square texture-slot
//!   presence indicator.
//! - A **detail pane** for the selected material: scalar PBR values plus
//!   one 128 px entry per texture role.
//!
//! The split goes side-by-side when the panel is wide enough
//! ([`use_side_by_side`]), list-on-top otherwise. Both halves are
//! user-resizable (`egui::SidePanel` / `TopBottomPanel::show_inside`).
//!
//! Reads CPU-side data from
//! [`solarxy_renderer::model::Model::material_thumbnails`] (populated
//! during `resources::upload_model` from the source `RawMaterialData`
//! before the bytes are consumed by GPU upload). 128×128 thumbnails are
//! decoded on first use per `(material_idx, role)` and stashed in an
//! egui texture cache cleared on model swap.
//!
//! Visibility is the canonical
//! `MenuBarVisibility.material_inspector_visible` flag threaded in as
//! `&mut bool` — same pattern as Review Panel + Console. The window's X
//! button writes through that flag.
//!
//! "Open externally" uses the existing `open` workspace dep
//! (`open::that(path)`); disabled when the source path is `None` (i.e.
//! embedded glTF textures with no on-disk file).

use std::collections::HashMap;

use image::{ImageBuffer, Rgba, imageops};
use solarxy_renderer::model::{MaterialThumbnails, Model, TextureThumbnail};

use super::overlays::ToastSeverity;
use super::theme::Theme;

/// Decode resolution for cached thumbnails.
const THUMBNAIL_SIZE: u32 = 128;
/// Texture preview edge length in the detail pane.
const TEXTURE_PREVIEW: f32 = 128.0;
/// Fixed width of a texture entry block in the detail pane's wrapping
/// flow — a `TEXTURE_PREVIEW` image plus a little horizontal breathing
/// room.
const TEXTURE_BLOCK_WIDTH: f32 = 150.0;
/// Fixed height of a texture entry block. Every block is sized to this
/// exactly (shorter content padded out) so `horizontal_wrapped` forms a
/// clean grid instead of staggering items by their natural height.
const TEXTURE_BLOCK_HEIGHT: f32 = 216.0;
/// Height of one material row in the picker list.
const LIST_ROW_HEIGHT: f32 = 20.0;
/// Below this panel width the split stacks (list on top) instead of
/// going side-by-side — narrower than this and the detail pane could not
/// fit a 128 px preview next to the list.
const SPLIT_MIN_WIDTH: f32 = 360.0;

/// Source-side texture role. Mirrors the five `Option<RawImageData>`
/// slots on `RawMaterialData` — `MetallicRoughness` and `Occlusion` are
/// kept separate (the renderer packs them into a single GPU "ORM"
/// texture, but artists author them separately and want to inspect each
/// independently).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TextureRole {
    Albedo,
    Normal,
    MetallicRoughness,
    Occlusion,
    Emissive,
}

impl TextureRole {
    fn label(self) -> &'static str {
        match self {
            Self::Albedo => "Albedo",
            Self::Normal => "Normal",
            Self::MetallicRoughness => "Metallic / Roughness",
            Self::Occlusion => "Occlusion",
            Self::Emissive => "Emissive",
        }
    }

    fn thumbnail_in(self, thumbs: &MaterialThumbnails) -> Option<&TextureThumbnail> {
        match self {
            Self::Albedo => thumbs.albedo.as_ref(),
            Self::Normal => thumbs.normal.as_ref(),
            Self::MetallicRoughness => thumbs.metallic_roughness.as_ref(),
            Self::Occlusion => thumbs.occlusion.as_ref(),
            Self::Emissive => thumbs.emissive.as_ref(),
        }
    }

    const ALL: &'static [Self] = &[
        Self::Albedo,
        Self::Normal,
        Self::MetallicRoughness,
        Self::Occlusion,
        Self::Emissive,
    ];
}

/// Per-model cache for the Material Inspector. Visibility lives in
/// `MenuBarVisibility`; this struct holds the decoded egui textures, the
/// picker selection, and the deferred toast queue (drained by
/// `EguiRenderer::render_ui` after the egui frame closes, where
/// `push_toast` access is available).
#[derive(Default)]
pub(super) struct MaterialInspectorState {
    thumbnail_cache: HashMap<(usize, TextureRole), egui::TextureHandle>,
    /// Index of the material shown in the detail pane. Reset to 0 on
    /// model swap and clamped into range each frame.
    selected: usize,
    /// Toast messages produced inside the egui frame closure (success /
    /// failure of "Open externally"). Drained by the renderer after the
    /// closure returns. Pre-existing toast queue + `push_toast` live on
    /// `EguiRenderer`; the closure can't reach them, so we buffer.
    pub pending_toasts: Vec<(String, ToastSeverity)>,
}

impl MaterialInspectorState {
    /// Drop the egui texture cache when a new model loads. Called from
    /// `EguiRenderer::clear_for_new_model`. Free-by-drop is enough —
    /// egui keeps no GPU-side reference beyond the `TextureHandle`'s
    /// internal refcount.
    pub fn clear_for_new_model(&mut self) {
        self.thumbnail_cache.clear();
        self.selected = 0;
        self.pending_toasts.clear();
    }
}

/// Whether the master/detail split should be side-by-side (list left,
/// detail right) rather than stacked (list on top). Side-by-side needs
/// the panel both wider than [`SPLIT_MIN_WIDTH`] and at least as wide as
/// it is tall.
fn use_side_by_side(width: f32, height: f32) -> bool {
    width >= SPLIT_MIN_WIDTH && width >= height
}

/// Render the Material Inspector's content into the provided `ui`.
/// The hosting Window / dock tab is the caller's job (`egui_dock` tab in
/// `gui::dock`).
pub(super) fn draw_material_inspector_content(
    ui: &mut egui::Ui,
    model: &Model,
    state: &mut MaterialInspectorState,
    theme: &Theme,
) {
    if model.materials.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("This model has no materials.")
                    .weak()
                    .italics(),
            );
        });
        return;
    }

    // Keep the selection valid even if a smaller model loaded mid-frame.
    state.selected = state.selected.min(model.materials.len() - 1);

    if use_side_by_side(ui.available_width(), ui.available_height()) {
        egui::SidePanel::left("solarxy_material_list_side")
            .resizable(true)
            .default_width(180.0)
            .width_range(140.0..=320.0)
            .show_inside(ui, |ui| draw_material_list(ui, model, state, theme));
        egui::CentralPanel::default()
            .show_inside(ui, |ui| draw_material_detail(ui, model, state, theme));
    } else {
        egui::TopBottomPanel::top("solarxy_material_list_top")
            .resizable(true)
            .default_height(150.0)
            .height_range(60.0..=320.0)
            .show_inside(ui, |ui| draw_material_list(ui, model, state, theme));
        egui::CentralPanel::default()
            .show_inside(ui, |ui| draw_material_detail(ui, model, state, theme));
    }
}

// ---------------------------------------------------------------------
// Master — the material picker list
// ---------------------------------------------------------------------

fn draw_material_list(
    ui: &mut egui::Ui,
    model: &Model,
    state: &mut MaterialInspectorState,
    theme: &Theme,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (idx, mat) in model.materials.iter().enumerate() {
                let Some(thumbs) = model.material_thumbnails.get(idx) else {
                    continue;
                };
                if draw_material_row(ui, idx, mat, thumbs, theme, idx == state.selected) {
                    state.selected = idx;
                }
            }
        });
}

/// One picker row: selection background, base-color swatch, truncated
/// name, and a five-square texture-slot presence indicator. Painted
/// manually (no child widgets) so the single row response owns the
/// click/hover cleanly. Returns `true` when clicked.
fn draw_material_row(
    ui: &mut egui::Ui,
    idx: usize,
    mat: &solarxy_renderer::material::Material,
    thumbs: &MaterialThumbnails,
    theme: &Theme,
    selected: bool,
) -> bool {
    const PAD: f32 = 4.0;
    const SWATCH: f32 = 12.0;
    const SQUARE: f32 = 6.0;
    const SQUARE_GAP: f32 = 2.5;

    let name = if mat.name.is_empty() {
        format!("Material {idx}")
    } else {
        mat.name.clone()
    };
    let swatch = base_color_to_color32(thumbs.base_color);

    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), LIST_ROW_HEIGHT),
        egui::Sense::click(),
    );

    if selected {
        ui.painter().rect_filled(rect, 0.0, theme.selection);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme.widget_hover);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // Base-color swatch, hard left.
    let swatch_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + PAD + SWATCH / 2.0, rect.center().y),
        egui::vec2(SWATCH, SWATCH),
    );
    ui.painter().rect_filled(swatch_rect, 0.0, swatch);
    ui.painter().rect_stroke(
        swatch_rect,
        0.0,
        egui::Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );

    // Texture-slot presence indicator, hard right.
    let present_count = TextureRole::ALL
        .iter()
        .filter(|r| r.thumbnail_in(thumbs).is_some())
        .count();
    let presence_w = 5.0 * SQUARE + 4.0 * SQUARE_GAP;
    let presence_left = rect.right() - PAD - presence_w;
    let mut sx = presence_left;
    for &role in TextureRole::ALL {
        let square = egui::Rect::from_min_size(
            egui::pos2(sx, rect.center().y - SQUARE / 2.0),
            egui::vec2(SQUARE, SQUARE),
        );
        if role.thumbnail_in(thumbs).is_some() {
            ui.painter().rect_filled(square, 0.0, theme.muted);
        } else {
            ui.painter().rect_stroke(
                square,
                0.0,
                egui::Stroke::new(1.0, theme.border),
                egui::StrokeKind::Inside,
            );
        }
        sx += SQUARE + SQUARE_GAP;
    }

    // Name, filling the gap between swatch and presence indicator.
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(swatch_rect.right() + 6.0, rect.top()),
        egui::pos2(presence_left - 6.0, rect.bottom()),
    );
    if text_rect.width() > 4.0 {
        paint_truncated_text(ui, text_rect, &name, theme.fg);
    }

    resp.on_hover_text(format!("{present_count}/5 texture maps"))
        .clicked()
}

/// Paint a single line of text into `rect`, truncated with an ellipsis
/// if it does not fit. Used for picker rows, where a child `Label` would
/// occlude the row's own click/hover response.
fn paint_truncated_text(ui: &egui::Ui, rect: egui::Rect, text: &str, color: egui::Color32) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(13.0),
            color,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: rect.width(),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
}

// ---------------------------------------------------------------------
// Detail — the selected material
// ---------------------------------------------------------------------

fn draw_material_detail(
    ui: &mut egui::Ui,
    model: &Model,
    state: &mut MaterialInspectorState,
    theme: &Theme,
) {
    let idx = state.selected;
    let (Some(mat), Some(thumbs)) = (model.materials.get(idx), model.material_thumbnails.get(idx))
    else {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("Select a material.").weak().italics());
        });
        return;
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            draw_detail_header(ui, idx, mat, thumbs, theme);
            ui.add_space(4.0);
            draw_detail_scalars(ui, mat);
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(2.0);
            ui.label(egui::RichText::new("Textures").small().strong());
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for &role in TextureRole::ALL {
                    draw_texture_block(ui, idx, role, thumbs, state, theme);
                }
            });
        });
}

fn draw_detail_header(
    ui: &mut egui::Ui,
    idx: usize,
    mat: &solarxy_renderer::material::Material,
    thumbs: &MaterialThumbnails,
    theme: &Theme,
) {
    let name = if mat.name.is_empty() {
        format!("Material {idx}")
    } else {
        mat.name.clone()
    };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 0.0, base_color_to_color32(thumbs.base_color));
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );
        ui.add(egui::Label::new(egui::RichText::new(&name).size(15.0).strong()).truncate());
    });

    let (prefix, suffix) = if thumbs.albedo.is_some() {
        ("Base color factor", " × Albedo")
    } else {
        ("Base color", "")
    };
    ui.label(
        egui::RichText::new(format!(
            "{prefix}: {}{suffix}",
            color_hex(thumbs.base_color)
        ))
        .monospace()
        .small(),
    );
}

fn draw_detail_scalars(ui: &mut egui::Ui, mat: &solarxy_renderer::material::Material) {
    ui.horizontal_wrapped(|ui| {
        scalar_chip(
            ui,
            "Metallic",
            &format!("{:.2}", mat.uniform.metallic_factor),
        );
        scalar_chip(
            ui,
            "Roughness",
            &format!("{:.2}", mat.uniform.roughness_factor),
        );
        scalar_chip(
            ui,
            "Alpha",
            &alpha_mode_label(mat.uniform.alpha_mode, mat.uniform.alpha_cutoff),
        );
        let emissive = mat.uniform.emissive;
        if emissive != [0.0, 0.0, 0.0] {
            scalar_chip(
                ui,
                "Emissive",
                &format!(
                    "({:.2}, {:.2}, {:.2})",
                    emissive[0], emissive[1], emissive[2]
                ),
            );
        }
    });
}

/// One `label value` pair in the detail pane's wrapping scalar row.
fn scalar_chip(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).small().weak());
    ui.label(egui::RichText::new(value).small().monospace());
    ui.add_space(10.0);
}

/// Paint the slashed-box placeholder shown in a texture block when the
/// slot is empty or its thumbnail could not be decoded.
fn draw_thumbnail_placeholder(ui: &mut egui::Ui, size: egui::Vec2, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme.widget_bg);
    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(1.0, theme.border),
    );
}

/// One texture role in the detail pane: a fixed-width block carrying a
/// 128 px preview plus filename / dimensions / "Open externally" when
/// the slot is filled, or a greyed placeholder when it is absent.
fn draw_texture_block(
    ui: &mut egui::Ui,
    material_idx: usize,
    role: TextureRole,
    thumbs: &MaterialThumbnails,
    state: &mut MaterialInspectorState,
    theme: &Theme,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(TEXTURE_BLOCK_WIDTH, TEXTURE_BLOCK_HEIGHT),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            // Pin every block to the same footprint so the wrapping flow
            // grids cleanly; content shorter than this is padded out.
            ui.set_width(TEXTURE_BLOCK_WIDTH);
            ui.set_min_height(TEXTURE_BLOCK_HEIGHT);
            ui.spacing_mut().item_spacing.y = 3.0;
            let preview = egui::vec2(TEXTURE_PREVIEW, TEXTURE_PREVIEW);

            let Some(tex) = role.thumbnail_in(thumbs) else {
                ui.add(
                    egui::Label::new(egui::RichText::new(role.label()).small().weak()).truncate(),
                );
                draw_thumbnail_placeholder(ui, preview, theme);
                ui.label(egui::RichText::new("not present").small().weak().italics());
                return;
            };

            ui.add(egui::Label::new(egui::RichText::new(role.label()).small().strong()).truncate());
            if let Some(handle) = get_or_decode_thumbnail(
                ui.ctx(),
                &mut state.thumbnail_cache,
                material_idx,
                role,
                tex,
            ) {
                ui.add(egui::Image::new(&handle).fit_to_exact_size(preview));
            } else {
                draw_thumbnail_placeholder(ui, preview, theme);
                ui.label(
                    egui::RichText::new("decode failed")
                        .small()
                        .weak()
                        .italics(),
                );
            }

            let tooltip = tex.source_path.as_ref().map_or_else(
                || "Embedded texture (no source file)".to_string(),
                |p| p.display().to_string(),
            );
            ui.add(
                egui::Label::new(egui::RichText::new(filename_or_embedded(tex)).small()).truncate(),
            )
            .on_hover_text(tooltip);
            ui.label(
                egui::RichText::new(format!("{}×{}", tex.image.width, tex.image.height))
                    .small()
                    .weak(),
            );

            let has_path = tex.source_path.is_some();
            let btn = egui::Button::new(egui::RichText::new("Open externally").small());
            let btn = ui.add_enabled(has_path, btn);
            let btn = if has_path {
                btn
            } else {
                btn.on_disabled_hover_text("Embedded texture — no source file to open")
            };
            if btn.clicked()
                && let Some(path) = tex.source_path.as_ref()
            {
                let name = filename_only(path);
                match open::that(path) {
                    Ok(()) => state
                        .pending_toasts
                        .push((format!("Opened {name}"), ToastSeverity::Success)),
                    Err(e) => state
                        .pending_toasts
                        .push((format!("Couldn't open {name}: {e}"), ToastSeverity::Error)),
                }
            }
        },
    );
}

// ---------------------------------------------------------------------
// Thumbnail decoding + small formatters
// ---------------------------------------------------------------------

fn get_or_decode_thumbnail(
    ctx: &egui::Context,
    cache: &mut HashMap<(usize, TextureRole), egui::TextureHandle>,
    material_idx: usize,
    role: TextureRole,
    tex: &TextureThumbnail,
) -> Option<egui::TextureHandle> {
    let key = (material_idx, role);
    if let Some(handle) = cache.get(&key) {
        return Some(handle.clone());
    }
    let downscaled = downscale_to_thumbnail(&tex.image)?;
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [downscaled.width() as usize, downscaled.height() as usize],
        downscaled.as_raw(),
    );
    let name = format!("material_inspector_{material_idx}_{role:?}");
    let handle = ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR);
    cache.insert(key, handle.clone());
    Some(handle)
}

/// Decode + downscale a raw RGBA8 texture into a thumbnail. Returns
/// `None` when the buffer length does not match `width × height × 4` (a
/// corrupt or malformed source texture) so the caller can draw a
/// placeholder instead of the panel panicking inside the egui frame.
fn downscale_to_thumbnail(
    raw: &solarxy_core::RawImageData,
) -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let buffer: ImageBuffer<Rgba<u8>, _> =
        ImageBuffer::from_raw(raw.width, raw.height, raw.pixels.clone())?;
    if raw.width <= THUMBNAIL_SIZE && raw.height <= THUMBNAIL_SIZE {
        return Some(buffer);
    }
    Some(imageops::thumbnail(&buffer, THUMBNAIL_SIZE, THUMBNAIL_SIZE))
}

fn base_color_to_color32(c: [f32; 3]) -> egui::Color32 {
    let clamp = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(clamp(c[0]), clamp(c[1]), clamp(c[2]))
}

fn color_hex(c: [f32; 3]) -> String {
    let to_byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        to_byte(c[0]),
        to_byte(c[1]),
        to_byte(c[2])
    )
}

fn alpha_mode_label(mode: u32, cutoff: f32) -> String {
    match mode {
        0 => "Opaque".to_string(),
        1 => format!("Mask (cutoff = {cutoff:.2})"),
        2 => "Blend".to_string(),
        _ => "Unknown".to_string(),
    }
}

fn filename_or_embedded(tex: &TextureThumbnail) -> String {
    match tex.source_path.as_ref() {
        Some(path) => filename_only(path),
        None => "(embedded)".to_string(),
    }
}

fn filename_only(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map_or_else(|| path.display().to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_default_is_empty() {
        let s = MaterialInspectorState::default();
        assert!(s.thumbnail_cache.is_empty());
        assert_eq!(s.selected, 0);
        assert!(s.pending_toasts.is_empty());
    }

    #[test]
    fn clear_for_new_model_resets_selection_and_pending() {
        let mut s = MaterialInspectorState {
            selected: 3,
            ..Default::default()
        };
        s.pending_toasts
            .push(("test".into(), ToastSeverity::Success));
        s.clear_for_new_model();
        assert_eq!(s.selected, 0);
        assert!(s.pending_toasts.is_empty());
        assert!(s.thumbnail_cache.is_empty());
    }

    #[test]
    fn side_by_side_when_wide_and_landscape() {
        assert!(use_side_by_side(800.0, 400.0));
        assert!(use_side_by_side(360.0, 360.0));
    }

    #[test]
    fn stacked_when_tall_or_too_narrow() {
        // Taller than wide → stacked.
        assert!(!use_side_by_side(400.0, 800.0));
        // Wide aspect but the panel is simply too narrow to split.
        assert!(!use_side_by_side(300.0, 100.0));
        assert!(!use_side_by_side(359.0, 100.0));
    }

    #[test]
    fn color_hex_round_trip_for_common_values() {
        assert_eq!(color_hex([1.0, 1.0, 1.0]), "#FFFFFF");
        assert_eq!(color_hex([0.0, 0.0, 0.0]), "#000000");
        assert_eq!(color_hex([0.5, 0.5, 0.5]), "#808080");
    }

    #[test]
    fn alpha_mode_label_formats_each_variant() {
        assert_eq!(alpha_mode_label(0, 0.5), "Opaque");
        assert_eq!(alpha_mode_label(1, 0.5), "Mask (cutoff = 0.50)");
        assert_eq!(alpha_mode_label(2, 0.5), "Blend");
        assert_eq!(alpha_mode_label(99, 0.5), "Unknown");
    }

    #[test]
    fn filename_only_keeps_extension_and_strips_directory() {
        assert_eq!(
            filename_only(std::path::Path::new("/a/b/c/diffuse.png")),
            "diffuse.png"
        );
        assert_eq!(
            filename_only(std::path::Path::new("textures/wood.jpg")),
            "wood.jpg"
        );
    }

    #[test]
    fn base_color_to_color32_clamps_out_of_range() {
        let c = base_color_to_color32([1.5, -0.2, 0.7]);
        assert_eq!(c.r(), 255);
        assert_eq!(c.g(), 0);
        assert_eq!(c.b(), 179);
    }
}
