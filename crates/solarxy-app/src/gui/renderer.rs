use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::state::hdri_info::HdriInfo;
use solarxy_core::preferences::PaneMode;

use super::modals::about::draw_about_modal;
use super::dock::{Dock, SolarxyTab, SolarxyTabViewer, default_dock_state, tab_present, toggle_tab};
use super::modals::shortcuts::{KeyboardShortcutsModalState, draw_keyboard_shortcuts_modal};
use super::intent::{Intent, Intents, LayoutIntent, ReviewIntent, ToolIntent};
use super::panels::asset_preview::AssetPreviewState;
use super::panels::assets::AssetsState;
use super::panels::attributes::AttributesState;
use super::panels::text::TextState;
use super::panels::texture::TextureState;
use super::panels::tree::TreeState;
use super::chrome::menu::{MenuContext, draw_menu_bar};
use super::chrome::overlays::{HudCtx, Toast, ToastSeverity, draw_hud_overlays, overlay_frame};
use super::chrome::viewport_context_menu::{ViewportContextMenu, draw_viewport_context_menu};
use super::modals::preferences::{PreferencesModal, draw_preferences_modal};
use super::panels::review::panel::draw_delete_confirm_modal;
use super::panels::review::popup::draw_review_popup;
use super::modals::screenshot::{ScreenshotModal, draw_screenshot_modal};
use super::modals::still::{StillRenderModal, draw_still_modal};
use super::modals::turntable::{TurntableModal, TurntableRequest, draw_turntable_modal};
use super::theme::{Theme, apply_theme, configure_fonts, make_dock_style};
use super::modals::keymap_change::{KeymapNoticeState, draw_keymap_notice};
use super::modals::recovery::{RecoveryChoice, RecoveryModalState, draw_recovery_modal};
use super::modals::unsaved::{DiscardWhat, UnsavedChoice, UnsavedModalState, draw_unsaved_modal};
use super::modals::environment::draw_environment_modal;
use super::modals::arrangement_save::{ArrangementSaveModal, draw_arrangement_save_modal};
use egui_dock::DockArea;
use solarxy_core::preferences::{Preferences, ThemeChoice};

pub struct EguiRenderer {
    ctx: egui::Context,
    winit_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    egui_format: wgpu::TextureFormat,
    theme: Theme,
    about_open: bool,
    arrangement_save: ArrangementSaveModal,
    preferences_modal: PreferencesModal,
    shortcuts_modal: KeyboardShortcutsModalState,
    unsaved_modal: UnsavedModalState,
    recovery_modal: RecoveryModalState,
    keymap_notice: KeymapNoticeState,
    environment_open: bool,
    screenshot_modal: ScreenshotModal,
    still_modal: StillRenderModal,
    turntable_modal: TurntableModal,
    tree: TreeState,
    assets: AssetsState,
    /// The asset the preview tab shows, by hash and name.
    asset_preview: Option<(String, String)>,
    asset_preview_state: AssetPreviewState,
    texture: TextureState,
    attributes: AttributesState,
    text: TextState,
    /// The size the preview tab last drew at, read by the state layer.
    preview_size_seen: Option<(u32, u32)>,
    canvas: super::panels::nodes::CanvasState,
    params: super::panels::params::ParamPanelState,
    /// The floating host's state: a second pin and a second tab, which is
    /// what lets one node stay up beside the one the docked panel follows.
    params_floating: super::panels::params::ParamPanelState,
    floating_props: bool,
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
    hdri_info: Option<HdriInfo>,
    dock: Dock,
    /// The panel the pointer was over on the last pass, which is what the
    /// maximize key acts on.
    hovered_tab: Option<SolarxyTab>,
    /// The interactive furniture drawn over the viewport on the last pass,
    /// the tool column today, in logical pixels. The pointer routing keeps
    /// a click on any of it from also reaching the camera and the pick,
    /// the way the pane toolbar strip is kept out.
    viewport_chrome: Vec<egui::Rect>,
    pub last_viewport_rect: Option<CachedViewportRect>,
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
            about_open: false,
            arrangement_save: ArrangementSaveModal::default(),
            preferences_modal: PreferencesModal::default(),
            shortcuts_modal: KeyboardShortcutsModalState::default(),
            unsaved_modal: UnsavedModalState::default(),
            recovery_modal: RecoveryModalState::default(),
            keymap_notice: KeymapNoticeState::default(),
            environment_open: false,
            screenshot_modal: ScreenshotModal::default(),
            still_modal: StillRenderModal::default(),
            turntable_modal: TurntableModal::default(),
            tree: TreeState::default(),
            assets: AssetsState::default(),
            asset_preview: None,
            asset_preview_state: AssetPreviewState::default(),
            texture: TextureState::default(),
            attributes: AttributesState::default(),
            text: TextState::default(),
            preview_size_seen: None,
            canvas: super::panels::nodes::CanvasState::default(),
            params: super::panels::params::ParamPanelState::default(),
            params_floating: super::panels::params::ParamPanelState::default(),
            floating_props: false,
            canvas_rect: None,
            graph_ctx: solarxy_graph::document::GraphContext::Root,
            toasts: VecDeque::with_capacity(Self::TOAST_QUEUE_CAP),
            next_toast_id: 0,
            loading_message: None,
            hdri_info: None,
            dock: Dock::new(default_dock_state()),
            hovered_tab: None,
            viewport_chrome: Vec::new(),
            last_viewport_rect: None,
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

    /// The three colours the attribute labels wear, as the browser reads
    /// them off its theme: the text, the elevated background for the chip,
    /// and the accent for the dot. Unit-range sRGB, the renderer's input.
    pub fn label_colors(&self) -> [[f32; 3]; 3] {
        let unit = |c: egui::Color32| {
            [
                f32::from(c.r()) / 255.0,
                f32::from(c.g()) / 255.0,
                f32::from(c.b()) / 255.0,
            ]
        };
        [
            unit(self.theme.fg),
            unit(self.theme.bg_elevated),
            unit(self.theme.accent),
        ]
    }

    /// Cache the loaded HDRI's metadata for the Properties panel.
    pub(crate) fn update_hdri_info(&mut self, info: HdriInfo) {
        self.hdri_info = Some(info);
    }

    /// Preview one asset: remember which, and bring the preview tab up
    /// beside the Assets panel.
    pub(crate) fn open_asset_preview(&mut self, hash: String, name: String) {
        self.asset_preview = Some((hash, name));
        super::dock::show_tab_beside(
            self.dock.layout_mut(),
            SolarxyTab::AssetPreview,
            SolarxyTab::Assets,
        );
    }

    /// `true` iff the Assets tab is mounted, so the state layer can skip
    /// gathering what it would draw.
    #[must_use]
    pub fn assets_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::Assets)
    }

    /// `true` iff the preview tab is mounted.
    #[must_use]
    pub fn asset_preview_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::AssetPreview)
    }

    /// `true` iff the Texture tab is mounted, so the state layer can skip
    /// the image query.
    #[must_use]
    pub fn texture_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::Texture)
    }

    /// `true` iff the Attributes tab is mounted.
    #[must_use]
    pub fn attributes_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::Attributes)
    }

    /// `true` iff the Text tab is mounted.
    #[must_use]
    pub fn text_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::Text)
    }

    /// The node the canvas's info card is open on, so the state layer can
    /// gather its report.
    #[must_use]
    pub fn canvas_info(&self) -> Option<solarxy_graph::document::NodeId> {
        self.canvas.info_node()
    }

    /// The size the preview tab last drew its model at, in physical pixels.
    #[must_use]
    pub fn preview_size(&self) -> Option<(u32, u32)> {
        self.preview_size_seen
    }

    /// Hand egui a texture this shell rendered, and get the handle to draw
    /// it with.
    pub fn register_native_texture(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
    ) -> egui::TextureId {
        self.renderer
            .register_native_texture(device, view, wgpu::FilterMode::Linear)
    }

    /// Point an existing handle at a new texture, after a resize.
    pub fn update_native_texture(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        id: egui::TextureId,
    ) {
        self.renderer.update_egui_texture_from_wgpu_texture(
            device,
            view,
            wgpu::FilterMode::Linear,
            id,
        );
    }

    /// Release a handle and what it held.
    pub fn free_native_texture(&mut self, id: egui::TextureId) {
        self.renderer.free_texture(&id);
    }

    /// Show the Environment dialog.
    pub(crate) fn open_environment_modal(&mut self) {
        self.environment_open = true;
    }

    /// Drop the cached HDRI metadata when the HDRI is cleared.
    pub(crate) fn clear_hdri_info(&mut self) {
        self.hdri_info = None;
    }

    /// The loaded HDRI's metadata, when one is loaded.
    pub(crate) fn hdri_info(&self) -> Option<&HdriInfo> {
        self.hdri_info.as_ref()
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
    /// One method rather than one per panel: a menu's panel row, the panel
    /// shortcuts and a panel's own close button all mean the same thing, and
    /// three of these existed with one of them never called.
    pub(crate) fn toggle_tab(&mut self, tab: SolarxyTab) {
        toggle_tab(self.dock.layout_mut(), tab);
    }

    /// Maximize the panel's leaf, or restore when anything is maximized.
    pub(crate) fn toggle_maximize(&mut self, tab: SolarxyTab) {
        self.dock.toggle_maximize(tab);
    }

    /// Whether a panel is currently mounted anywhere in the dock.
    #[must_use]
    pub(crate) fn tab_present(&self, tab: SolarxyTab) -> bool {
        tab_present(self.dock.layout(), tab)
    }

    #[must_use]
    pub fn cursor_in_viewport(&self, cursor_logical: egui::Pos2) -> bool {
        self.last_viewport_rect
            .is_none_or(|c| c.rect.contains(cursor_logical))
    }

    /// Whether the cursor is over furniture drawn on the viewport that
    /// takes clicks of its own, so a press there is the widget's and never
    /// also the camera's or the pick's.
    pub fn cursor_over_viewport_chrome(&self, cursor_logical: egui::Pos2) -> bool {
        self.viewport_chrome
            .iter()
            .any(|rect| rect.contains(cursor_logical))
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
        // What is drawn rather than what is arranged: with another panel
        // maximized the viewport is mounted and not on screen.
        tab_present(self.dock.drawn(), SolarxyTab::Viewport)
    }

    /// `true` iff the Node Tree tab is currently mounted in the dock. The
    /// state layer gates the tree fold on this, so a closed panel costs
    /// nothing per frame. Read before the egui pass, so opening the tab
    /// from a menu populates it on the following frame — a
    /// latency no one can see.
    #[must_use]
    pub fn tree_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::Tree)
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
    pub fn properties_tab_present(&self) -> bool {
        self.tab_present(SolarxyTab::Properties)
    }

    /// The node the parameter panel is pinned to, if any.
    #[must_use]
    pub fn params_pin(&self) -> Option<solarxy_graph::document::NodeId> {
        self.params.pinned()
    }

    /// Whether the floating parameter panel is up, which is what the state
    /// layer gates its subject's assembly on.
    #[must_use]
    pub fn floating_props_open(&self) -> bool {
        self.floating_props
    }

    #[must_use]
    pub fn floating_params_pin(&self) -> Option<solarxy_graph::document::NodeId> {
        self.params_floating.pinned()
    }

    pub(crate) fn toggle_floating_props(&mut self) {
        self.floating_props = !self.floating_props;
    }

    /// Open the node info card on a node, from a surface other than the
    /// canvas the card is drawn over.
    pub(crate) fn open_node_info(&mut self, node: solarxy_graph::document::NodeId) {
        self.canvas.open_info(node);
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

    /// Whether the pointer is over the 3D viewport, and the viewport is
    /// actually mounted.
    ///
    /// Stricter than [`Self::cursor_in_viewport`], which answers `true` when
    /// no rect has been recorded so the camera keeps working on the first
    /// frame. A key scope must not guess: an unknown rect means the pointer
    /// is over nothing, so the press falls through to the canvas and then to
    /// the global scope.
    #[must_use]
    pub fn pointer_over_viewport(&self) -> bool {
        self.viewport_tab_present()
            && self
                .last_viewport_rect
                .zip(self.ctx.pointer_latest_pos())
                .is_some_and(|(cached, at)| cached.rect.contains(at))
    }

    /// Return every graph surface to the root context, unfolded and
    /// unseeded. Called whenever the open document is replaced: all of it
    /// addresses nodes the new document need not contain.
    pub fn reset_graph_surfaces(&mut self) {
        self.graph_ctx = solarxy_graph::document::GraphContext::Root;
        self.tree.reset();
        self.text.reset();
        self.asset_preview = None;
        self.canvas.reset();
        // The pin especially: node ids are minted per document, so one
        // carried across an open would point at whatever holds that id in
        // the incoming scene.
        self.params.reset();
        self.params_floating.reset();
    }

    /// The graph the user is looking at: where a selection made in either
    /// graph surface lives, and where a dropped model lands.
    pub fn graph_ctx(&self) -> solarxy_graph::document::GraphContext {
        self.graph_ctx
    }

    /// Look at another graph: where an undo puts the user back, so the
    /// change taken back is the one on screen. A context the document no
    /// longer has falls back to the root in the surfaces themselves.
    pub fn set_graph_ctx(&mut self, ctx: solarxy_graph::document::GraphContext) {
        self.graph_ctx = ctx;
    }

    /// Apply a JSON-serialized dock layout. Returns `true` if it could be
    /// restored; on failure, the existing layout is preserved and a debug
    /// line is logged. A panel the layout names that this build no longer
    /// has is dropped, and the rest kept.
    pub fn apply_layout_json(&mut self, json: &str) -> bool {
        match super::dock::restore(json) {
            Ok((state, dropped)) => {
                if dropped > 0 {
                    tracing::debug!("dock layout named {dropped} retired panel(s), dropped");
                }
                self.dock.replace(state);
                true
            }
            Err(err) => {
                tracing::debug!("dock layout rejected (keeping the current one): {err}");
                false
            }
        }
    }

    /// Serialize the current dock layout to a JSON string. Returns `None`
    /// if `serde_json` rejects the state (shouldn't happen for the upstream
    /// `DockState<SolarxyTab>` impl, but we treat it as best-effort).
    #[must_use]
    pub fn serialize_layout(&self) -> Option<String> {
        // The arrangement, never the leaf that may be covering it: a
        // maximized panel is not part of what gets saved.
        serde_json::to_string(self.dock.layout()).ok()
    }

    /// Replace the current dock layout with the factory default produced
    /// by the crate-private `default_dock_state` constructor.
    /// Ask for a name to save the current arrangement under.
    pub(crate) fn open_arrangement_save(&mut self) {
        self.arrangement_save.open();
    }

    /// The name the user committed, once.
    pub(crate) fn take_arrangement_name(&mut self) -> Option<String> {
        self.arrangement_save.take_committed()
    }

    /// Replace the panel layout with a named arrangement's. The layout is
    /// all this touches: the canvas preferences and the pane split that an
    /// arrangement also carries are the state layer's to write.
    pub(crate) fn apply_arrangement_layout(&mut self, arrangement: &super::Arrangement) {
        self.dock.replace(arrangement.recipe.build());
    }

    pub fn set_scene_open(&mut self, open: bool) {
        self.scene_open = open;
    }

    #[must_use]
    pub fn any_blocking_modal_open(&self, review: &crate::state::review::ReviewState) -> bool {
        self.about_open
            || self.arrangement_save.open
            || self.preferences_modal.open
            || self.shortcuts_modal.open
            || self.unsaved_modal.open
            || self.recovery_modal.open
            || self.keymap_notice.open
            || self.environment_open
            || review.delete_confirm.is_some()
            || review.editing.is_some()
    }

    /// Offer the autosave a launch found.
    /// Tell this installation, once, that the keyboard map changed.
    pub(crate) fn open_keymap_notice(&mut self) {
        self.keymap_notice.open();
    }

    /// Whether the reader asked for the full reference, and whether the
    /// notice was answered. Both are true once, on the frame it happened.
    pub(crate) fn take_keymap_notice_answer(&mut self) -> (bool, bool) {
        (
            self.keymap_notice.take_show_reference(),
            self.keymap_notice.take_dismissed(),
        )
    }

    pub(crate) fn open_recovery_prompt(&mut self, name: &str, when: &str) {
        self.recovery_modal.open(name, when);
    }

    /// The offer's answer, once.
    pub(crate) fn take_recovery_choice(&mut self) -> Option<RecoveryChoice> {
        self.recovery_modal.take_choice()
    }

    /// Ask whether to save before an action that discards the document.
    pub(crate) fn open_unsaved_prompt(&mut self, filename: &str, what: DiscardWhat) {
        self.unsaved_modal.open(filename, what);
    }

    /// The prompt's answer, once.
    pub(crate) fn take_unsaved_choice(&mut self) -> Option<UnsavedChoice> {
        self.unsaved_modal.take_choice()
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
        sources: &super::pass::PanelSources<'_>,
        review: &mut crate::state::review::ReviewState,
        // Everything the panels ask for this pass. Raised during it, applied
        // once it is over, and never cleared inside it: the pass can run
        // twice, and clearing per pass would drop what the first one raised.
        intents: &mut Intents,
        viewport_context_menu: &mut Option<ViewportContextMenu>,
        capture: super::pass::CaptureFrame,
    ) {
        let raw_input = self.winit_state.take_egui_input(frame.window);

        // The review panel's open flag is written by the state layer when
        // review mode starts, so it is reconciled into the dock before the
        // pass; the reverse direction is synced after the drain, where the
        // toggles it raises have already landed.
        if review.panel_open != tab_present(self.dock.layout(), SolarxyTab::ReviewPanel) {
            toggle_tab(self.dock.layout_mut(), SolarxyTab::ReviewPanel);
        }

        // Read from the dock rather than mirrored into a struct: the Window
        // menu's ticks are a question about the dock, and mirroring them was
        // what made a panel cost a field in four places.
        let present_at_start: std::collections::HashSet<SolarxyTab> = self
            .dock
            .layout()
            .iter_all_tabs()
            .map(|(_, t)| *t)
            .collect();
        let menu_cx = MenuContext {
            // The still renders either root: an open scene, or an open model
            // through the synthesized document.
            has_model: self.scene_open,
            recent_files: sources.recent_files,
            arrangements: sources.arrangements,
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
            theme: self.theme,
        };
        let mut viewport_rect_logical: Option<egui::Rect> = None;
        let mut canvas_rect_seen: Option<egui::Rect> = None;
        let mut preview_size_seen: Option<(u32, u32)> = None;
        let mut hovered_tab_seen: Option<SolarxyTab> = None;
        let mut chrome_rects_seen: Vec<egui::Rect> = Vec::new();
        let mut dismissed_toast_id: Option<u64> = None;

        // Cloned so the closure below can borrow the renderer mutably: the
        // context is a handle over a shared pointer, and `run` only needs a
        // shared borrow of it.
        let ctx_handle = self.ctx.clone();
        let full_output = ctx_handle.run(raw_input, |ctx| {

            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
                intents.raise(Intent::Edit(super::EditIntent::OpenPreferences));
            }
            // Always drawn. It could be hidden until 0.10.0, from a menu row
            // whose key had lost its binding, which left no way to bring it
            // back; the browser's bar cannot be hidden either.
            draw_menu_bar(
                ctx,
                sources.settings,
                intents,
                &|tab| present_at_start.contains(&tab),
                menu_cx,
            );

            let mut tab_viewer = SolarxyTabViewer {
                sources: *sources,
                panels: super::pass::PanelState {
                    tree: &mut self.tree,
                    assets: &mut self.assets,
                    asset_preview: self.asset_preview.as_ref(),
                    preview: &mut self.asset_preview_state,
                    texture: &mut self.texture,
                    attributes: &mut self.attributes,
                    text: &mut self.text,
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
                preview_size_out: &mut preview_size_seen,
                hovered_tab_out: &mut hovered_tab_seen,
                chrome_rects_out: &mut chrome_rects_seen,
                floating_props_open: self.floating_props,
                theme: self.theme,
            };
            DockArea::new(self.dock.drawn_mut())
                .style(make_dock_style(ctx, &self.theme))
                .show(ctx, &mut tab_viewer);

            // The screenshot modal counts as a blocking overlay only on
            // frames it is actually drawn — during a re-capture frame it
            // is suppressed so the markers it would occlude get captured.
            let screenshot_drawn = self.screenshot_modal.open && !capture.capturing;
            let suppress_overlay = self.about_open
                || self.arrangement_save.open
                || self.preferences_modal.open
                || self.shortcuts_modal.open
                || self.unsaved_modal.open
                || self.recovery_modal.open
            || self.keymap_notice.open
                || self.environment_open
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

            // The parameter panel's second host: modeless, so the canvas
            // underneath stays usable, which is what makes it useful for
            // holding one node up beside the one the docked panel follows.
            if self.floating_props {
                let mut open = true;
                egui::Window::new("Properties")
                    .id(egui::Id::new("solarxy_floating_properties"))
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(true)
                    .min_width(300.0)
                    .min_height(220.0)
                    .default_size([340.0, 420.0])
                    .show(ctx, |ui| {
                        super::panels::params::draw_params_content(
                            ui,
                            sources.params_floating,
                            &mut self.params_floating,
                            super::panels::params::Surface::Floating,
                            intents,
                            self.theme,
                        );
                    });
                if !open {
                    self.floating_props = false;
                }
            }

            draw_about_modal(ctx, &mut self.about_open);
            draw_arrangement_save_modal(ctx, &mut self.arrangement_save, sources.arrangements);
            draw_preferences_modal(ctx, &mut self.preferences_modal);
            draw_keyboard_shortcuts_modal(ctx, &mut self.shortcuts_modal);
            if !capture.capturing {
                draw_screenshot_modal(ctx, &mut self.screenshot_modal, &self.theme);
            }
            // Drawn ahead of the escape chain below: while a render runs,
            // Escape cancels it before it dismisses anything else.
            draw_still_modal(ctx, &mut self.still_modal, &self.theme);
            // Ahead of it for the same reason, and after the still so that a
            // still's Escape wins when both are somehow up.
            draw_turntable_modal(ctx, &mut self.turntable_modal, &self.theme);
            // Ahead of the escape chain for the same reason: while the
            // question is up, Escape answers it.
            draw_unsaved_modal(ctx, &mut self.unsaved_modal, &self.theme);
            draw_recovery_modal(ctx, &mut self.recovery_modal);
            draw_keymap_notice(ctx, &mut self.keymap_notice);
            draw_environment_modal(
                ctx,
                &mut self.environment_open,
                sources.settings,
                self.hdri_info.as_ref(),
                intents,
                &self.theme,
            );

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

            // The escape ladder, in the browser's order: a transform drag
            // in flight, then a pending re-anchor, then review mode, then a
            // maximized panel, which is last because it is the least in
            // flight of the four. The drag is the state's, so its rung
            // raises rather than cancels.
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                if sources.settings.tools.dragging {
                    intents.raise(Intent::Tool(ToolIntent::CancelDrag));
                } else if review.reanchor_target.is_some() {
                    review.cancel_reanchor();
                    intents.raise(Intent::Review(ReviewIntent::ReanchorCancelled));
                } else if review.active {
                    review.toggle_active();
                    intents.raise(Intent::Review(ReviewIntent::Exited));
                } else if !self.dock.restore() {
                    // Last of all, the floating parameter panel: the least
                    // in flight of anything Escape lets go of.
                    self.floating_props = false;
                }
            }
            // Maximize or restore the panel under the pointer. Consumed here
            // rather than dispatched, because only this pass knows which
            // panel that is; while maximized there is one panel on screen,
            // so the way back needs no target.
            if !suppress_overlay
                && !ctx.wants_keyboard_input()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backtick))
                && let Some(tab) = hovered_tab_seen
                    .or(self.hovered_tab)
                    .or(self.dock.is_maximized().then_some(SolarxyTab::Viewport))
            {
                self.dock.toggle_maximize(tab);
            }
            self.dock.settle();
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
        self.preview_size_seen = preview_size_seen;
        self.hovered_tab = hovered_tab_seen;
        self.viewport_chrome = chrome_rects_seen;
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

    /// Open the turntable export dialog.
    pub fn open_turntable_modal(&mut self) {
        self.turntable_modal.open_dialog();
    }

    /// Drain a request to start an export, with everything it was asked for.
    pub(crate) fn take_turntable_start(&mut self) -> Option<TurntableRequest> {
        self.turntable_modal.take_start_request()
    }

    pub fn take_turntable_cancel(&mut self) -> bool {
        self.turntable_modal.take_cancel_request()
    }

    /// Drain a request for the folder picker, which the state layer owns
    /// because the native dialog blocks and the interface pass must not.
    pub fn take_turntable_folder_request(&mut self) -> bool {
        self.turntable_modal.take_folder_request()
    }

    pub fn set_turntable_folder(&mut self, folder: std::path::PathBuf) {
        self.turntable_modal.set_folder(folder);
    }

    pub fn begin_turntable(&mut self, total: u32) {
        self.turntable_modal.begin(total);
    }

    pub fn set_turntable_progress(&mut self, written: u32, total: u32) {
        self.turntable_modal.set_progress(written, total);
    }

    pub fn finish_turntable(&mut self) {
        self.turntable_modal.finish();
    }

    pub fn mark_turntable_cancelled(&mut self, written: u32) {
        self.turntable_modal.mark_cancelled(written);
    }

    pub fn fail_turntable(&mut self, why: &str) {
        self.turntable_modal.fail(why);
    }
}
