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
//! - `snapshot` — `GuiSnapshot` (crate-private; the sidebar ↔ state mirror)
//!   and [`SidebarChanges`]; see that module's docs for the "adding a
//!   sidebar control" recipe.
//! - `actions` — `MenuActions` (crate-private) event flags drained by
//!   `state/render.rs` after each frame.
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
mod actions;
mod console_view;
mod dock;
mod keyboard_shortcuts_modal;
mod material_inspector;
mod menu;
mod node_tree;
mod outliner;
mod overlays;
mod pane_toolbar;
mod preferences_modal;
mod properties;
mod renderer;
mod review_overlay;
mod review_panel;
mod review_popup;
mod review_visuals;
mod screenshot_modal;
mod sidebar;
mod snapshot;
mod status_bar;
mod theme;
mod update_modal;
mod viewport_context_menu;

#[cfg(target_os = "macos")]
const MOD: &str = "\u{2318}";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "Ctrl";

pub use overlays::ToastSeverity;
pub use renderer::EguiRenderer;
pub use snapshot::SidebarChanges;

pub(crate) use actions::{DividerInfo, MenuActions};
pub(crate) use node_tree::{NodeTreeAction, NodeTreeEvents, NodeTreeSource};
pub(crate) use outliner::{OutlinerAction, OutlinerEvents, OutlinerSource};
pub(crate) use pane_toolbar::{LookThroughChange, PaneToolbarData};
pub(crate) use properties::{PropertiesEvents, ValidationView};
pub(crate) use review_overlay::ReviewPaneOverlay;
pub(crate) use snapshot::{GuiSnapshot, HudInfo};
pub(crate) use viewport_context_menu::ViewportContextMenu;
