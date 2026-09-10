//! egui integration — the only module in the workspace that depends on
//! egui + winit's pointer events together. Everything user-facing in the
//! GUI funnels through here.
//!
//! ## The layout
//!
//! Split along panel boundaries, so a reader looking for a panel finds a
//! module rather than a section of one.
//!
//! - `panels/` is one module per dock tab, with a folder where a panel is
//!   several files. A panel reads through `PanelSources` and writes nothing.
//! - `chrome/` is the shell's own furniture: the menu bar, the status bar, the
//!   per-pane toolbars, the floating overlays, the split divider and the
//!   viewport context menu. What separates it from `panels/` is that nothing
//!   here owns a dock tab.
//! - `modals/` is the dialogs. Each owns state on the renderer and is drained
//!   through a `take_*` accessor rather than the intent queue.
//! - `widgets` is the helpers more than one of the three uses.
//!
//! The rest is the core the three share. `renderer` is the per-frame
//! orchestrator and the only thing that runs an interface pass. `pass` is what
//! one pass is given, grouped by what the values are for. `dock` is the tab
//! set and the layout, and its variant names are persisted in user
//! preferences, so renaming one silently costs a reader their arrangement.
//! `intent` is the typed queue panels raise into, drained by
//! `state/intents.rs` once the pass is over; read its module docs before
//! raising from anywhere new, because a queue fed from state rather than from
//! an event double-raises on a twice-run frame. `settings` is the read-only
//! display state panels draw from. `theme` is the egui adapter over the shared
//! palette and authors no colours of its own.
//!
//! One rule cuts across all of it: `EguiRenderer::push_toast` emits its own
//! `tracing` event on `target: "solarxy::toast"`, so a caller must not also
//! log the same message.
//!
//! Cross-platform: `MOD` resolves to `⌘` on macOS and `Ctrl` elsewhere,
//! used in menu shortcut labels.

mod chrome;
mod dock;
mod intent;
mod modals;
mod panels;
mod pass;
mod renderer;
mod settings;
mod theme;
mod widgets;

#[cfg(target_os = "macos")]
const MOD: &str = "\u{2318}";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "Ctrl";

pub use chrome::overlays::ToastSeverity;
pub use renderer::EguiRenderer;

pub(crate) use chrome::divider::DividerInfo;
pub(crate) use dock::SolarxyTab;
pub(crate) use pass::{CaptureFrame, FramePaint, PanelSources, ViewportChrome};
pub(crate) use settings::{CookReadout, PanelSettings};
pub(crate) use modals::still::StillOpening;
pub(crate) use intent::{
    CaptureIntent, CookIntent, DisplayChange, EditIntent, FileIntent, HelpIntent, Intent, Intents,
    LayoutIntent, PaneChange, PanelIntent, PostChange, ReviewIntent,
};
pub(crate) use panels::node_tree::{NodeTreeAction, NodeTreeSource};
pub(crate) use panels::nodes::{CanvasAction, CanvasScene, CanvasSource, CanvasToggle, NodeCook};
pub(crate) use panels::params::{ParamPanelSource, ParamScene};
pub(crate) use panels::outliner::{OutlinerAction, OutlinerSource};
pub(crate) use chrome::pane_toolbar::{LookThroughChange, PaneToolbarData, PaneView};
pub(crate) use panels::properties::{NodeActionsView, ValidationView};
pub(crate) use panels::review::overlay::ReviewPaneOverlay;
pub(crate) use chrome::overlays::HudInfo;
pub(crate) use chrome::viewport_context_menu::{ContextTarget, ViewportAction, ViewportContextMenu};
