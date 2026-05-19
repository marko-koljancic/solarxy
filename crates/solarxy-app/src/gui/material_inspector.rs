//! View-only Material Inspector window.
//!
//! Toggled via `Window → Material Inspector` (menu-only — no global
//! keyboard shortcut; `M` and `Shift+M` are already bound to the
//! material override cycle, and the comprehensive-plan decision log
//! D1 ruled out a new modifier-key binding).
//!
//! Reads CPU-side data from
//! [`solarxy_renderer::model::Model::material_thumbnails`] (populated
//! during `resources::upload_model` from the source `RawMaterialData`
//! before the bytes are consumed by GPU upload). 128×128 thumbnails are
//! decoded on first inspector open per `(material_idx, role)` and stashed
//! in an egui texture cache that gets cleared on model swap.
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

const THUMBNAIL_SIZE: u32 = 128;

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
/// `MenuBarVisibility`; this struct holds the decoded egui textures and
/// the deferred toast queue (drained by `EguiRenderer::render_ui` after
/// the egui frame closes, where `push_toast` access is available).
#[derive(Default)]
pub(super) struct MaterialInspectorState {
    thumbnail_cache: HashMap<(usize, TextureRole), egui::TextureHandle>,
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
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (idx, mat) in model.materials.iter().enumerate() {
                let thumbs = model
                    .material_thumbnails
                    .get(idx)
                    .expect("material_thumbnails length matches materials");
                draw_material_row(ui, idx, mat, thumbs, state);
                ui.add_space(2.0);
            }
        });
}

fn draw_material_row(
    ui: &mut egui::Ui,
    idx: usize,
    mat: &solarxy_renderer::material::Material,
    thumbs: &MaterialThumbnails,
    state: &mut MaterialInspectorState,
) {
    let display_name = if mat.name.is_empty() {
        format!("Material {idx}")
    } else {
        mat.name.clone()
    };
    let swatch_color = base_color_to_color32(thumbs.base_color);

    let header_id = egui::Id::new(("material_inspector_header", idx));
    egui::CollapsingHeader::new(egui::RichText::new(&display_name).strong())
        .id_salt(header_id)
        .default_open(idx == 0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, swatch_color);
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                    egui::StrokeKind::Outside,
                );
                let has_albedo = thumbs.albedo.is_some();
                let (prefix, suffix) = if has_albedo {
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
            });

            egui::Grid::new(("material_grid", idx))
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Metallic");
                    ui.label(format!("{:.2}", mat.uniform.metallic_factor));
                    ui.end_row();

                    ui.label("Roughness");
                    ui.label(format!("{:.2}", mat.uniform.roughness_factor));
                    ui.end_row();

                    ui.label("Alpha mode");
                    ui.label(alpha_mode_label(
                        mat.uniform.alpha_mode,
                        mat.uniform.alpha_cutoff,
                    ));
                    ui.end_row();

                    let emissive = mat.uniform.emissive;
                    if emissive != [0.0, 0.0, 0.0] {
                        ui.label("Emissive factor");
                        ui.label(format!(
                            "({:.2}, {:.2}, {:.2})",
                            emissive[0], emissive[1], emissive[2]
                        ));
                        ui.end_row();
                    }
                });

            ui.add_space(4.0);
            let mut any_texture = false;
            for &role in TextureRole::ALL {
                if let Some(tex) = role.thumbnail_in(thumbs) {
                    any_texture = true;
                    draw_texture_row(ui, idx, role, tex, state);
                }
            }
            if !any_texture {
                ui.label(egui::RichText::new("No textures").small().weak().italics());
            }
        });
}

fn draw_texture_row(
    ui: &mut egui::Ui,
    material_idx: usize,
    role: TextureRole,
    tex: &TextureThumbnail,
    state: &mut MaterialInspectorState,
) {
    let handle = get_or_decode_thumbnail(
        ui.ctx(),
        &mut state.thumbnail_cache,
        material_idx,
        role,
        tex,
    );

    ui.separator();
    ui.label(egui::RichText::new(role.label()).strong().small());
    ui.horizontal_top(|ui| {
        let size = egui::vec2(THUMBNAIL_SIZE as f32, THUMBNAIL_SIZE as f32);
        ui.add(egui::Image::new(&handle).fit_to_exact_size(size));
        ui.vertical(|ui| {
            let label = filename_or_embedded(tex);
            let tooltip = tex.source_path.as_ref().map_or_else(
                || "Embedded texture (no source file)".to_string(),
                |p| p.display().to_string(),
            );
            ui.label(egui::RichText::new(label).small())
                .on_hover_text(tooltip);
            ui.label(
                egui::RichText::new(format!("{}×{}", tex.image.width, tex.image.height))
                    .small()
                    .weak(),
            );

            let has_path = tex.source_path.is_some();
            let btn = ui.add_enabled(has_path, egui::Button::new("Open externally"));
            let btn = if has_path {
                btn
            } else {
                btn.on_disabled_hover_text("Embedded texture — no source file to open")
            };
            if btn.clicked()
                && let Some(path) = tex.source_path.as_ref()
            {
                let label = filename_only(path);
                match open::that(path) {
                    Ok(()) => {
                        state
                            .pending_toasts
                            .push((format!("Opened {label}"), ToastSeverity::Success));
                    }
                    Err(e) => {
                        state
                            .pending_toasts
                            .push((format!("Couldn't open {label}: {e}"), ToastSeverity::Error));
                    }
                }
            }
        });
    });
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
        assert!(s.pending_toasts.is_empty());
    }

    #[test]
    fn clear_for_new_model_drops_caches_and_pending() {
        let mut s = MaterialInspectorState::default();
        s.pending_toasts
            .push(("test".into(), ToastSeverity::Success));
        s.clear_for_new_model();
        assert!(s.pending_toasts.is_empty());
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
