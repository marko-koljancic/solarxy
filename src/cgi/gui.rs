use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui_wgpu::ScreenDescriptor;

use crate::format_number;
use crate::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ToneMode, UvMapBackground, UvMode, ViewMode,
};
use crate::state::renderer::PostProcessing;
use crate::state::view_state::{BoundsMode, DisplaySettings, PaneDisplaySettings};

use super::resources::ModelStats;

struct Toast {
    message: String,
    color: [f32; 4],
    created: Instant,
    duration: Duration,
}

struct ModelInfo {
    filename: String,
    file_path: String,
    file_size: u64,
    format: String,
    mesh_count: usize,
    material_count: usize,
    stats: ModelStats,
    bounds_size: [f32; 3],
    has_uvs: bool,
}

pub struct EguiRenderer {
    ctx: egui::Context,
    winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    egui_format: wgpu::TextureFormat,
    pub sidebar_visible: bool,

    hints_visible: bool,
    toast: Option<Toast>,
    loading_message: Option<String>,
    frame_times: VecDeque<f32>,

    model_info: Option<ModelInfo>,
    backend_info: String,

    stats_visible: bool,
}

#[derive(Default)]
pub struct SidebarChanges {
    pub background_changed: bool,
    pub wireframe_params_changed: bool,
    pub composite_params_changed: bool,
    pub ibl_changed: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct GuiSnapshot {
    pub view_mode: ViewMode,
    pub normals_mode: NormalsMode,
    pub background_mode: BackgroundMode,
    pub uv_mode: UvMode,
    pub bounds_mode: BoundsMode,
    pub line_weight: LineWeight,
    pub show_grid: bool,
    pub show_axis_gizmo: bool,
    pub show_local_axes: bool,
    pub inspection_mode: InspectionMode,
    pub material_override: MaterialOverride,
    pub texel_density_target: f32,
    pub pane_mode: PaneMode,
    pub uv_bg: UvMapBackground,
    pub show_uv_overlap: bool,
    pub show_validation: bool,
    pub turntable_active: bool,
    pub turntable_rpm: f32,
    pub lights_locked: bool,
    pub roughness_scale: f32,
    pub metallic_scale: f32,
    pub bloom_enabled: bool,
    pub ssao_enabled: bool,
    pub tone_mode: ToneMode,
    pub exposure: f32,
    pub ibl_mode: IblMode,
    pub cameras_linked: bool,
    pub is_split: bool,
}

impl GuiSnapshot {
    pub fn from_state(
        pds: &PaneDisplaySettings,
        display: &DisplaySettings,
        post: &PostProcessing,
        ibl_mode: IblMode,
        cameras_linked: bool,
        is_split: bool,
    ) -> Self {
        Self {
            view_mode: pds.view_mode,
            normals_mode: pds.normals_mode,
            background_mode: pds.background_mode,
            uv_mode: pds.uv_mode,
            bounds_mode: pds.bounds_mode,
            line_weight: pds.line_weight,
            show_grid: pds.show_grid,
            show_axis_gizmo: pds.show_axis_gizmo,
            show_local_axes: pds.show_local_axes,
            inspection_mode: pds.inspection_mode,
            material_override: pds.material_override,
            texel_density_target: pds.texel_density_target,
            pane_mode: pds.pane_mode,
            uv_bg: pds.uv_bg,
            show_uv_overlap: pds.show_uv_overlap,
            show_validation: pds.show_validation,
            turntable_active: display.turntable_active,
            turntable_rpm: display.turntable_rpm,
            lights_locked: display.lights_locked,
            roughness_scale: display.roughness_scale,
            metallic_scale: display.metallic_scale,
            bloom_enabled: post.bloom_enabled,
            ssao_enabled: post.ssao_enabled,
            tone_mode: post.tone_mode,
            exposure: post.exposure,
            ibl_mode,
            cameras_linked,
            is_split,
        }
    }

    pub fn diff(&self, prev: &Self) -> SidebarChanges {
        let bg_changed = self.background_mode != prev.background_mode;
        let lw_changed = self.line_weight != prev.line_weight;
        SidebarChanges {
            background_changed: bg_changed,
            wireframe_params_changed: lw_changed && !bg_changed,
            composite_params_changed: self.bloom_enabled != prev.bloom_enabled
                || self.ssao_enabled != prev.ssao_enabled
                || self.tone_mode != prev.tone_mode
                || (self.exposure - prev.exposure).abs() > f32::EPSILON,
            ibl_changed: self.ibl_mode != prev.ibl_mode,
        }
    }

    pub fn write_back_pane(&self, pds: &mut PaneDisplaySettings) {
        pds.view_mode = self.view_mode;
        pds.normals_mode = self.normals_mode;
        pds.background_mode = self.background_mode;
        pds.uv_mode = self.uv_mode;
        pds.bounds_mode = self.bounds_mode;
        pds.line_weight = self.line_weight;
        pds.show_grid = self.show_grid;
        pds.show_axis_gizmo = self.show_axis_gizmo;
        pds.show_local_axes = self.show_local_axes;
        pds.inspection_mode = self.inspection_mode;
        pds.material_override = self.material_override;
        pds.texel_density_target = self.texel_density_target;
        pds.pane_mode = self.pane_mode;
        pds.uv_bg = self.uv_bg;
        pds.show_uv_overlap = self.show_uv_overlap;
        pds.show_validation = self.show_validation;
    }

    pub fn write_back_display(&self, display: &mut DisplaySettings) {
        display.turntable_active = self.turntable_active;
        display.turntable_rpm = self.turntable_rpm;
        display.lights_locked = self.lights_locked;
        display.roughness_scale = self.roughness_scale;
        display.metallic_scale = self.metallic_scale;
    }

    pub fn write_back_post(&self, post: &mut PostProcessing) {
        post.bloom_enabled = self.bloom_enabled;
        post.ssao_enabled = self.ssao_enabled;
        post.tone_mode = self.tone_mode;
        post.exposure = self.exposure;
    }
}

pub(crate) struct HudInfo {
    pub pane_label: String,
    pub cameras_linked: Option<bool>,
    pub has_uvs: bool,
    pub uv_overlap_pct: Option<f32>,
}

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    let panel_bg = egui::Color32::from_rgba_unmultiplied(30, 30, 40, 217);
    visuals.panel_fill = panel_bg;
    visuals.window_fill = panel_bg;

    let corner = egui::CornerRadius::same(4);
    visuals.widgets.noninteractive.corner_radius = corner;
    visuals.widgets.inactive.corner_radius = corner;
    visuals.widgets.hovered.corner_radius = corner;
    visuals.widgets.active.corner_radius = corner;
    visuals.widgets.open.corner_radius = corner;
    visuals.window_corner_radius = corner;

    let accent = egui::Color32::from_rgb(80, 200, 190);
    visuals.selection.bg_fill = accent;
    visuals.hyperlink_color = accent;
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgba_unmultiplied(80, 200, 190, 40);

    let mut style = egui::Style {
        visuals,
        ..Default::default()
    };

    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(10.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(12.0, egui::FontFamily::Monospace),
    );

    style.spacing.item_spacing = egui::vec2(6.0, 2.0);
    style.spacing.button_padding = egui::vec2(4.0, 1.0);
    style.spacing.indent = 16.0;
    style.spacing.window_margin = egui::Margin::same(4);

    ctx.set_style(style);
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "lilex".to_owned(),
        egui::FontData::from_static(include_bytes!("../../res/Lilex/static/Lilex-Medium.ttf"))
            .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "lilex".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "lilex".to_owned());
    ctx.set_fonts(fonts);
}

impl EguiRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        window: &winit::window::Window,
    ) -> Self {
        let egui_format = surface_format.remove_srgb_suffix();
        let ctx = egui::Context::default();
        let viewport_id = ctx.viewport_id();
        let winit_state =
            egui_winit::State::new(ctx.clone(), viewport_id, window, None, None, None);
        let renderer =
            egui_wgpu::Renderer::new(device, egui_format, egui_wgpu::RendererOptions::default());

        configure_fonts(&ctx);
        apply_theme(&ctx);

        Self {
            ctx,
            winit_state,
            renderer,
            egui_format,
            sidebar_visible: false,
            hints_visible: true,
            toast: None,
            loading_message: None,
            frame_times: VecDeque::with_capacity(30),
            model_info: None,
            backend_info: String::new(),
            stats_visible: false,
        }
    }

    pub fn on_window_event(
        &mut self,
        window: &winit::window::Window,
        event: &winit::event::WindowEvent,
    ) -> bool {
        self.winit_state.on_window_event(window, event).consumed
    }

    pub fn wants_pointer_input(&self) -> bool {
        self.ctx.wants_pointer_input()
    }

    pub fn wants_keyboard_input(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    pub fn set_toast(&mut self, msg: &str, color: [f32; 4]) {
        self.toast = Some(Toast {
            message: msg.to_string(),
            color,
            created: Instant::now(),
            duration: Duration::from_secs(3),
        });
    }

    pub fn set_capture_message(&mut self, filename: String) {
        self.toast = Some(Toast {
            message: format!("Saved {filename}"),
            color: [0.0, 0.4, 0.0, 1.0],
            created: Instant::now(),
            duration: Duration::from_secs(2),
        });
    }

    pub fn set_loading_message(&mut self, msg: &str) {
        self.loading_message = Some(msg.to_string());
    }

    pub fn clear_loading_message(&mut self) {
        self.loading_message = None;
    }

    pub fn clear_expired_toast(&mut self) {
        if let Some(ref toast) = self.toast
            && toast.created.elapsed() > toast.duration
        {
            self.toast = None;
        }
    }

    pub fn toggle_hints(&mut self) {
        self.hints_visible = !self.hints_visible;
    }

    pub fn set_backend_info(&mut self, info: String) {
        self.backend_info = info;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_model_info(
        &mut self,
        filename: &str,
        file_path: &str,
        file_size: u64,
        mesh_count: usize,
        material_count: usize,
        stats: &ModelStats,
        bounds_size: [f32; 3],
        has_uvs: bool,
    ) {
        let format = std::path::Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_uppercase();
        self.model_info = Some(ModelInfo {
            filename: filename.to_string(),
            file_path: file_path.to_string(),
            file_size,
            format,
            mesh_count,
            material_count,
            stats: *stats,
            bounds_size,
            has_uvs,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_ui(
        &mut self,
        mut snap: GuiSnapshot,
        hud: &HudInfo,
        validation_report: Option<&crate::validation::ValidationReport>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &winit::window::Window,
        surface_texture: &wgpu::Texture,
        screen: ScreenDescriptor,
        frame_ms: f32,
        divider_rect: Option<egui::Rect>,
        active_pane_rect: Option<egui::Rect>,
    ) -> GuiSnapshot {
        if self.frame_times.len() >= 30 {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(frame_ms);

        let raw_input = self.winit_state.take_egui_input(window);
        let sidebar_visible = self.sidebar_visible;
        let mut stats_visible = self.stats_visible;
        let has_model = self.model_info.is_some();
        let hints_visible = self.hints_visible;
        let avg_ms = self.frame_times.iter().sum::<f32>() / self.frame_times.len().max(1) as f32;
        let fps = if avg_ms > 0.0 {
            (1000.0 / avg_ms) as u32
        } else {
            0
        };
        let backend_info = &self.backend_info;
        let toast = self.toast.as_ref();
        let loading_message = self.loading_message.as_ref();
        let model_info = &self.model_info;
        let pane_label = &hud.pane_label;
        let cameras_linked = hud.cameras_linked;
        let validation_counts =
            validation_report.map_or((0, 0), |r| (r.error_count(), r.warning_count()));

        let full_output = self.ctx.run(raw_input, |ctx| {
            draw_sidebar(
                ctx,
                &mut snap,
                sidebar_visible,
                &mut stats_visible,
                has_model,
                hud.uv_overlap_pct,
                validation_report,
            );
            if let Some(info) = model_info {
                draw_stats_window(ctx, info, &mut stats_visible);
            }
            draw_hud_overlays(
                ctx,
                avg_ms,
                fps,
                toast,
                loading_message,
                has_model,
                hints_visible,
                backend_info,
                pane_label,
                cameras_linked,
                validation_counts,
            );
            if let Some(rect) = divider_rect {
                let painter = ctx.layer_painter(egui::LayerId::background());
                painter.rect_filled(rect, 0.0, egui::Color32::from_gray(40));
            }
            if let Some(rect) = active_pane_rect {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("active_pane"),
                ));
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(100, 160, 255, 120),
                    ),
                    egui::StrokeKind::Outside,
                );
            }
            if snap.pane_mode == PaneMode::UvMap && !hud.has_uvs {
                let screen_rect = ctx.input(egui::InputState::viewport_rect);
                let pane_center = active_pane_rect.unwrap_or(screen_rect).center();
                let offset = pane_center - screen_rect.center();
                egui::Area::new(egui::Id::new("no_uv_overlay"))
                    .anchor(egui::Align2::CENTER_CENTER, [offset.x, offset.y])
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        overlay_frame().show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("No UV data")
                                    .size(16.0)
                                    .color(egui::Color32::from_rgb(128, 179, 255)),
                            );
                        });
                    });
            }
        });

        self.stats_visible = stats_visible;

        self.winit_state
            .handle_platform_output(window, full_output.platform_output);

        let tris = self
            .ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }
        self.renderer
            .update_buffers(device, queue, encoder, &tris, &screen);

        let egui_view = surface_texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.egui_format),
            ..Default::default()
        });

        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &egui_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        self.renderer.render(&mut pass, &tris, &screen);
        drop(pass);

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }

        snap
    }
}

fn combo_with_tooltip<T>(ui: &mut egui::Ui, label: &str, shortcut: &str, current: &mut T, all: &[T])
where
    T: Copy + PartialEq + std::fmt::Display,
{
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(label)
            .selected_text(current.to_string())
            .width(140.0)
            .show_ui(ui, |ui| {
                for &variant in all {
                    ui.selectable_value(current, variant, variant.to_string());
                }
            });
        ui.label(label).on_hover_text(shortcut);
    });
}

fn checkbox_with_tooltip(ui: &mut egui::Ui, value: &mut bool, label: &str, shortcut: &str) {
    ui.horizontal(|ui| {
        ui.checkbox(value, label);
        ui.small(shortcut)
            .on_hover_text(format!("Shortcut: {shortcut}"));
    });
}

fn draw_sidebar(
    ctx: &egui::Context,
    s: &mut GuiSnapshot,
    visible: bool,
    stats_visible: &mut bool,
    has_model: bool,
    uv_overlap_pct: Option<f32>,
    validation_report: Option<&crate::validation::ValidationReport>,
) {
    egui::SidePanel::left("sidebar")
        .resizable(false)
        .default_width(220.0)
        .show_animated(ctx, visible, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(2.0);

                egui::CollapsingHeader::new("View")
                    .default_open(true)
                    .show(ui, |ui| {
                        if s.pane_mode == PaneMode::UvMap {
                            ui.label("UV Map");
                            combo_with_tooltip(
                                ui,
                                "Background",
                                "U",
                                &mut s.uv_bg,
                                UvMapBackground::ALL,
                            );
                            combo_with_tooltip(
                                ui,
                                "Weight",
                                "Shift+W",
                                &mut s.line_weight,
                                LineWeight::ALL,
                            );
                            checkbox_with_tooltip(ui, &mut s.show_uv_overlap, "Overlap", "O");
                            if s.show_uv_overlap
                                && let Some(pct) = uv_overlap_pct
                            {
                                ui.indent("overlap_stats", |ui| {
                                    ui.label(format!("Overlap: {:.1}%", pct));
                                });
                            }
                            if ui.small_button("Back to 3D (3)").clicked() {
                                s.pane_mode = PaneMode::Scene3D;
                            }
                        } else {
                            combo_with_tooltip(ui, "Mode", "W", &mut s.view_mode, ViewMode::ALL);
                            combo_with_tooltip(
                                ui,
                                "Inspection",
                                "1\u{2013}5",
                                &mut s.inspection_mode,
                                InspectionMode::ALL,
                            );
                            if s.inspection_mode == InspectionMode::TexelDensity {
                                ui.indent("texel_density_indent", |ui| {
                                    ui.add(
                                        egui::Slider::new(&mut s.texel_density_target, 0.01..=10.0)
                                            .logarithmic(true)
                                            .text("Target"),
                                    );
                                });
                            }
                            combo_with_tooltip(
                                ui,
                                "Material",
                                "M",
                                &mut s.material_override,
                                MaterialOverride::ALL,
                            );
                            combo_with_tooltip(
                                ui,
                                "Normals",
                                "N",
                                &mut s.normals_mode,
                                NormalsMode::ALL,
                            );
                            combo_with_tooltip(ui, "UV", "U", &mut s.uv_mode, UvMode::ALL);
                            combo_with_tooltip(
                                ui,
                                "Weight",
                                "Shift+W",
                                &mut s.line_weight,
                                LineWeight::ALL,
                            );
                            combo_with_tooltip(
                                ui,
                                "Background",
                                "B",
                                &mut s.background_mode,
                                BackgroundMode::ALL,
                            );
                            combo_with_tooltip(
                                ui,
                                "Bounds",
                                "Shift+B",
                                &mut s.bounds_mode,
                                BoundsMode::ALL,
                            );
                        }
                        if s.is_split {
                            checkbox_with_tooltip(
                                ui,
                                &mut s.cameras_linked,
                                "Link cameras",
                                "Ctrl+L",
                            );
                        }
                    });

                ui.separator();

                egui::CollapsingHeader::new("Display")
                    .default_open(true)
                    .show(ui, |ui| {
                        checkbox_with_tooltip(ui, &mut s.show_grid, "Grid", "G");
                        checkbox_with_tooltip(ui, &mut s.show_axis_gizmo, "Axis Gizmo", "A");
                        checkbox_with_tooltip(ui, &mut s.show_local_axes, "Local Axes", "Shift+A");
                        checkbox_with_tooltip(ui, &mut s.lights_locked, "Lock Lights", "Shift+L");
                        checkbox_with_tooltip(ui, &mut s.turntable_active, "Turntable", "V");
                        if s.turntable_active {
                            ui.indent("turntable_indent", |ui| {
                                ui.add(
                                    egui::Slider::new(&mut s.turntable_rpm, 1.0..=60.0)
                                        .text("RPM")
                                        .logarithmic(true),
                                );
                            });
                        }
                    });

                ui.separator();

                if let Some(report) = validation_report {
                    egui::CollapsingHeader::new("Validation")
                        .default_open(false)
                        .show(ui, |ui| {
                            checkbox_with_tooltip(
                                ui,
                                &mut s.show_validation,
                                "Highlight issues on mesh",
                                "Shift+V",
                            );
                            if report.is_clean() {
                                ui.label("No issues found");
                            } else {
                                ui.label(format!(
                                    "{} error(s), {} warning(s)",
                                    report.error_count(),
                                    report.warning_count()
                                ));
                                for issue in &report.issues {
                                    let color = crate::validation::issue_category(issue).color();
                                    let egui_color = egui::Color32::from_rgba_unmultiplied(
                                        (color[0] * 255.0) as u8,
                                        (color[1] * 255.0) as u8,
                                        (color[2] * 255.0) as u8,
                                        255,
                                    );
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(8.0, 8.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(rect.center(), 4.0, egui_color);
                                        ui.label(format!("{} {}", issue.scope, issue.message));
                                    });
                                }
                            }
                        });

                    ui.separator();
                }

                egui::CollapsingHeader::new("Post-Processing")
                    .default_open(true)
                    .show(ui, |ui| {
                        checkbox_with_tooltip(ui, &mut s.bloom_enabled, "Bloom", "Shift+D");
                        checkbox_with_tooltip(ui, &mut s.ssao_enabled, "SSAO", "Shift+O");
                        combo_with_tooltip(
                            ui,
                            "Tone Map",
                            "Shift+T",
                            &mut s.tone_mode,
                            ToneMode::ALL,
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Slider::new(&mut s.exposure, 0.1..=10.0)
                                    .text("Exposure")
                                    .logarithmic(true),
                            );
                        })
                        .response
                        .on_hover_text("E / Shift+E");
                    });

                ui.separator();

                egui::CollapsingHeader::new("Material")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(s.material_override == MaterialOverride::None, |ui| {
                            ui.add(
                                egui::Slider::new(&mut s.roughness_scale, 0.0..=2.0)
                                    .text("Roughness Scale"),
                            );
                            ui.add(
                                egui::Slider::new(&mut s.metallic_scale, 0.0..=2.0)
                                    .text("Metallic Scale"),
                            );
                            if ui.small_button("Reset").clicked() {
                                s.roughness_scale = 1.0;
                                s.metallic_scale = 1.0;
                            }
                        });
                        if s.material_override != MaterialOverride::None {
                            ui.label("(disabled in override modes)");
                        }
                    });

                ui.separator();

                egui::CollapsingHeader::new("Lighting")
                    .default_open(true)
                    .show(ui, |ui| {
                        combo_with_tooltip(ui, "IBL", "I / Shift+I", &mut s.ibl_mode, IblMode::ALL);
                    });

                ui.separator();

                if has_model {
                    ui.checkbox(stats_visible, "Model Stats");
                }

                ui.add_space(8.0);
            });
        });
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    match bytes {
        b if b >= GB => format!("{:.1} GB", b as f64 / GB as f64),
        b if b >= MB => format!("{:.1} MB", b as f64 / MB as f64),
        b if b >= KB => format!("{:.1} KB", b as f64 / KB as f64),
        b => format!("{b} B"),
    }
}

fn draw_stats_window(ctx: &egui::Context, info: &ModelInfo, open: &mut bool) {
    egui::Window::new("Model Stats")
        .open(open)
        .resizable(true)
        .collapsible(true)
        .default_pos([240.0, 60.0])
        .default_width(260.0)
        .show(ctx, |ui| {
            egui::Grid::new("stats_file")
                .num_columns(2)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    ui.label("File");
                    ui.label(&info.filename);
                    ui.end_row();

                    ui.label("Path");
                    ui.label(&info.file_path);
                    ui.end_row();

                    ui.label("Size");
                    ui.label(format_file_size(info.file_size));
                    ui.end_row();

                    ui.label("Format");
                    ui.label(&info.format);
                    ui.end_row();
                });

            ui.separator();
            ui.strong("Geometry");

            egui::Grid::new("stats_geo")
                .num_columns(2)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    ui.label("Polygons");
                    ui.label(format_number(info.stats.polys));
                    ui.end_row();

                    ui.label("Triangles");
                    ui.label(format_number(info.stats.tris));
                    ui.end_row();

                    ui.label("Vertices");
                    ui.label(format_number(info.stats.verts));
                    ui.end_row();

                    ui.label("Meshes");
                    ui.label(info.mesh_count.to_string());
                    ui.end_row();

                    ui.label("Materials");
                    ui.label(info.material_count.to_string());
                    ui.end_row();
                });

            ui.separator();
            ui.strong("Bounds");

            let [w, h, d] = info.bounds_size;
            egui::Grid::new("stats_bounds")
                .num_columns(2)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    ui.label("W \u{00d7} H \u{00d7} D");
                    ui.label(format!("{w:.3} \u{00d7} {h:.3} \u{00d7} {d:.3}"));
                    ui.end_row();
                });

            ui.separator();
            ui.strong("UV Data");

            egui::Grid::new("stats_uv")
                .num_columns(2)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    ui.label("UV Mapping");
                    ui.label(if info.has_uvs { "Yes" } else { "No" });
                    ui.end_row();

                    ui.label("Coverage");
                    ui.label("N/A");
                    ui.end_row();
                });

            ui.separator();
            ui.strong("Validation");

            egui::Grid::new("stats_val")
                .num_columns(2)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    ui.label("Status");
                    ui.label("N/A");
                    ui.end_row();
                });
        });
}

fn overlay_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(egui::Color32::from_black_alpha(160))
        .corner_radius(egui::CornerRadius::same(3))
        .inner_margin(egui::Margin::same(4))
}

#[allow(clippy::too_many_arguments)]
fn draw_hud_overlays(
    ctx: &egui::Context,
    avg_ms: f32,
    fps: u32,
    toast: Option<&Toast>,
    loading_message: Option<&String>,
    has_model: bool,
    hints_visible: bool,
    backend_info: &str,
    pane_label: &str,
    cameras_linked: Option<bool>,
    validation_counts: (usize, usize),
) {
    let screen = ctx.content_rect();
    let default_pos = egui::pos2(screen.right() - 8.0, screen.top() + 8.0);
    egui::Area::new(egui::Id::new("fps_overlay"))
        .default_pos(default_pos)
        .pivot(egui::Align2::RIGHT_TOP)
        .movable(true)
        .interactable(true)
        .constrain(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            overlay_frame().show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("{avg_ms:.1} ms  {fps} fps"))
                        .small()
                        .color(egui::Color32::from_white_alpha(200)),
                );
                if !backend_info.is_empty() {
                    ui.label(
                        egui::RichText::new(backend_info)
                            .small()
                            .color(egui::Color32::from_white_alpha(160)),
                    );
                }
                if has_model && !pane_label.is_empty() {
                    ui.label(
                        egui::RichText::new(pane_label)
                            .small()
                            .color(egui::Color32::from_white_alpha(180)),
                    );
                }
                if let Some(linked) = cameras_linked {
                    let text = if linked {
                        "Cameras: Linked"
                    } else {
                        "Cameras: Independent"
                    };
                    ui.label(
                        egui::RichText::new(text)
                            .small()
                            .color(egui::Color32::from_white_alpha(140)),
                    );
                }
                let (errors, warnings) = validation_counts;
                if errors > 0 || warnings > 0 {
                    let mut parts = Vec::new();
                    if errors > 0 {
                        parts.push(format!(
                            "\u{2715} {} error{}",
                            errors,
                            if errors == 1 { "" } else { "s" }
                        ));
                    }
                    if warnings > 0 {
                        parts.push(format!(
                            "\u{26a0} {} warning{}",
                            warnings,
                            if warnings == 1 { "" } else { "s" }
                        ));
                    }
                    let color = if errors > 0 {
                        egui::Color32::from_rgb(255, 100, 100)
                    } else {
                        egui::Color32::from_rgb(255, 200, 80)
                    };
                    ui.label(egui::RichText::new(parts.join("  ")).small().color(color));
                }
            });
        });

    if let Some(toast) = toast {
        let color = egui::Color32::from_rgba_unmultiplied(
            (toast.color[0] * 255.0) as u8,
            (toast.color[1] * 255.0) as u8,
            (toast.color[2] * 255.0) as u8,
            (toast.color[3] * 255.0) as u8,
        );
        egui::Area::new(egui::Id::new("toast_overlay"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -48.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                overlay_frame().show(ui, |ui| {
                    ui.label(egui::RichText::new(&toast.message).color(color));
                });
            });
    }

    if let Some(msg) = loading_message {
        egui::Area::new(egui::Id::new("loading_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                overlay_frame().show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(msg)
                            .size(16.0)
                            .color(egui::Color32::from_rgb(128, 179, 255)),
                    );
                });
            });
    } else if !has_model {
        egui::Area::new(egui::Id::new("drop_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                overlay_frame().show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Drop a 3D model to view\n(.obj  .stl  .ply  .gltf  .glb)",
                        )
                        .size(16.0)
                        .color(egui::Color32::from_white_alpha(140)),
                    );
                });
            });
    }

    if hints_visible {
        let hints = if has_model {
            "M Mode  1-5 Inspect  S Shaded  X Ghost  N Normals  U UV  B Bg  G Grid  A Axes  \
             I IBL  E/Shift+E Exposure\n\
             Shift+W Weight  Shift+B Bounds  Shift+D Bloom  Shift+O SSAO  Shift+T Tone  \
             Shift+I IBL Mode  Shift+V Valid\n\
             Shift+A Local Axes  Shift+L Lights  Shift+S Save  V Turn  P/O Proj  \
             C Cap  H Frame  Tab Panel  ? Hints\n\
             F1 Single  F2 V-Split  F3 H-Split  Ctrl+L Link"
        } else {
            "? Hints"
        };
        egui::Area::new(egui::Id::new("hints_overlay"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -8.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_max_width(ctx.content_rect().width().min(900.0));
                overlay_frame().show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(hints)
                            .small()
                            .color(egui::Color32::from_white_alpha(160)),
                    );
                });
            });
    }
}
