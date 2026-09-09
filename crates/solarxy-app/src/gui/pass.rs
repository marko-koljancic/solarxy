//! What one interface pass is given, grouped by what the values are for.
//!
//! `render_ui` took twenty-four parameters before these existed, and every
//! panel the shell gained cost it another one, plus a field on the dock's tab
//! viewer, plus a line at the construction site. The groups below are the
//! answer to that: a panel that needs a new source adds a field to the group
//! that is already about sources, and the entry point and the tab viewer both
//! see it without either signature moving.
//!
//! They are groups rather than one bundle because the four have genuinely
//! different lifetimes and mutability, and a single struct would have to be
//! the loosest of them.

use egui_wgpu::ScreenDescriptor;

use crate::console::ConsoleState;
use crate::state::hdri_info::HdriInfo;

use super::chrome::divider::DividerInfo;
use super::panels::node_tree::{NodeTreeSource, NodeTreeState};
use super::panels::outliner::OutlinerSource;
use super::chrome::overlays::HudInfo;
use super::panels::properties::{ModelInfo, ValidationView};
use super::panels::review::overlay::ReviewPaneOverlay;
use super::settings::PanelSettings;

/// The graphics handles and the surface one pass paints into.
pub(crate) struct FramePaint<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub window: &'a winit::window::Window,
    pub surface_texture: &'a wgpu::Texture,
    pub screen: ScreenDescriptor,
    /// This frame's duration, which feeds the status bar's rolling average.
    pub frame_ms: f32,
}

/// The viewport's own geometry: where the panes are, what sits between them,
/// and what floats on top of them.
pub(crate) struct ViewportChrome<'a> {
    pub divider: Option<DividerInfo>,
    pub pane_gaps: &'a [egui::Rect],
    pub active_pane_rect: Option<egui::Rect>,
    pub review_panes: &'a [ReviewPaneOverlay],
    pub toolbars: super::chrome::pane_toolbar::PaneToolbarData<'a>,
}

/// What the panels read about the open document and the session.
///
/// **A panel that needs something new adds a field here**, rather than a
/// parameter to the entry point and a field on the tab viewer and a line at
/// the construction site.
#[derive(Clone, Copy)]
pub(crate) struct PanelSources<'a> {
    /// The display state the panels draw their current values from. Changing
    /// one is an intent, so this is read-only like everything else here.
    pub settings: PanelSettings<'a>,
    pub hud: &'a HudInfo,
    pub validation: ValidationView<'a>,
    pub outliner: OutlinerSource<'a>,
    /// The open document, when the Node Tree tab is mounted. The state layer
    /// passes `Empty` for a closed tab so the fold is skipped.
    pub node_tree: NodeTreeSource<'a>,
    /// The node selected in the Node Tree and the actions it declares, for
    /// the Properties panel's Actions section.
    pub actions: super::panels::properties::NodeActionsView<'a>,
    pub recent_files: &'a [String],
}

/// What the renderer caches about the open file.
///
/// Its own group rather than part of [`PanelSources`], because the state layer
/// hands these over once at load rather than every frame, and the renderer is
/// what holds them in between.
#[derive(Clone, Copy)]
pub(super) struct OpenFile<'a> {
    pub model_info: Option<&'a ModelInfo>,
    pub hdri_info: Option<&'a HdriInfo>,
}

/// The per-panel interface state the dock's tabs write directly: folds,
/// selections, filters. Never document state, which the engine owns.
pub(crate) struct PanelState<'a> {
    pub console: &'a mut ConsoleState,
    pub node_tree: &'a mut NodeTreeState,
}

/// What a capture frame asks the interface to do differently.
///
/// The screenshot modal is suppressed on any capture frame so it cannot land
/// in the shot; a re-capture additionally forces every review card open for
/// that one frame.
#[derive(Clone, Copy, Default)]
pub(crate) struct CaptureFrame {
    pub capturing: bool,
    pub expand_review: bool,
}
