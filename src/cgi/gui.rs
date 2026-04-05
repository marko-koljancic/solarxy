use egui_wgpu::ScreenDescriptor;

use crate::preferences::{BackgroundMode, IblMode, LineWeight, NormalsMode, ToneMode, UvMode, ViewMode};
use crate::state::BoundsMode;

pub struct EguiRenderer {
    ctx: egui::Context,
    winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    egui_format: wgpu::TextureFormat,
    pub sidebar_visible: bool,
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

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_with_sidebar(
        &mut self,
        sidebar: &mut SidebarState,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &winit::window::Window,
        surface_texture: &wgpu::Texture,
        screen: ScreenDescriptor,
    ) -> SidebarChanges {
        let prev_bg = *sidebar.background_mode;
        let prev_line_weight = *sidebar.line_weight;
        let prev_bloom = *sidebar.bloom_enabled;
        let prev_ssao = *sidebar.ssao_enabled;
        let prev_tone = *sidebar.tone_mode;
        let prev_exposure = *sidebar.exposure;
        let prev_ibl = *sidebar.ibl_mode;

        let raw_input = self.winit_state.take_egui_input(window);
        let sidebar_visible = self.sidebar_visible;
        let full_output = self.ctx.run(raw_input, |ctx| {
            draw_sidebar(ctx, sidebar, sidebar_visible);
        });

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

fn draw_sidebar(ctx: &egui::Context, s: &mut SidebarState, visible: bool) {
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

                ui.add_space(8.0);
            });
        });
}
