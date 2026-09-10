use std::collections::VecDeque;
use std::time::{Duration, Instant};

use solarxy_renderer::resources::ModelStats;
use crate::console::{ConsoleState, LogBuffer};
use crate::state::hdri_info::HdriInfo;
use solarxy_core::preferences::PaneMode;

use super::modals::about::draw_about_modal;
use super::dock::{SolarxyTab, SolarxyTabViewer, default_dock_state, tab_present, toggle_tab};
use super::modals::shortcuts::{KeyboardShortcutsModalState, draw_keyboard_shortcuts_modal};
use super::intent::{Intent, Intents, LayoutIntent, ReviewIntent};
use super::panels::node_tree::NodeTreeState;
use super::chrome::menu::{MenuContext, draw_menu_bar};
use super::chrome::overlays::{HudCtx, Toast, ToastSeverity, draw_hud_overlays, overlay_frame};
use super::chrome::status_bar::{self, StatusBarData};
use super::chrome::viewport_context_menu::{ViewportContextMenu, draw_viewport_context_menu};
use super::modals::preferences::{PreferencesModal, draw_preferences_modal};
use super::panels::review::panel::draw_delete_confirm_modal;
use super::panels::review::popup::draw_review_popup;
use super::modals::screenshot::{ScreenshotModal, draw_screenshot_modal};
use super::modals::still::{StillRenderModal, draw_still_modal};
use super::panels::properties::ModelInfo;
use super::theme::{Theme, apply_theme, configure_fonts, make_dock_style};
use super::modals::update::{UpdateModalState, draw_update_modal};
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
    still_modal: StillRenderModal,
    node_tree: NodeTreeState,
    canvas: super::panels::nodes::CanvasState,
    params: super::panels::params::ParamPanelState,
    /// The node canvas's rect as the last frame drew it, so a key claim
    /// made before the interface pass can ask where the pointer is.
    pub(super) canvas_rect: Option<egui::Rect>,
    /// Which graph the user is looking at. Shared by the Node Tree and the
    /// canvas, because a dive is one fact about the session: two copies of
    /// it would let the two panels disagree about where the user is, and a
    /// dropped model would land wherever the stale one said.
    graph_ctx: solarxy_graph::document::GraphContext,
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
            still_modal: StillRenderModal::default(),
            node_tree: NodeTreeState::default(),
            canvas: super::panels::nodes::CanvasState::default(),
            params: super::panels::params::ParamPanelState::default(),
            canvas_rect: None,
            graph_ctx: solarxy_graph::document::GraphContext::Root,
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

    /// Drop the cached document info on close. Panel visibility is left
    /// untouched: panels are user-controlled, with no auto open or close. The
    /// HDRI is independent of the document, so `hdri_info` is kept.
    pub fn clear_model_info(&mut self) {
        self.model_info = None;
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

    /// Show a panel if it is hidden, hide it if it is shown.
    ///
    /// One method rather than one per panel: the Window menu, the panel
    /// shortcuts and a panel's own close button all mean the same thing, and
    /// three of these existed with one of them never called.
    pub(crate) fn toggle_tab(&mut self, tab: SolarxyTab) {
        toggle_tab(&mut self.dock_state, tab);
    }

    /// Whether a panel is currently mounted anywhere in the dock.
    #[must_use]
    pub(crate) fn tab_present(&self, tab: SolarxyTab) -> bool {
        tab_present(&self.dock_state, tab)
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
        self.tab_present(SolarxyTab::Viewport)
    }

    /// `true` iff the Node Tree tab is currently mounted in the dock. The
    /// state layer gates the tree fold on this, so a closed panel costs
    /// nothing per frame. Read before the egui pass, so opening the tab
    /// from the Window menu populates it on the following frame — a
    /// latency no one can see.
    #[must_use]
    pub fn node_tree_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::NodeTree)
    }

    /// `true` iff the node canvas tab is currently mounted in the dock.
    /// The state layer gates the canvas source on this, so a closed panel
    /// costs nothing per frame.
    #[must_use]
    pub fn nodes_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::Nodes)
    }

    /// `true` iff the parameter panel is mounted, so the state layer can
    /// skip gathering what it would draw.
    #[must_use]
    pub fn params_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::Parameters)
    }

    /// The node the parameter panel is pinned to, if any.
    #[must_use]
    pub fn params_pin(&self) -> Option<solarxy_graph::document::NodeId> {
        self.params.pinned()
    }

    /// Whether the pointer is over the node canvas, which is the one
    /// thing a key claim decides by position rather than by focus.
    ///
    /// Read from the dock's own rect for the tab rather than mirrored,
    /// so a panel dragged elsewhere keeps its keys with it.
    #[must_use]
    pub fn pointer_over_canvas(&self) -> bool {
        self.canvas_rect
            .zip(self.ctx.pointer_latest_pos())
            .is_some_and(|(rect, at)| rect.contains(at))
    }

    /// Return every graph surface to the root context, unfolded and
    /// unseeded. Called whenever the open document is replaced: all of it
    /// addresses nodes the new document need not contain.
    pub fn reset_graph_surfaces(&mut self) {
        self.graph_ctx = solarxy_graph::document::GraphContext::Root;
        self.node_tree.reset();
        self.canvas.reset();
        // The pin especially: node ids are minted per document, so one
        // carried across an open would point at whatever holds that id in
        // the incoming scene.
        self.params.reset();
    }

    /// The graph the user is looking at: where a selection made in either
    /// graph surface lives, and where a dropped model lands.
    pub fn graph_ctx(&self) -> solarxy_graph::document::GraphContext {
        self.graph_ctx
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

    /// Draw one interface pass.
    ///
    /// The context handle is cloned before the pass so the closure can borrow
    /// the renderer mutably. It is a cheap handle over a shared pointer, and
    /// without the clone the whole renderer has to be taken apart into
    /// individual field borrows first, which is what fifteen lines here used
    /// to do and why the pane toolbars were destructured and rebuilt.
    pub(crate) fn render_ui(
        &mut self,
        frame: super::pass::FramePaint<'_>,
        chrome: super::pass::ViewportChrome<'_>,
        sources: super::pass::PanelSources<'_>,
        review: &mut crate::state::review::ReviewState,
        // Everything the panels ask for this pass. Raised during it, applied
        // once it is over, and never cleared inside it: the pass can run
        // twice, and clearing per pass would drop what the first one raised.
        intents: &mut Intents,
        viewport_context_menu: &mut Option<ViewportContextMenu>,
        capture: super::pass::CaptureFrame,
    ) {
        if self.frame_times.len() >= 30 {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(frame.frame_ms);

        let raw_input = self.winit_state.take_egui_input(frame.window);
        let avg_ms = self.frame_times.iter().sum::<f32>() / self.frame_times.len().max(1) as f32;
        let fps = if avg_ms > 0.0 {
            (1000.0 / avg_ms) as u32
        } else {
            0
        };
        let validation_counts = sources
            .validation
            .report
            .map_or((0, 0), |r| (r.error_count(), r.warning_count()));

        // The review panel's open flag is written by the state layer when
        // review mode starts, so it is reconciled into the dock before the
        // pass; the reverse direction is synced after the drain, where the
        // toggles it raises have already landed.
        if review.panel_open != tab_present(&self.dock_state, SolarxyTab::ReviewPanel) {
            toggle_tab(&mut self.dock_state, SolarxyTab::ReviewPanel);
        }

        // Read from the dock rather than mirrored into a struct: the Window
        // menu's ticks are a question about the dock, and mirroring them was
        // what made a panel cost a field in four places.
        let present_at_start: std::collections::HashSet<SolarxyTab> =
            self.dock_state.iter_all_tabs().map(|(_, t)| *t).collect();
        let menu_cx = MenuContext {
            // The still renders either root: an open scene, or an open model
            // through the synthesized document.
            has_model: self.model_info.is_some() || self.scene_open,
            still_renderable: self.scene_open || self.model_info.is_some(),
            recent_files: sources.recent_files,
            hdri_available: chrome.toolbars.hdri_available,
            customs: chrome.toolbars.customs,
            // **Review is off for this release.** It anchors against a
            // file-loaded model's meshes, and the second root that held one
            // went away with the one-document-root change; repointing it at
            // the engine's own review store is its own piece of work. The
            // menu entries stay visible and disabled rather than vanishing,
            // so the capability reads as absent rather than as never having
            // existed.
            review_available: false,
            review_active: review.active,
            review_markers_hidden: review.markers_hidden,
            review_dirty: review.dirty,
            menu_bar_visible: self.menu_bar_visible,
            status_bar_visible: self.status_bar_visible,
            has_saved_layout: self.has_saved_layout,
            theme: self.theme,
        };
        let mut viewport_rect_logical: Option<egui::Rect> = None;
        let mut canvas_rect_seen: Option<egui::Rect> = None;
        let mut dismissed_toast_id: Option<u64> = None;

        // Cloned so the closure below can borrow the renderer mutably: the
        // context is a handle over a shared pointer, and `run` only needs a
        // shared borrow of it.
        let ctx_handle = self.ctx.clone();
        let full_output = ctx_handle.run(raw_input, |ctx| {

            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
                intents.raise(Intent::Edit(super::EditIntent::OpenPreferences));
            }
            if self.menu_bar_visible {
                draw_menu_bar(
                    ctx,
                    sources.settings,
                    intents,
                    &|tab| present_at_start.contains(&tab),
                    menu_cx,
                );
            }

            if self.status_bar_visible {
                let status = status_bar::draw(
                    ctx,
                    &StatusBarData {
                        model: self
                            .model_info
                            .as_ref()
                            .map(|m| (m.filename.as_str(), m.format.as_str())),
                        validation: validation_counts,
                        review_active: review.active,
                        pane_label: &sources.hud.pane_label,
                        cameras_linked: sources.hud.cameras_linked,
                        avg_ms,
                        fps,
                        backend: &self.backend_info,
                        still: self.still_modal.running_progress(),
                    },
                    self.theme,
                );
                if status.review_badge_clicked {
                    review.toggle_active();
                    intents.raise(Intent::Review(ReviewIntent::Exited));
                }
            }

            let mut tab_viewer = SolarxyTabViewer {
                sources,
                open_file: super::pass::OpenFile {
                    model_info: self.model_info.as_ref(),
                    hdri_info: self.hdri_info.as_ref(),
                },
                panels: super::pass::PanelState {
                    console: &mut self.console,
                    node_tree: &mut self.node_tree,
                    canvas: &mut self.canvas,
                    params: &mut self.params,
                    graph_ctx: &mut self.graph_ctx,
                },
                review,
                // Reborrowed rather than moved: the queue outlives the tab
                // viewer, and the context menu below raises into it.
                intents: &mut *intents,
                toolbars: &chrome.toolbars,
                viewport_rect_out: &mut viewport_rect_logical,
                canvas_rect_out: &mut canvas_rect_seen,
                theme: self.theme,
            };
            DockArea::new(&mut self.dock_state)
                .style(make_dock_style(ctx, &self.theme))
                .show(ctx, &mut tab_viewer);

            // The screenshot modal counts as a blocking overlay only on
            // frames it is actually drawn — during a re-capture frame it
            // is suppressed so the markers it would occlude get captured.
            let screenshot_drawn = self.screenshot_modal.open && !capture.capturing;
            let suppress_overlay = self.about_open
                || self.preferences_modal.open
                || self.update_modal.open
                || self.shortcuts_modal.open
                || screenshot_drawn
                || review.delete_confirm.is_some()
                || review.editing.is_some();
            // `markers_hidden` suppresses the 3D overlay while the panel
            // keeps listing every annotation.
            let suppress_markers = suppress_overlay || review.markers_hidden;
            super::panels::review::overlay::draw_review_overlay(
                ctx,
                chrome.review_panes,
                review,
                suppress_markers,
                self.theme,
                capture.expand_review,
            );

            draw_about_modal(ctx, &mut self.about_open);
            draw_update_modal(ctx, &mut self.update_modal);
            draw_preferences_modal(ctx, &mut self.preferences_modal);
            draw_keyboard_shortcuts_modal(ctx, &mut self.shortcuts_modal);
            if !capture.capturing {
                draw_screenshot_modal(ctx, &mut self.screenshot_modal, &self.theme);
            }
            // Drawn ahead of the escape chain below: while a render runs,
            // Escape cancels it before it dismisses anything else.
            draw_still_modal(ctx, &mut self.still_modal, &self.theme);

            draw_delete_confirm_modal(ctx, review);
            draw_review_popup(ctx, review);

            // Viewport right-click context menu — painted on top; its Esc
            // consume runs before the review-mode Esc chain below.
            let menu_outcome = viewport_context_menu
                .as_mut()
                .map(|menu| draw_viewport_context_menu(ctx, menu));
            if let Some(outcome) = menu_outcome {
                if let Some(act) = outcome.action {
                    intents.raise(Intent::Viewport(act));
                }
                if outcome.close {
                    *viewport_context_menu = None;
                }
            }

            if review.active {
                let stripe = egui::Color32::from_rgba_unmultiplied(
                    self.theme.accent.r(),
                    self.theme.accent.g(),
                    self.theme.accent.b(),
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
                    let amber_fg = self.theme.accent;
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
                let amber_fg = self.theme.accent;
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
                    intents.raise(Intent::Review(ReviewIntent::ReanchorCancelled));
                } else if review.active {
                    review.toggle_active();
                    intents.raise(Intent::Review(ReviewIntent::Exited));
                }
            }
            let hud_ctx = HudCtx {
                toasts: &self.toasts,
                loading_message: self.loading_message.as_ref(),
                overdraw_active: sources.hud.overdraw_active,
            };
            let hud_result = draw_hud_overlays(ctx, &hud_ctx);
            if let Some(id) = hud_result.dismissed_toast_id {
                dismissed_toast_id = Some(id);
            }
            // Every inter-pane gap strip, painted in the theme's border
            // colour; without this the composite's black clear shows
            // through wherever a layout has no draggable divider.
            if !chrome.pane_gaps.is_empty() {
                let painter = ctx.layer_painter(egui::LayerId::background());
                for gap in chrome.pane_gaps {
                    painter.rect_filled(*gap, 0.0, self.theme.border);
                }
            }
            if let Some(div) = chrome.divider {
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
                    intents.raise(Intent::Layout(LayoutIntent::SetSplitRatio(raw_ratio)));
                }
                if resp.double_clicked() {
                    intents.raise(Intent::Layout(LayoutIntent::SetSplitRatio(
                        solarxy_core::view_config::DisplaySettings::DEFAULT_SPLIT_RATIO,
                    )));
                }
            }
            if sources.settings.active_pane().pane_mode == PaneMode::UvMap && !sources.hud.has_uvs {
                let screen_rect = ctx.input(egui::InputState::viewport_rect);
                let pane_center = chrome.active_pane_rect.unwrap_or(screen_rect).center();
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

        // Cleared as well as set, so a closed or undocked canvas stops
        // claiming the key it took while it was on screen.
        self.canvas_rect = canvas_rect_seen;
        if let Some(rect) = viewport_rect_logical {
            self.last_viewport_rect = Some(CachedViewportRect {
                rect,
                surface_size: (
                    frame.screen.size_in_pixels[0],
                    frame.screen.size_in_pixels[1],
                ),
            });
        }

        if let Some(id) = dismissed_toast_id {
            self.toasts.retain(|t| t.id != id);
        }

        self.winit_state
            .handle_platform_output(frame.window, full_output.platform_output);

        let tris = self
            .ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(frame.device, frame.queue, *id, image_delta);
        }
        self.renderer.update_buffers(
            frame.device,
            frame.queue,
            frame.encoder,
            &tris,
            &frame.screen,
        );

        let egui_view = frame
            .surface_texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.egui_format),
                ..Default::default()
            });

        let mut pass = frame
            .encoder
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
        self.renderer.render(&mut pass, &tris, &frame.screen);
        drop(pass);

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }
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

    /// Open the still-render modal for a fresh run.
    pub(crate) fn open_still_modal(&mut self, opening: super::modals::still::StillOpening) {
        self.still_modal.start(opening);
    }

    /// Update the still modal's tile and sample readout.
    pub fn set_still_progress(&mut self, tile: u32, tiles: u32, sample: u32, samples: u32) {
        self.still_modal.set_progress(tile, tiles, sample, samples);
    }

    /// The output format and space the dialog is set to, in the words the
    /// shared readback rule takes.
    pub fn still_output_choice(&self) -> (&'static str, &'static str) {
        self.still_modal.output_choice()
    }

    /// Whether the dialog is set to write a floating-point image, which is
    /// what decides the save filter and the extension.
    pub fn still_is_float(&self) -> bool {
        self.still_modal.is_float()
    }

    /// The dialog's Render button was pressed.
    pub fn take_still_render_request(&mut self) -> bool {
        self.still_modal.take_render_request()
    }

    /// The job has started; the dialog stops being idle. `transparent` comes
    /// from the job's own spec, so the preview's checker cannot drift from
    /// what the render actually carries.
    pub fn begin_still(
        &mut self,
        transparent: bool,
        requested: Vec<solarxy_host::passes::AovKind>,
        writes_aovs: bool,
    ) {
        self.still_modal.begin(transparent, requested, writes_aovs);
    }

    /// Hand the modal the render's elapsed and remaining, both computed by the
    /// shared job so this shell agrees with the other two.
    pub fn set_still_timing(&mut self, elapsed_ms: u64, remaining_ms: Option<u64>) {
        self.still_modal.set_timing(elapsed_ms, remaining_ms);
    }

    /// Hand the modal a fresh preview-sized frame of the assembling image.
    pub fn set_still_preview(&mut self, preview: image::RgbaImage) {
        self.still_modal.set_preview(preview);
    }

    /// Hand the modal the finished picture; `Save As…` becomes available.
    pub fn finish_still(&mut self, image: image::RgbaImage) {
        self.still_modal.finish(image);
    }

    /// Tell the modal the render failed and the picture is incomplete.
    pub fn fail_still(&mut self) {
        self.still_modal.fail();
    }

    /// Tell the modal the run was cancelled.
    pub fn mark_still_cancelled(&mut self) {
        self.still_modal.mark_cancelled();
    }

    /// Drain a pending cancel from the still modal (button or Escape).
    pub fn take_still_cancel(&mut self) -> bool {
        self.still_modal.take_cancel_request()
    }

    /// Drain a pending `Save As…` request from the still modal.
    pub fn take_still_save_request(&mut self) -> bool {
        self.still_modal.take_save_request()
    }

    /// Drain a pending `Save All…` request from the still modal.
    pub fn take_still_save_all_request(&mut self) -> bool {
        self.still_modal.take_save_all_request()
    }

    /// Drain a change of the still modal's Showing combo.
    pub fn take_still_pass_request(&mut self) -> Option<solarxy_host::passes::PassKind> {
        self.still_modal.take_pass_request()
    }

    /// The finished still, for a replay of the beauty into the preview.
    pub fn still_image(&self) -> Option<&image::RgbaImage> {
        self.still_modal.image()
    }

    /// The still modal's suggested file name.
    pub fn still_suggested_filename(&self) -> String {
        self.still_modal.suggested_filename().to_string()
    }

    /// Take the finished still out and close the modal.
    pub fn take_still_image(&mut self) -> Option<image::RgbaImage> {
        self.still_modal.take_image()
    }
}
