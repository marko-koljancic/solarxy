use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui_wgpu::ScreenDescriptor;

use solarxy_renderer::resources::ModelStats;
use crate::console::{ConsoleState, LogBuffer};
use crate::state::hdri_info::HdriInfo;
use solarxy_core::preferences::PaneMode;

use super::about::draw_about_modal;
use super::actions::{DividerInfo, MenuActions, MenuBarVisibility};
use super::dock::{SolarxyTab, SolarxyTabViewer, default_dock_state, tab_present, toggle_tab};
use super::keyboard_shortcuts_modal::{KeyboardShortcutsModalState, draw_keyboard_shortcuts_modal};
use super::material_inspector::MaterialInspectorState;
use super::menu::draw_menu_bar;
use super::outliner::OutlinerEvents;
use super::overlays::{HudCtx, Toast, ToastSeverity, draw_hud_overlays, overlay_frame};
use super::status_bar::{self, StatusBarData};
use super::viewport_context_menu::{ViewportContextMenu, draw_viewport_context_menu};
use super::preferences_modal::{PreferencesModal, draw_preferences_modal};
use super::review_panel::draw_delete_confirm_modal;
use super::review_popup::draw_review_popup;
use super::screenshot_modal::{ScreenshotModal, draw_screenshot_modal};
use super::properties::{ModelInfo, PropertiesEvents};
use super::snapshot::{GuiSnapshot, HudInfo};
use super::theme::{Theme, apply_theme, configure_fonts, make_dock_style};
use super::update_modal::{UpdateModalState, draw_update_modal};
use egui_dock::{DockArea, DockState};
use solarxy_core::preferences::{Preferences, ThemeChoice};

pub struct EguiRenderer {
    ctx: egui::Context,
    winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    egui_format: wgpu::TextureFormat,
    theme: Theme,
    pub menu_bar_visible: bool,
    pub status_bar_visible: bool,
    pub console: ConsoleState,
    about_open: bool,
    update_modal: UpdateModalState,
    preferences_modal: PreferencesModal,
    shortcuts_modal: KeyboardShortcutsModalState,
    screenshot_modal: ScreenshotModal,
    material_inspector: MaterialInspectorState,
    toasts: VecDeque<Toast>,
    next_toast_id: u64,
    loading_message: Option<String>,
    frame_times: VecDeque<f32>,
    model_info: Option<ModelInfo>,
    hdri_info: Option<HdriInfo>,
    backend_info: String,
    pub(super) dock_state: DockState<SolarxyTab>,
    pub last_viewport_rect: Option<CachedViewportRect>,
    pub(super) has_saved_layout: bool,
    /// Whether a node-engine scene is open. Separate from `model_info`,
    /// which describes a file-loaded model: the two roots are mutually
    /// exclusive, and File > Close acts on whichever is present.
    pub(super) scene_open: bool,
}

/// Viewport-tab geometry from the previous egui frame, tagged with the
/// surface dimensions it was captured at. Consumed next frame by
/// `state::panes` to size the wgpu render targets to the Viewport rect.
#[derive(Debug, Clone, Copy)]
pub struct CachedViewportRect {
    pub rect: egui::Rect,
    pub surface_size: (u32, u32),
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
        // The startup default; `state/init.rs` re-applies the user's
        // persisted choice once preferences have loaded. Routed through
        // `ThemeChoice::default()` rather than naming a preset, so the
        // default lives in exactly one place.
        let theme = Theme::from_choice(ThemeChoice::default());
        apply_theme(&ctx, &theme);

        Self {
            ctx,
            winit_state,
            renderer,
            egui_format,
            theme,
            menu_bar_visible: true,
            status_bar_visible: true,
            console: ConsoleState::new(console_buffer),
            about_open: false,
            update_modal: UpdateModalState::new(),
            preferences_modal: PreferencesModal::default(),
            shortcuts_modal: KeyboardShortcutsModalState::default(),
            screenshot_modal: ScreenshotModal::default(),
            material_inspector: MaterialInspectorState::default(),
            toasts: VecDeque::with_capacity(Self::TOAST_QUEUE_CAP),
            next_toast_id: 0,
            loading_message: None,
            frame_times: VecDeque::with_capacity(30),
            model_info: None,
            hdri_info: None,
            backend_info: String::new(),
            dock_state: default_dock_state(),
            last_viewport_rect: None,
            has_saved_layout: false,
            scene_open: false,
        }
    }

    /// Swap the active interface theme and re-push it into the egui
    /// context. Called at startup with the persisted choice and again on
    /// every Preferences commit (only when the choice actually changed).
    pub fn apply_theme_choice(&mut self, choice: ThemeChoice) {
        self.theme = Theme::from_choice(choice);
        apply_theme(&self.ctx, &self.theme);
    }

    /// Drop the cached model info on model close. Panel visibility is
    /// left untouched — panels are user-controlled (no auto open/close).
    /// The HDRI is independent of the model, so `hdri_info` is kept.
    pub fn clear_model_info(&mut self) {
        self.model_info = None;
        self.material_inspector.clear_for_new_model();
    }

    /// Cache the loaded HDRI's metadata for the Properties panel.
    pub(crate) fn update_hdri_info(&mut self, info: HdriInfo) {
        self.hdri_info = Some(info);
    }

    /// Drop the cached HDRI metadata when the HDRI is cleared.
    pub(crate) fn clear_hdri_info(&mut self) {
        self.hdri_info = None;
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

    /// `true` while any combo / menu / popup is open — the camera input
    /// gate uses this so a click inside an open pane-toolbar dropdown
    /// doesn't also orbit the scene.
    pub fn any_popup_open(&self) -> bool {
        egui::Popup::is_any_open(&self.ctx)
    }

    pub fn wants_keyboard_input(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    const TOAST_QUEUE_CAP: usize = 5;

    fn push_toast(&mut self, severity: ToastSeverity, message: String, duration: Duration) {
        match severity {
            ToastSeverity::Error => {
                tracing::error!(target: "solarxy::toast", "{message}");
            }
            ToastSeverity::Warning => {
                tracing::warn!(target: "solarxy::toast", "{message}");
            }
            ToastSeverity::Info | ToastSeverity::Success => {
                tracing::info!(target: "solarxy::toast", "{message}");
            }
        }
        self.next_toast_id = self.next_toast_id.wrapping_add(1);
        if self.toasts.len() >= Self::TOAST_QUEUE_CAP {
            self.toasts.pop_front();
        }
        self.toasts.push_back(Toast {
            id: self.next_toast_id,
            message,
            severity,
            created: Instant::now(),
            duration,
        });
    }

    pub fn set_toast(&mut self, msg: &str, severity: ToastSeverity) {
        self.push_toast(severity, msg.to_string(), Duration::from_secs(5));
    }

    pub fn set_capture_message(&mut self, filename: String) {
        self.push_toast(
            ToastSeverity::Success,
            format!("Saved {filename}"),
            Duration::from_secs(2),
        );
    }

    pub fn set_loading_message(&mut self, msg: &str) {
        self.loading_message = Some(msg.to_string());
    }

    pub fn clear_loading_message(&mut self) {
        self.loading_message = None;
    }

    pub fn clear_expired_toasts(&mut self) {
        let now = Instant::now();
        self.toasts
            .retain(|t| now.duration_since(t.created) < t.duration);
    }

    pub fn open_shortcuts_modal(&mut self) {
        self.shortcuts_modal.open = true;
    }

    pub fn toggle_sidebar_tab(&mut self) {
        toggle_tab(&mut self.dock_state, SolarxyTab::Sidebar);
    }

    pub fn toggle_console_tab(&mut self) {
        toggle_tab(&mut self.dock_state, SolarxyTab::Console);
    }

    pub fn toggle_viewport_tab(&mut self) {
        toggle_tab(&mut self.dock_state, SolarxyTab::Viewport);
    }

    #[must_use]
    pub fn cursor_in_viewport(&self, cursor_logical: egui::Pos2) -> bool {
        self.last_viewport_rect
            .is_none_or(|c| c.rect.contains(cursor_logical))
    }

    #[must_use]
    pub fn viewport_rect_for_surface(&self, surface_size: (u32, u32)) -> Option<egui::Rect> {
        self.last_viewport_rect
            .and_then(|c| (c.surface_size == surface_size).then_some(c.rect))
    }

    pub fn invalidate_viewport_rect(&mut self) {
        self.last_viewport_rect = None;
    }

    /// `true` iff the Viewport tab is currently mounted in the dock. The
    /// state layer gates the 3D render pass on this so a hidden Viewport
    /// doesn't burn GPU work behind opaque docked panels.
    #[must_use]
    pub fn viewport_tab_present(&self) -> bool {
        tab_present(&self.dock_state, SolarxyTab::Viewport)
    }

    /// Apply a JSON-serialized dock layout. Returns `true` if the JSON
    /// deserialized into a valid `DockState`; on failure, the existing
    /// layout is preserved and a debug line is logged.
    pub fn apply_layout_json(&mut self, json: &str) -> bool {
        match serde_json::from_str::<DockState<SolarxyTab>>(json) {
            Ok(state) => {
                self.dock_state = state;
                true
            }
            Err(err) => {
                tracing::debug!("dock layout JSON rejected (falling back): {err}");
                false
            }
        }
    }

    /// Serialize the current dock layout to a JSON string. Returns `None`
    /// if `serde_json` rejects the state (shouldn't happen for the upstream
    /// `DockState<SolarxyTab>` impl, but we treat it as best-effort).
    #[must_use]
    pub fn serialize_layout(&self) -> Option<String> {
        serde_json::to_string(&self.dock_state).ok()
    }

    /// Replace the current dock layout with the factory default produced
    /// by the crate-private `default_dock_state` constructor.
    pub fn reset_dock_layout(&mut self) {
        self.dock_state = default_dock_state();
    }

    pub fn set_scene_open(&mut self, open: bool) {
        self.scene_open = open;
    }

    pub fn set_has_saved_layout(&mut self, has: bool) {
        self.has_saved_layout = has;
    }

    #[must_use]
    pub fn any_blocking_modal_open(&self, review: &crate::state::review::ReviewState) -> bool {
        self.about_open
            || self.preferences_modal.open
            || self.update_modal.open
            || self.shortcuts_modal.open
            || review.delete_confirm.is_some()
            || review.editing.is_some()
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
            scene: None,
        });
    }

    /// The scene equivalent of [`Self::update_model_info`]: the same panel
    /// slot, filled from summed object counters rather than from one
    /// loaded file.
    ///
    /// Called on every drained scene delta, not once at open, because a
    /// cook changes the counts.
    pub(crate) fn update_scene_info(
        &mut self,
        filename: &str,
        path: &str,
        file_size: u64,
        counts: crate::state::engine_scene::SceneGeometryCounts,
        bounds_size: [f32; 3],
    ) {
        self.model_info = Some(ModelInfo {
            filename: filename.to_string(),
            file_path: path.to_string(),
            file_size,
            format: "SLXY".to_string(),
            mesh_count: counts.meshes,
            material_count: counts.materials,
            // Drawn totals. `polys` has no meaning for cooked geometry, so
            // it stays zero and the panel drops its row rather than
            // printing the triangle count twice.
            stats: ModelStats {
                polys: 0,
                tris: counts.drawn_tris,
                verts: counts.drawn_verts,
            },
            bounds_size,
            has_uvs: counts.has_uvs,
            scene: Some(counts),
        });
    }

    /// Drop the Material Inspector's per-model thumbnail cache and reset
    /// its selection. Must be called alongside [`Self::update_model_info`]
    /// on every model load: the cache is keyed by `(material_index,
    /// texture_role)`, so without this a stale `TextureHandle` from the
    /// previous model would be served for the new model's matching slot
    /// (and the old handles would leak until app exit).
    pub(crate) fn reset_material_inspector(&mut self) {
        self.material_inspector.clear_for_new_model();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_ui(
        &mut self,
        mut snap: GuiSnapshot,
        hud: &HudInfo,
        validation: super::properties::ValidationView<'_>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &winit::window::Window,
        surface_texture: &wgpu::Texture,
        screen: ScreenDescriptor,
        frame_ms: f32,
        divider: Option<DividerInfo>,
        active_pane_rect: Option<egui::Rect>,
        review_panes: &[super::ReviewPaneOverlay],
        recent_files: &[String],
        review: &mut crate::state::review::ReviewState,
        // The file-loaded model, when one is open. Still separate from
        // `outliner_source` because the Material Inspector and the review
        // overlay are file-model surfaces and stay that way.
        model: Option<&solarxy_renderer::model::Model>,
        outliner_source: super::outliner::OutlinerSource<'_>,
        pane_toolbar: super::pane_toolbar::PaneToolbarData<'_>,
        properties_events: &mut PropertiesEvents,
        outliner_events: &mut OutlinerEvents,
        viewport_context_menu: &mut Option<ViewportContextMenu>,
        force_expand_review: bool,
        suppress_screenshot_modal: bool,
    ) -> (GuiSnapshot, MenuActions) {
        if self.frame_times.len() >= 30 {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(frame_ms);

        let raw_input = self.winit_state.take_egui_input(window);
        let has_model = self.model_info.is_some() || self.scene_open;
        let avg_ms = self.frame_times.iter().sum::<f32>() / self.frame_times.len().max(1) as f32;
        let fps = if avg_ms > 0.0 {
            (1000.0 / avg_ms) as u32
        } else {
            0
        };
        let backend_info = &self.backend_info;
        let toasts = &self.toasts;
        let loading_message = self.loading_message.as_ref();
        let model_info = &self.model_info;
        let hdri_info = &self.hdri_info;
        let pane_label = &hud.pane_label;
        let cameras_linked = hud.cameras_linked;
        let validation_counts = validation
            .report
            .map_or((0, 0), |r| (r.error_count(), r.warning_count()));

        let mut actions = MenuActions::default();

        if review.panel_open != tab_present(&self.dock_state, SolarxyTab::ReviewPanel) {
            toggle_tab(&mut self.dock_state, SolarxyTab::ReviewPanel);
        }

        let present_at_start: std::collections::HashSet<SolarxyTab> =
            self.dock_state.iter_all_tabs().map(|(_, t)| *t).collect();
        let mut menu_vis = MenuBarVisibility {
            sidebar_visible: present_at_start.contains(&SolarxyTab::Sidebar),
            outliner_visible: present_at_start.contains(&SolarxyTab::Outliner),
            menu_bar_visible: self.menu_bar_visible,
            properties_visible: present_at_start.contains(&SolarxyTab::Properties),
            status_bar_visible: self.status_bar_visible,
            console_visible: present_at_start.contains(&SolarxyTab::Console),
            review_panel_visible: present_at_start.contains(&SolarxyTab::ReviewPanel),
            material_inspector_visible: present_at_start.contains(&SolarxyTab::MaterialInspector),
            viewport_visible: present_at_start.contains(&SolarxyTab::Viewport),
            has_saved_layout: self.has_saved_layout,
        };
        let menu_vis_before = menu_vis;
        let mut about_open = self.about_open;
        let mut dismissed_toast_id: Option<u64> = None;
        let console = &mut self.console;
        let update_modal = &mut self.update_modal;
        let preferences_modal = &mut self.preferences_modal;
        let screenshot_modal = &mut self.screenshot_modal;
        let shortcuts_modal = &mut self.shortcuts_modal;
        let material_inspector = &mut self.material_inspector;
        let dock_state = &mut self.dock_state;
        let theme = self.theme;
        // Destructured here so the egui closure (an `FnMut`) captures the
        // individual borrows — it rebuilds a fresh `PaneToolbarData` each
        // run rather than moving a captured owned value out.
        let super::pane_toolbar::PaneToolbarData {
            rects: pt_rects,
            active: pt_active,
            pane_settings: pt_pane_settings,
            projections: pt_projections,
            projection_change: pt_projection_change,
            hdri_available: pt_hdri_available,
            customs: pt_customs,
            uv_overlap_pct: pt_uv_overlap_pct,
        } = pane_toolbar;
        let mut viewport_rect_logical: Option<egui::Rect> = None;

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
                    pt_hdri_available,
                    pt_customs,
                    review.active,
                    review.markers_hidden,
                    review.dirty,
                    theme,
                );
            }

            if menu_vis.status_bar_visible {
                let status = status_bar::draw(
                    ctx,
                    &StatusBarData {
                        model: model_info
                            .as_ref()
                            .map(|m| (m.filename.as_str(), m.format.as_str())),
                        validation: validation_counts,
                        review_active: review.active,
                        pane_label,
                        cameras_linked,
                        avg_ms,
                        fps,
                        backend: backend_info,
                    },
                    theme,
                );
                if status.review_badge_clicked {
                    review.toggle_active();
                    actions.exit_review_mode = true;
                }
            }

            let mut tab_viewer = SolarxyTabViewer {
                snap: &mut snap,
                review,
                console,
                model,
                outliner_source,
                model_info: model_info.as_ref(),
                hdri_info: hdri_info.as_ref(),
                validation,
                properties_events,
                outliner_events,
                material_inspector,
                viewport_rect_out: &mut viewport_rect_logical,
                theme,
                pane_toolbar: super::pane_toolbar::PaneToolbarData {
                    rects: pt_rects,
                    active: pt_active,
                    pane_settings: pt_pane_settings,
                    projections: pt_projections,
                    projection_change: pt_projection_change,
                    hdri_available: pt_hdri_available,
                    customs: pt_customs,
                    uv_overlap_pct: pt_uv_overlap_pct,
                },
            };
            DockArea::new(dock_state)
                .style(make_dock_style(ctx, &theme))
                .show(ctx, &mut tab_viewer);

            // The screenshot modal counts as a blocking overlay only on
            // frames it is actually drawn — during a re-capture frame it
            // is suppressed so the markers it would occlude get captured.
            let screenshot_drawn = screenshot_modal.open && !suppress_screenshot_modal;
            let suppress_overlay = about_open
                || preferences_modal.open
                || update_modal.open
                || shortcuts_modal.open
                || screenshot_drawn
                || review.delete_confirm.is_some()
                || review.editing.is_some();
            // `markers_hidden` suppresses the 3D overlay while the panel
            // keeps listing every annotation.
            let suppress_markers = suppress_overlay || review.markers_hidden;
            super::review_overlay::draw_review_overlay(
                ctx,
                review_panes,
                review,
                suppress_markers,
                theme,
                model,
                force_expand_review,
            );

            draw_about_modal(ctx, &mut about_open);
            draw_update_modal(ctx, update_modal);
            draw_preferences_modal(ctx, preferences_modal);
            draw_keyboard_shortcuts_modal(ctx, shortcuts_modal);
            if !suppress_screenshot_modal {
                draw_screenshot_modal(ctx, screenshot_modal, &theme);
            }

            draw_delete_confirm_modal(ctx, review);
            draw_review_popup(ctx, review);

            // Viewport right-click context menu — painted on top; its Esc
            // consume runs before the review-mode Esc chain below.
            let menu_outcome = viewport_context_menu
                .as_mut()
                .map(|menu| draw_viewport_context_menu(ctx, menu));
            if let Some(outcome) = menu_outcome {
                if let Some(act) = outcome.action {
                    outliner_events.action = Some(act);
                }
                if outcome.close {
                    *viewport_context_menu = None;
                }
            }

            if review.active {
                let stripe = egui::Color32::from_rgba_unmultiplied(
                    theme.accent.r(),
                    theme.accent.g(),
                    theme.accent.b(),
                    0xB0,
                );
                let stripe_painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("solarxy_review_mode_edge_stripe"),
                ));
                stripe_painter.rect_stroke(
                    ctx.content_rect(),
                    egui::CornerRadius::ZERO,
                    egui::Stroke::new(3.0_f32, stripe),
                    egui::StrokeKind::Inside,
                );

                if review.reanchor_target.is_none() {
                    let amber_bg =
                        egui::Color32::from_rgba_unmultiplied(0x4A, 0x37, 0x0E, 0xCC);
                    let amber_fg = theme.accent;
                    egui::Area::new(egui::Id::new("solarxy_review_mode_banner"))
                        .anchor(egui::Align2::CENTER_TOP, [0.0, 16.0])
                        .order(egui::Order::Foreground)
                        .interactable(false)
                        .show(ctx, |ui| {
                            egui::Frame::NONE
                                .fill(amber_bg)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(12, 6))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "Review Mode \u{2014} click to add a note, Shift+R to exit",
                                        )
                                        .color(amber_fg),
                                    );
                                });
                        });
                }
            }

            if let Some(target_id) = review.reanchor_target.clone() {
                let preview = review
                    .find(&target_id)
                    .map_or_else(|| "annotation".to_string(), |a| {
                        crate::state::review::short_text_preview(&a.text)
                    });
                let amber_bg = egui::Color32::from_rgba_unmultiplied(0x4A, 0x37, 0x0E, 0xE6);
                let amber_fg = theme.accent;
                egui::Area::new(egui::Id::new("solarxy_reanchor_banner"))
                    .anchor(egui::Align2::CENTER_TOP, [0.0, 16.0])
                    .order(egui::Order::Foreground)
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::NONE
                            .fill(amber_bg)
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Re-anchoring \u{201C}{preview}\u{201D} \u{2014} click on the model to re-place. Esc to cancel."
                                    ))
                                    .color(amber_fg),
                                );
                            });
                    });
                ctx.request_repaint();
            }

            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                if review.reanchor_target.is_some() {
                    review.cancel_reanchor();
                    actions.cancel_reanchor = true;
                } else if review.active {
                    review.toggle_active();
                    actions.exit_review_mode = true;
                }
            }
            let hud_ctx = HudCtx {
                toasts,
                loading_message,
                overdraw_active: hud.overdraw_active,
            };
            let hud_result = draw_hud_overlays(ctx, &hud_ctx);
            if let Some(id) = hud_result.dismissed_toast_id {
                dismissed_toast_id = Some(id);
            }
            if let Some(div) = divider {
                let painter = ctx.layer_painter(egui::LayerId::background());
                painter.rect_filled(div.visible, 0.0, theme.border);
                let resp = egui::Area::new(egui::Id::new("solarxy_divider_drag"))
                    .fixed_pos(div.hit.min)
                    .order(egui::Order::Foreground)
                    .interactable(true)
                    .show(ctx, |ui| {
                        ui.allocate_exact_size(div.hit.size(), egui::Sense::click_and_drag())
                    })
                    .inner
                    .1;

                if resp.hovered() || resp.dragged() {
                    ctx.set_cursor_icon(match div.layout {
                        solarxy_core::view_config::ViewLayout::SplitVertical => {
                            egui::CursorIcon::ResizeHorizontal
                        }
                        solarxy_core::view_config::ViewLayout::SplitHorizontal => {
                            egui::CursorIcon::ResizeVertical
                        }
                        _ => egui::CursorIcon::Default,
                    });
                }
                if resp.dragged()
                    && let Some(pos) = resp.interact_pointer_pos()
                {
                    let viewport = viewport_rect_logical
                        .unwrap_or_else(|| ctx.input(egui::InputState::viewport_rect));
                    let raw_ratio = match div.layout {
                        solarxy_core::view_config::ViewLayout::SplitVertical => {
                            (pos.x - viewport.left()) / viewport.width().max(1.0)
                        }
                        solarxy_core::view_config::ViewLayout::SplitHorizontal => {
                            (pos.y - viewport.top()) / viewport.height().max(1.0)
                        }
                        _ => 0.5,
                    };
                    actions.set_split_ratio = Some(
                        solarxy_core::view_config::DisplaySettings::clamp_split_ratio(raw_ratio),
                    );
                }
                if resp.double_clicked() {
                    actions.set_split_ratio =
                        Some(solarxy_core::view_config::DisplaySettings::DEFAULT_SPLIT_RATIO);
                }
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

        self.menu_bar_visible = menu_vis.menu_bar_visible;
        self.status_bar_visible = menu_vis.status_bar_visible;

        let menu_intents: [(SolarxyTab, bool, bool); 7] = [
            (
                SolarxyTab::Viewport,
                menu_vis_before.viewport_visible,
                menu_vis.viewport_visible,
            ),
            (
                SolarxyTab::Sidebar,
                menu_vis_before.sidebar_visible,
                menu_vis.sidebar_visible,
            ),
            (
                SolarxyTab::Outliner,
                menu_vis_before.outliner_visible,
                menu_vis.outliner_visible,
            ),
            (
                SolarxyTab::Console,
                menu_vis_before.console_visible,
                menu_vis.console_visible,
            ),
            (
                SolarxyTab::ReviewPanel,
                menu_vis_before.review_panel_visible,
                menu_vis.review_panel_visible,
            ),
            (
                SolarxyTab::MaterialInspector,
                menu_vis_before.material_inspector_visible,
                menu_vis.material_inspector_visible,
            ),
            (
                SolarxyTab::Properties,
                menu_vis_before.properties_visible,
                menu_vis.properties_visible,
            ),
        ];
        for &(tab, before, after) in &menu_intents {
            if before != after {
                toggle_tab(&mut self.dock_state, tab);
            }
        }

        let present_after: std::collections::HashSet<SolarxyTab> =
            self.dock_state.iter_all_tabs().map(|(_, t)| *t).collect();

        self.console.visible = present_after.contains(&SolarxyTab::Console);
        review.panel_open = present_after.contains(&SolarxyTab::ReviewPanel);

        if let Some(rect) = viewport_rect_logical {
            self.last_viewport_rect = Some(CachedViewportRect {
                rect,
                surface_size: (screen.size_in_pixels[0], screen.size_in_pixels[1]),
            });
        }

        let pending = std::mem::take(&mut self.material_inspector.pending_toasts);
        for (msg, severity) in pending {
            self.push_toast(severity, msg, Duration::from_secs(5));
        }
        self.about_open = about_open;
        if let Some(id) = dismissed_toast_id {
            self.toasts.retain(|t| t.id != id);
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

    pub fn open_preferences(&mut self, prefs: Preferences) {
        self.preferences_modal.open_with(prefs);
    }

    pub fn take_committed_prefs(&mut self) -> Option<Preferences> {
        self.preferences_modal.take_committed()
    }

    /// Install a fresh screenshot capture and open the modal.
    pub fn set_screenshot_capture(
        &mut self,
        image: image::RgbaImage,
        filename: String,
        review_available: bool,
        expand_review: bool,
    ) {
        self.screenshot_modal
            .set_capture(image, filename, review_available, expand_review);
    }

    /// Drain a pending re-capture request from the screenshot modal,
    /// returning the desired expand-review setting.
    pub fn take_screenshot_recapture(&mut self) -> Option<bool> {
        self.screenshot_modal.take_recapture()
    }

    /// Drain a pending `Save As…` request from the screenshot modal.
    pub fn take_screenshot_save_request(&mut self) -> bool {
        self.screenshot_modal.take_save_request()
    }

    /// The screenshot modal's suggested file name (pre-fills the native
    /// save dialog).
    pub fn screenshot_suggested_filename(&self) -> String {
        self.screenshot_modal.suggested_filename().to_string()
    }

    /// Take the captured screenshot image out and close the modal.
    pub fn take_screenshot_image(&mut self) -> Option<image::RgbaImage> {
        self.screenshot_modal.take_image()
    }
}
