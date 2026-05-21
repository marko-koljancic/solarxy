//! View-only Material Inspector window.
//!
//! Toggled via `Window → Material Inspector` (menu-only — no global
//! keyboard shortcut; `M` and `Shift+M` are already bound to the
//! material override cycle, and the comprehensive-plan decision log
//! D1 ruled out a new modifier-key binding).
//!
//! Materials are laid out as **responsive cards** in a
//! `horizontal_wrapped` flow: a narrow dock collapses to a single
//! column, a wide dock wraps into a grid. Each card is collapsed by
//! default (header + base-color swatch + a 5-slot texture strip) and
//! expands in place to the full scalar grid + 128 px texture entries.
//!
//! Reads CPU-side data from
//! [`solarxy_renderer::model::Model::material_thumbnails`] (populated
//! during `resources::upload_model` from the source `RawMaterialData`
//! before the bytes are consumed by GPU upload). 128×128 thumbnails are
//! decoded on first inspector open per `(material_idx, role)` and stashed
//! in an egui texture cache that gets cleared on model swap — the small
//! strip thumbs reuse the same handle, downscaled by the GPU.
//!
//! Visibility is the canonical
//! `MenuBarVisibility.material_inspector_visible` flag threaded in as
//! `&mut bool` — same pattern as Review Panel + Console. The window's X
//! button writes through that flag.
//!
//! "Open externally" uses the existing `open` workspace dep
//! (`open::that(path)`); disabled when the source path is `None` (i.e.
//! embedded glTF textures with no on-disk file).

use std::collections::{HashMap, HashSet};

use image::{ImageBuffer, Rgba, imageops};
use solarxy_renderer::model::{MaterialThumbnails, Model, TextureThumbnail};

use super::overlays::ToastSeverity;
use super::theme::Theme;

/// Decode resolution for cached thumbnails — also the expanded-card
/// display size. The collapsed strip reuses the same handle downscaled.
const THUMBNAIL_SIZE: u32 = 128;
/// Texture preview edge length in an expanded card.
const EXPANDED_THUMB: f32 = 128.0;
/// Largest a collapsed-strip thumbnail is allowed to grow to. Narrower
/// cards shrink the strip below this to fit five slots in one row.
const STRIP_THUMB_MAX: f32 = 40.0;
/// Card width band. The flow packs as many `CARD_MIN_WIDTH` columns as
/// fit, then distributes slack evenly until a card would exceed
/// `CARD_MAX_WIDTH`.
const CARD_MIN_WIDTH: f32 = 220.0;
const CARD_MAX_WIDTH: f32 = 280.0;
/// Card `Frame` inner padding, per side.
const CARD_PAD: i8 = 8;

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
/// per-card expanded set, and the deferred toast queue (drained by
/// `EguiRenderer::render_ui` after the egui frame closes, where
/// `push_toast` access is available).
#[derive(Default)]
pub(super) struct MaterialInspectorState {
    thumbnail_cache: HashMap<(usize, TextureRole), egui::TextureHandle>,
    /// Material indices currently expanded to the detailed card view.
    /// Session-only; cleared on model swap. Empty = every card collapsed.
    expanded: HashSet<usize>,
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
        self.expanded.clear();
        self.pending_toasts.clear();
    }
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

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let card_width =
                    card_layout_width(ui.available_width(), ui.spacing().item_spacing.x);
                for (idx, mat) in model.materials.iter().enumerate() {
                    let Some(thumbs) = model.material_thumbnails.get(idx) else {
                        continue;
                    };
                    draw_material_card(ui, idx, mat, thumbs, state, theme, card_width);
                }
            });
        });
}

/// Even card width for an available row width, clamped to the card size
/// band. The column count is the most columns of at least
/// [`CARD_MIN_WIDTH`] that fit; the slack is distributed evenly so a row
/// fills edge-to-edge until cards would exceed [`CARD_MAX_WIDTH`], after
/// which the flow simply wraps with left-over space on the right.
fn card_layout_width(avail: f32, gap: f32) -> f32 {
    let columns = ((avail + gap) / (CARD_MIN_WIDTH + gap)).floor().max(1.0);
    let even = (avail - gap * (columns - 1.0)) / columns;
    even.clamp(CARD_MIN_WIDTH, CARD_MAX_WIDTH)
}

fn draw_material_card(
    ui: &mut egui::Ui,
    idx: usize,
    mat: &solarxy_renderer::material::Material,
    thumbs: &MaterialThumbnails,
    state: &mut MaterialInspectorState,
    theme: &Theme,
    card_width: f32,
) {
    let expanded = state.expanded.contains(&idx);
    let content_width = card_width - 2.0 * f32::from(CARD_PAD);

    egui::Frame::NONE
        .fill(theme.bg_elevated)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .inner_margin(egui::Margin::same(CARD_PAD))
        .show(ui, |ui| {
            ui.set_width(content_width);
            ui.spacing_mut().item_spacing.y = 4.0;

            if draw_card_header(ui, idx, mat, thumbs, theme, expanded) {
                if expanded {
                    state.expanded.remove(&idx);
                } else {
                    state.expanded.insert(idx);
                }
            }

            if expanded {
                draw_card_expanded(ui, idx, mat, thumbs, state);
            } else {
                draw_card_strip(ui, idx, thumbs, state, theme);
            }
        });
}

/// Clickable card header: collapse triangle, base-color swatch, name.
/// Returns `true` if the header was clicked this frame (toggle request).
fn draw_card_header(
    ui: &mut egui::Ui,
    idx: usize,
    mat: &solarxy_renderer::material::Material,
    thumbs: &MaterialThumbnails,
    theme: &Theme,
    expanded: bool,
) -> bool {
    let display_name = if mat.name.is_empty() {
        format!("Material {idx}")
    } else {
        mat.name.clone()
    };
    let swatch = base_color_to_color32(thumbs.base_color);

    // Reserve a shape slot so the hover background paints *behind* the
    // header content (egui draws shapes in insertion order).
    let bg_idx = ui.painter().add(egui::Shape::Noop);

    let row = ui.horizontal(|ui| {
        let (tri_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        paint_triangle(ui.painter(), tri_rect, expanded, theme.muted);

        let (sw_rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(sw_rect, 0.0, swatch);
        ui.painter().rect_stroke(
            sw_rect,
            0.0,
            egui::Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );

        ui.add(egui::Label::new(egui::RichText::new(&display_name).strong()).truncate());
    });

    // Extend the click target across the whole card width so the empty
    // space right of a short name still toggles the card.
    let row_rect = row.response.rect;
    let full_rect = egui::Rect::from_min_max(
        row_rect.min,
        egui::pos2(
            row_rect.min.x + ui.available_width().max(row_rect.width()),
            row_rect.max.y,
        ),
    );
    let resp = ui.interact(
        full_rect,
        ui.id().with(("mat_card_header", idx)),
        egui::Sense::click(),
    );
    if resp.hovered() {
        ui.painter().set(
            bg_idx,
            egui::Shape::rect_filled(full_rect, 0.0, theme.widget_hover),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Paint egui's collapsing-triangle as a font-independent polygon:
/// right-pointing when collapsed, down-pointing when expanded.
fn paint_triangle(painter: &egui::Painter, rect: egui::Rect, expanded: bool, color: egui::Color32) {
    let r = rect.shrink(1.0);
    let points = if expanded {
        vec![
            egui::pos2(r.left(), r.top()),
            egui::pos2(r.right(), r.top()),
            egui::pos2(r.center().x, r.bottom()),
        ]
    } else {
        vec![
            egui::pos2(r.left(), r.top()),
            egui::pos2(r.left(), r.bottom()),
            egui::pos2(r.right(), r.center().y),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

/// Collapsed-card body: a single row of five texture slots.
fn draw_card_strip(
    ui: &mut egui::Ui,
    idx: usize,
    thumbs: &MaterialThumbnails,
    state: &mut MaterialInspectorState,
    theme: &Theme,
) {
    const GAP: f32 = 3.0;
    let size = ((ui.available_width() - GAP * 4.0) / 5.0).clamp(20.0, STRIP_THUMB_MAX);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for &role in TextureRole::ALL {
            draw_strip_thumb(ui, idx, role, thumbs, state, theme, size);
        }
    });
}

fn draw_strip_thumb(
    ui: &mut egui::Ui,
    material_idx: usize,
    role: TextureRole,
    thumbs: &MaterialThumbnails,
    state: &mut MaterialInspectorState,
    theme: &Theme,
    size: f32,
) {
    let sz = egui::vec2(size, size);
    if let Some(tex) = role.thumbnail_in(thumbs) {
        let handle = get_or_decode_thumbnail(
            ui.ctx(),
            &mut state.thumbnail_cache,
            material_idx,
            role,
            tex,
        );
        let resp = ui.add(egui::Image::new(&handle).fit_to_exact_size(sz));
        resp.on_hover_ui(|ui| {
            ui.label(egui::RichText::new(role.label()).strong());
            ui.label(
                egui::RichText::new(format!("{}×{}", tex.image.width, tex.image.height)).weak(),
            );
            ui.add(
                egui::Image::new(&handle)
                    .fit_to_exact_size(egui::vec2(EXPANDED_THUMB, EXPANDED_THUMB)),
            );
        });
    } else {
        // Absent slot: a recessed box with a diagonal "empty" slash.
        let (rect, resp) = ui.allocate_exact_size(sz, egui::Sense::hover());
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
        resp.on_hover_text(format!("{}: not present", role.label()));
    }
}

/// Expanded-card body: base-color line, scalar PBR grid, and one entry
/// per texture role (present roles greyed when absent).
fn draw_card_expanded(
    ui: &mut egui::Ui,
    idx: usize,
    mat: &solarxy_renderer::material::Material,
    thumbs: &MaterialThumbnails,
    state: &mut MaterialInspectorState,
) {
    ui.add_space(2.0);
    draw_base_color_line(ui, thumbs);
    ui.add_space(2.0);

    egui::Grid::new(("material_card_grid", idx))
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Metallic").small());
            ui.label(
                egui::RichText::new(format!("{:.2}", mat.uniform.metallic_factor))
                    .small()
                    .monospace(),
            );
            ui.end_row();

            ui.label(egui::RichText::new("Roughness").small());
            ui.label(
                egui::RichText::new(format!("{:.2}", mat.uniform.roughness_factor))
                    .small()
                    .monospace(),
            );
            ui.end_row();

            ui.label(egui::RichText::new("Alpha mode").small());
            ui.label(
                egui::RichText::new(alpha_mode_label(
                    mat.uniform.alpha_mode,
                    mat.uniform.alpha_cutoff,
                ))
                .small(),
            );
            ui.end_row();

            let emissive = mat.uniform.emissive;
            if emissive != [0.0, 0.0, 0.0] {
                ui.label(egui::RichText::new("Emissive factor").small());
                ui.label(
                    egui::RichText::new(format!(
                        "({:.2}, {:.2}, {:.2})",
                        emissive[0], emissive[1], emissive[2]
                    ))
                    .small()
                    .monospace(),
                );
                ui.end_row();
            }
        });

    ui.add_space(2.0);
    for &role in TextureRole::ALL {
        draw_texture_entry(ui, idx, role, thumbs, state);
    }
}

fn draw_base_color_line(ui: &mut egui::Ui, thumbs: &MaterialThumbnails) {
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

/// One texture role in an expanded card: 128 px preview + filename +
/// dimensions + "Open externally" when present, a greyed stub otherwise.
fn draw_texture_entry(
    ui: &mut egui::Ui,
    material_idx: usize,
    role: TextureRole,
    thumbs: &MaterialThumbnails,
    state: &mut MaterialInspectorState,
) {
    ui.separator();
    let Some(tex) = role.thumbnail_in(thumbs) else {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(role.label()).small().weak());
            ui.label(
                egui::RichText::new("— not present")
                    .small()
                    .weak()
                    .italics(),
            );
        });
        return;
    };

    ui.label(egui::RichText::new(role.label()).small().strong());

    let handle = get_or_decode_thumbnail(
        ui.ctx(),
        &mut state.thumbnail_cache,
        material_idx,
        role,
        tex,
    );
    ui.add(egui::Image::new(&handle).fit_to_exact_size(egui::vec2(EXPANDED_THUMB, EXPANDED_THUMB)));

    let label = filename_or_embedded(tex);
    let tooltip = tex.source_path.as_ref().map_or_else(
        || "Embedded texture (no source file)".to_string(),
        |p| p.display().to_string(),
    );
    ui.add(egui::Label::new(egui::RichText::new(label).small()).truncate())
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
            Ok(()) => {
                state
                    .pending_toasts
                    .push((format!("Opened {name}"), ToastSeverity::Success));
            }
            Err(e) => {
                state
                    .pending_toasts
                    .push((format!("Couldn't open {name}: {e}"), ToastSeverity::Error));
            }
        }
    }
}

fn get_or_decode_thumbnail(
    ctx: &egui::Context,
    cache: &mut HashMap<(usize, TextureRole), egui::TextureHandle>,
    material_idx: usize,
    role: TextureRole,
    tex: &TextureThumbnail,
) -> egui::TextureHandle {
    let key = (material_idx, role);
    if let Some(handle) = cache.get(&key) {
        return handle.clone();
    }
    let downscaled = downscale_to_thumbnail(&tex.image);
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [downscaled.width() as usize, downscaled.height() as usize],
        downscaled.as_raw(),
    );
    let name = format!("material_inspector_{material_idx}_{role:?}");
    let handle = ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR);
    cache.insert(key, handle.clone());
    handle
}

fn downscale_to_thumbnail(raw: &solarxy_core::RawImageData) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let buffer: ImageBuffer<Rgba<u8>, _> =
        ImageBuffer::from_raw(raw.width, raw.height, raw.pixels.clone())
            .expect("RawImageData length must match width × height × 4");
    if raw.width <= THUMBNAIL_SIZE && raw.height <= THUMBNAIL_SIZE {
        return buffer;
    }
    imageops::thumbnail(&buffer, THUMBNAIL_SIZE, THUMBNAIL_SIZE)
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
        assert!(s.expanded.is_empty());
        assert!(s.pending_toasts.is_empty());
    }

    #[test]
    fn clear_for_new_model_drops_caches_and_pending() {
        let mut s = MaterialInspectorState::default();
        s.expanded.insert(0);
        s.pending_toasts
            .push(("test".into(), ToastSeverity::Success));
        s.clear_for_new_model();
        assert!(s.expanded.is_empty());
        assert!(s.pending_toasts.is_empty());
    }

    #[test]
    fn card_width_clamps_up_on_narrow_dock() {
        // One column, dock narrower than a minimum card → floor is the
        // minimum width (card overflows slightly rather than shrinking).
        assert!((card_layout_width(200.0, 4.0) - CARD_MIN_WIDTH).abs() < 0.01);
    }

    #[test]
    fn card_width_clamps_down_to_max() {
        // Wide enough for only one column but wider than the max card →
        // the card caps at CARD_MAX_WIDTH, leaving slack on the right.
        assert!((card_layout_width(400.0, 4.0) - CARD_MAX_WIDTH).abs() < 0.01);
    }

    #[test]
    fn card_width_fills_evenly_between_bounds() {
        // 460 px, 4 px gap: two columns of (460-4)/2 = 228, inside band.
        assert!((card_layout_width(460.0, 4.0) - 228.0).abs() < 0.01);
        // 900 px, 4 px gap: four columns of (900-12)/4 = 222.
        assert!((card_layout_width(900.0, 4.0) - 222.0).abs() < 0.01);
    }

    #[test]
    fn card_width_never_below_minimum() {
        for avail in [50.0_f32, 120.0, 219.0, 444.0, 1000.0, 3000.0] {
            assert!(card_layout_width(avail, 4.0) >= CARD_MIN_WIDTH);
            assert!(card_layout_width(avail, 4.0) <= CARD_MAX_WIDTH);
        }
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
