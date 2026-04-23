use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui_wgpu::ScreenDescriptor;

use solarxy_renderer::resources::ModelStats;
use crate::console::{ConsoleState, LogBuffer};
use solarxy_core::preferences::PaneMode;

use super::about::draw_about_modal;
use super::actions::{MenuActions, MenuBarVisibility};
use super::console_view::{draw_console_docked, draw_console_floating};
use super::menu::draw_menu_bar;
use super::overlays::{HudCtx, Toast, ToastSeverity, draw_hud_overlays, overlay_frame};
use super::preferences_modal::{PreferencesModal, draw_preferences_modal};
use super::sidebar::draw_sidebar;
use super::snapshot::{GuiSnapshot, HudInfo};
use super::stats::{ModelInfo, draw_stats_window};
use super::theme::{apply_theme, configure_fonts};
use super::update_modal::{UpdateModalState, draw_update_modal};
use solarxy_core::preferences::Preferences;

pub struct EguiRenderer {
    ctx: egui::Context,
    winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    egui_format: wgpu::TextureFormat,
    pub sidebar_visible: bool,
    pub menu_bar_visible: bool,
    hints_visible: bool,
    fps_hud_visible: bool,
    pub console: ConsoleState,
    about_open: bool,
    update_modal: UpdateModalState,
    preferences_modal: PreferencesModal,
    toasts: VecDeque<Toast>,
    loading_message: Option<String>,
    frame_times: VecDeque<f32>,
    model_info: Option<ModelInfo>,
    backend_info: String,
    stats_visible: bool,
    stats_user_hidden: bool,
}

impl EguiRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        window: &winit::window::Window,
        console_buffer: LogBuffer,
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
            menu_bar_visible: true,
            hints_visible: false,
            fps_hud_visible: false,
            console: ConsoleState::new(console_buffer),
            about_open: false,
            update_modal: UpdateModalState::new(),
            preferences_modal: PreferencesModal::default(),
            toasts: VecDeque::with_capacity(Self::TOAST_QUEUE_CAP),
            loading_message: None,
            frame_times: VecDeque::with_capacity(30),
            model_info: None,
            backend_info: String::new(),
            stats_visible: false,
            stats_user_hidden: false,
        }
    }

    pub fn clear_model_info(&mut self) {
        self.model_info = None;
        self.stats_visible = false;
        self.stats_user_hidden = false;
    }

    pub fn notify_model_loaded(&mut self) {
        if !self.stats_user_hidden {
            self.stats_visible = true;
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

    /// Upper bound on queued toasts. A burst larger than this drops the
    /// oldest — "most recent is most relevant" matches user expectation.
    const TOAST_QUEUE_CAP: usize = 5;

    fn push_toast(&mut self, toast: Toast) {
        if self.toasts.len() >= Self::TOAST_QUEUE_CAP {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    pub fn set_toast(&mut self, msg: &str, severity: ToastSeverity) {
        self.push_toast(Toast {
            message: msg.to_string(),
            severity,
            created: Instant::now(),
            duration: Duration::from_secs(3),
        });
    }

    pub fn set_capture_message(&mut self, filename: String) {
        self.push_toast(Toast {
            message: format!("Saved {filename}"),
            severity: ToastSeverity::Success,
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
        while let Some(front) = self.toasts.front()
            && front.created.elapsed() > front.duration
        {
            self.toasts.pop_front();
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
        validation_report: Option<&solarxy_core::validation::ValidationReport>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &winit::window::Window,
        surface_texture: &wgpu::Texture,
        screen: ScreenDescriptor,
        frame_ms: f32,
        divider_rect: Option<egui::Rect>,
        active_pane_rect: Option<egui::Rect>,
        recent_files: &[String],
    ) -> (GuiSnapshot, MenuActions) {
        if self.frame_times.len() >= 30 {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(frame_ms);

        let raw_input = self.winit_state.take_egui_input(window);
        let has_model = self.model_info.is_some();
        let avg_ms = self.frame_times.iter().sum::<f32>() / self.frame_times.len().max(1) as f32;
        let fps = if avg_ms > 0.0 {
            (1000.0 / avg_ms) as u32
        } else {
            0
        };
        let backend_info = &self.backend_info;
        let toast = self.toasts.front();
        let loading_message = self.loading_message.as_ref();
        let model_info = &self.model_info;
        let pane_label = &hud.pane_label;
        let cameras_linked = hud.cameras_linked;
        let validation_counts =
            validation_report.map_or((0, 0), |r| (r.error_count(), r.warning_count()));

        let mut actions = MenuActions::default();
        let stats_visible_before = self.stats_visible;
        let mut menu_vis = MenuBarVisibility {
            sidebar_visible: self.sidebar_visible,
            menu_bar_visible: self.menu_bar_visible,
            stats_visible: self.stats_visible,
            hints_visible: self.hints_visible,
            fps_hud_visible: self.fps_hud_visible,
            console_visible: self.console.visible,
        };
        let mut about_open = self.about_open;
        let mut toast_dismissed = false;
        let console = &mut self.console;
        let update_modal = &mut self.update_modal;
        let preferences_modal = &mut self.preferences_modal;

        let full_output = self.ctx.run(raw_input, |ctx| {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
                actions.open_preferences = true;
            }
            if menu_vis.menu_bar_visible {
                draw_menu_bar(
                    ctx,
                    &mut snap,
                    &mut actions,
                    &mut menu_vis,
                    has_model,
                    recent_files,
                );
            }
            if console.docked {
                draw_console_docked(ctx, console, &mut menu_vis.console_visible);
            }
            draw_sidebar(
                ctx,
                &mut snap,
                menu_vis.sidebar_visible,
                hud.uv_overlap_pct,
                validation_report,
            );
            if let Some(info) = model_info {
                draw_stats_window(ctx, info, &mut menu_vis.stats_visible);
            }
            if !console.docked && menu_vis.console_visible {
                draw_console_floating(ctx, console, &mut menu_vis.console_visible);
            }
            draw_about_modal(ctx, &mut about_open);
            draw_update_modal(ctx, update_modal);
            draw_preferences_modal(ctx, preferences_modal);
            let hud_ctx = HudCtx {
                avg_ms,
                fps,
                toast,
                loading_message,
                has_model,
                hints_visible: menu_vis.hints_visible,
                fps_hud_visible: menu_vis.fps_hud_visible,
                backend_info,
                pane_label,
                cameras_linked,
                validation_counts,
            };
            let hud_result = draw_hud_overlays(ctx, &hud_ctx);
            if hud_result.toast_dismissed {
                toast_dismissed = true;
            }
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

        self.sidebar_visible = menu_vis.sidebar_visible;
        self.menu_bar_visible = menu_vis.menu_bar_visible;
        self.stats_visible = menu_vis.stats_visible;
        if stats_visible_before && !self.stats_visible {
            self.stats_user_hidden = true;
        } else if !stats_visible_before && self.stats_visible {
            self.stats_user_hidden = false;
        }
        self.hints_visible = menu_vis.hints_visible;
        self.fps_hud_visible = menu_vis.fps_hud_visible;
        self.console.visible = menu_vis.console_visible;
        self.about_open = about_open;
        if toast_dismissed {
            self.toasts.pop_front();
        }

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

        (snap, actions)
    }

    pub fn open_about(&mut self) {
        self.about_open = true;
    }

    pub fn check_for_updates(&mut self) {
        self.update_modal.refresh();
    }

    /// Open the preferences modal with `prefs` as the starting draft + snapshot.
    pub fn open_preferences(&mut self, prefs: Preferences) {
        self.preferences_modal.open_with(prefs);
    }

    /// Drain the modal's "committed this frame" slot. The caller writes the
    /// returned `Preferences` into its in-memory copy; `save()` has already
    /// been performed by the modal.
    pub fn take_committed_prefs(&mut self) -> Option<Preferences> {
        self.preferences_modal.take_committed()
    }
}
