//! What one interface pass is given, grouped by what the values are for.
//!
//! `render_ui` took twenty-four parameters before these existed, and every
//! panel the shell gained cost it another one, plus a field on the dock's tab
//! viewer, plus a line at the construction site. The groups below are the
//! answer to that: a panel that needs a new source adds a field to the group
//! that is already about sources, and the entry point and the tab viewer both
//! see it without either signature moving.
//!
//! They are groups rather than one bundle because the six have genuinely
//! different lifetimes and mutability, and a single struct would have to be
//! the loosest of them.

use egui_wgpu::ScreenDescriptor;

use crate::console::ConsoleState;

use super::chrome::divider::DividerInfo;
use super::chrome::overlays::HudInfo;
use super::panels::asset_preview::{AssetPreviewState, PreviewView};
use super::panels::assets::{AssetsSource, AssetsState};
use super::panels::attributes::{AttributesSource, AttributesState};
use super::panels::nodes::{CanvasSource, CanvasState};
use super::panels::params::{ParamPanelSource, ParamPanelState};
use super::panels::text::{TextSource, TextState};
use super::panels::texture::{TextureSource, TextureState};
use super::panels::tree::{TreeSource, TreeState};
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
    /// The open document's file name and format, for the status bar.
    pub document: Option<(&'a str, &'a str)>,
    /// Errors and warnings across every object, for the status bar.
    pub validation_counts: (usize, usize),
    /// The open document, when the Tree tab is mounted. The state layer
    /// passes `Empty` for a closed tab so the fold is skipped.
    pub tree: TreeSource<'a>,
    /// The open document and the revision that produced it, when the
    /// canvas tab is mounted. `Empty` for a closed tab, so a canvas nobody
    /// is looking at costs nothing.
    pub canvas: CanvasSource<'a>,
    /// The node the parameter panel edits, and what its last cook said,
    /// when the Properties tab that hosts it is mounted.
    pub params: ParamPanelSource<'a>,
    /// The staged assets, when the Assets tab or the preview is mounted.
    pub assets: AssetsSource<'a>,
    /// The model preview's texture and status, for the preview tab.
    pub preview: PreviewView<'a>,
    /// The image network's published output, when the Texture tab is
    /// mounted.
    pub texture: TextureSource<'a>,
    /// The engine and the graph the user is in, for the paged table, when
    /// the Attributes tab is mounted.
    pub attributes: AttributesSource<'a>,
    /// The document and the graph the user is in, for the snippets, when
    /// the Text tab is mounted.
    pub text: TextSource<'a>,
    pub recent_files: &'a [String],
}

/// The per-panel interface state the dock's tabs write directly: folds,
/// selections, filters. Never document state, which the engine owns.
pub(crate) struct PanelState<'a> {
    pub console: &'a mut ConsoleState,
    pub tree: &'a mut TreeState,
    pub assets: &'a mut AssetsState,
    /// The asset the preview tab shows, as its hash and name.
    pub asset_preview: Option<&'a (String, String)>,
    /// The preview tab's own image view.
    pub preview: &'a mut AssetPreviewState,
    pub texture: &'a mut TextureState,
    pub attributes: &'a mut AttributesState,
    pub text: &'a mut TextState,
    pub canvas: &'a mut CanvasState,
    pub params: &'a mut ParamPanelState,
    /// **Shared, and deliberately so.** Which graph the user is looking
    /// at is one fact, not one per panel: a dive made on the canvas has to
    /// be where the tree is too, and a dropped model has to land where the
    /// user believes they are. It sits here rather than on either panel
    /// because both write it and neither owns it.
    pub graph_ctx: &'a mut solarxy_graph::document::GraphContext,
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
