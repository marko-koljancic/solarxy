use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui_wgpu::ScreenDescriptor;

use crate::format_number;
use crate::preferences::{BackgroundMode, IblMode, LineWeight, NormalsMode, ToneMode, UvMode, ViewMode};
use crate::state::BoundsMode;

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

pub(crate) struct SidebarState<'a> {
    pub view_mode: &'a mut ViewMode,
    pub normals_mode: &'a mut NormalsMode,
    pub background_mode: &'a mut BackgroundMode,
    pub uv_mode: &'a mut UvMode,
    pub bounds_mode: &'a mut BoundsMode,
    pub line_weight: &'a mut LineWeight,
    pub show_grid: &'a mut bool,
    pub show_axis_gizmo: &'a mut bool,
    pub show_local_axes: &'a mut bool,
    pub turntable_active: &'a mut bool,
    pub turntable_rpm: &'a mut f32,
    pub lights_locked: &'a mut bool,
    pub bloom_enabled: &'a mut bool,
    pub ssao_enabled: &'a mut bool,
    pub tone_mode: &'a mut ToneMode,
    pub exposure: &'a mut f32,
    pub ibl_mode: &'a mut IblMode,
}

#[derive(Default)]
pub struct SidebarChanges {
    pub background_changed: bool,
    pub wireframe_params_changed: bool,
    pub composite_params_changed: bool,
    pub ibl_changed: bool,
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
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(11.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(13.0, egui::FontFamily::Monospace),
    );

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
        sidebar: &mut SidebarState,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &winit::window::Window,
        surface_texture: &wgpu::Texture,
        screen: ScreenDescriptor,
        frame_ms: f32,
    ) -> SidebarChanges {
        let prev_bg = *sidebar.background_mode;
        let prev_line_weight = *sidebar.line_weight;
        let prev_bloom = *sidebar.bloom_enabled;
        let prev_ssao = *sidebar.ssao_enabled;
        let prev_tone = *sidebar.tone_mode;
        let prev_exposure = *sidebar.exposure;
        let prev_ibl = *sidebar.ibl_mode;

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

        let full_output = self.ctx.run(raw_input, |ctx| {
            draw_sidebar(ctx, sidebar, sidebar_visible, &mut stats_visible, has_model);
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
            );
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

        let bg_changed = *sidebar.background_mode != prev_bg;
        let lw_changed = *sidebar.line_weight != prev_line_weight;
        SidebarChanges {
            background_changed: bg_changed,
            wireframe_params_changed: lw_changed && !bg_changed,
            composite_params_changed: *sidebar.bloom_enabled != prev_bloom
                || *sidebar.ssao_enabled != prev_ssao
                || *sidebar.tone_mode != prev_tone
                || (*sidebar.exposure - prev_exposure).abs() > f32::EPSILON,
            ibl_changed: *sidebar.ibl_mode != prev_ibl,
        }
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
    s: &mut SidebarState,
    visible: bool,
    stats_visible: &mut bool,
    has_model: bool,
) {
    egui::SidePanel::left("sidebar")
        .resizable(false)
        .default_width(220.0)
        .show_animated(ctx, visible, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                ui.heading("Solarxy");
                ui.separator();

                egui::CollapsingHeader::new("View")
                    .default_open(true)
                    .show(ui, |ui| {
                        combo_with_tooltip(ui, "Mode", "W", s.view_mode, ViewMode::ALL);
                        combo_with_tooltip(ui, "Normals", "N", s.normals_mode, NormalsMode::ALL);
                        combo_with_tooltip(ui, "UV", "U", s.uv_mode, UvMode::ALL);
                        combo_with_tooltip(ui, "Weight", "Shift+W", s.line_weight, LineWeight::ALL);
                        combo_with_tooltip(
                            ui,
                            "Background",
                            "B",
                            s.background_mode,
                            BackgroundMode::ALL,
                        );
                        combo_with_tooltip(ui, "Bounds", "Shift+B", s.bounds_mode, BoundsMode::ALL);
                    });

                ui.separator();

                egui::CollapsingHeader::new("Display")
                    .default_open(true)
                    .show(ui, |ui| {
                        checkbox_with_tooltip(ui, s.show_grid, "Grid", "G");
                        checkbox_with_tooltip(ui, s.show_axis_gizmo, "Axis Gizmo", "A");
                        checkbox_with_tooltip(ui, s.show_local_axes, "Local Axes", "Shift+A");
                        checkbox_with_tooltip(ui, s.lights_locked, "Lock Lights", "Shift+L");
                        checkbox_with_tooltip(ui, s.turntable_active, "Turntable", "V");
                        if *s.turntable_active {
                            ui.indent("turntable_indent", |ui| {
                                ui.add(
                                    egui::Slider::new(s.turntable_rpm, 1.0..=60.0)
                                        .text("RPM")
                                        .logarithmic(true),
                                );
                            });
                        }
                    });

                ui.separator();

                egui::CollapsingHeader::new("Post-Processing")
                    .default_open(true)
                    .show(ui, |ui| {
                        checkbox_with_tooltip(ui, s.bloom_enabled, "Bloom", "Shift+M");
                        checkbox_with_tooltip(ui, s.ssao_enabled, "SSAO", "Shift+O");
                        combo_with_tooltip(ui, "Tone Map", "Shift+T", s.tone_mode, ToneMode::ALL);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Slider::new(s.exposure, 0.1..=10.0)
                                    .text("Exposure")
                                    .logarithmic(true),
                            );
                        })
                        .response
                        .on_hover_text("E / Shift+E");
                    });

                ui.separator();

                egui::CollapsingHeader::new("Lighting")
                    .default_open(true)
                    .show(ui, |ui| {
                        combo_with_tooltip(ui, "IBL", "I / Shift+I", s.ibl_mode, IblMode::ALL);
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
                .spacing([12.0, 4.0])
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
                .spacing([12.0, 4.0])
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
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label("W \u{00d7} H \u{00d7} D");
                    ui.label(format!("{w:.3} \u{00d7} {h:.3} \u{00d7} {d:.3}"));
                    ui.end_row();
                });

            ui.separator();
            ui.strong("UV Data");

            egui::Grid::new("stats_uv")
                .num_columns(2)
                .spacing([12.0, 4.0])
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
                .spacing([12.0, 4.0])
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
) {
    egui::Area::new(egui::Id::new("fps_overlay"))
        .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
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
            "W Mode  S Shaded  X Ghost  N Normals  U UV  B Bg  G Grid  A Axes  \
             I IBL  E/Shift+E Exposure\n\
             Shift+W Weight  Shift+B Bounds  Shift+M Bloom  Shift+O SSAO  Shift+T Tone  \
             Shift+I IBL Mode\n\
             Shift+A Local Axes  Shift+L Lights  Shift+S Save  V Turn  P/O Proj  \
             C Cap  H Frame  Tab Panel  ? Hints"
        } else {
            "? Hints"
        };
        egui::Area::new(egui::Id::new("hints_overlay"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -8.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let available = ui.available_width().min(900.0);
                ui.set_max_width(available);
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
