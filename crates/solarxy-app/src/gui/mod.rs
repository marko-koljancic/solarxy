//! egui integration — the only module in the workspace that depends on
//! egui + winit's pointer events together. Everything user-facing in the
//! GUI funnels through here.
//!
//! Submodules (one responsibility per file):
//! - `renderer` — [`EguiRenderer`], the per-frame orchestrator; owns the
//!   toast queue, preferences modal state, update modal state, console
//!   state.
//! - `sidebar` — collapsible View / Inspect / Material / Debug / Rendering /
//!   Advanced panels. Canonical surface for live runtime settings.
//! - `menu` — native-style menu bar (File / Edit / View / Window / Help).
//!   The Window menu is the single source of truth for togglable panel
//!   visibility.
//! - `settings`: the read-only display state panels draw from. Changing one
//!   is an intent, not a write through a borrow.
//! - `intent`: the typed intent queue panels raise into, drained by
//!   `state/intents.rs` once the pass is over. Read its module docs before
//!   raising from anywhere new: a queue fed from state rather than from an
//!   event double-raises on a twice-run frame.
//! - `overlays` — toast queue + FPS HUD + loading indicator + severities.
//!   Every `push_toast` emits a matching `tracing` event on
//!   `target: "solarxy::toast"` — callers must NOT also emit their own
//!   log for the same message.
//! - `preferences_modal`, `keyboard_shortcuts_modal`, `update_modal`,
//!   `about`, `console_view`, `properties`, `theme` — supporting
//!   modal/panel surfaces.
//!
//! Cross-platform: `MOD` resolves to `⌘` on macOS and `Ctrl` elsewhere,
//! used in menu shortcut labels.

mod about;
mod console_view;
mod divider;
mod dock;
mod intent;
mod keyboard_shortcuts_modal;
mod material_inspector;
mod menu;
mod node_tree;
mod outliner;
mod overlays;
mod pane_toolbar;
mod pass;
mod preferences_modal;
mod properties;
mod renderer;
mod review_overlay;
mod review_panel;
mod review_popup;
mod review_visuals;
mod screenshot_modal;
mod settings;
mod sidebar;
mod status_bar;
mod still_modal;
mod theme;
mod update_modal;
mod viewport_context_menu;

#[cfg(target_os = "macos")]
const MOD: &str = "\u{2318}";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "Ctrl";

pub use overlays::ToastSeverity;
pub use renderer::EguiRenderer;

pub(crate) use divider::DividerInfo;
pub(crate) use dock::SolarxyTab;
pub(crate) use pass::{CaptureFrame, FramePaint, PanelSources, ViewportChrome};
pub(crate) use settings::PanelSettings;
pub(crate) use intent::{
    CaptureIntent, DisplayChange, EditIntent, FileIntent, HelpIntent, Intent, Intents,
    LayoutIntent, PaneChange, PanelIntent, PostChange, ReviewIntent,
};
pub(crate) use node_tree::{NodeTreeAction, NodeTreeSource};
pub(crate) use outliner::{OutlinerAction, OutlinerSource};
pub(crate) use pane_toolbar::{LookThroughChange, PaneToolbarData};
pub(crate) use properties::ValidationView;
pub(crate) use review_overlay::ReviewPaneOverlay;
pub(crate) use overlays::HudInfo;
pub(crate) use viewport_context_menu::ViewportContextMenu;
