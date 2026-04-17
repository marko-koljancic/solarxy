mod about;
mod actions;
mod console_view;
mod menu;
mod overlays;
mod renderer;
mod sidebar;
mod snapshot;
mod stats;
mod theme;
mod update_modal;

#[cfg(target_os = "macos")]
const MOD: &str = "\u{2318}";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "Ctrl";

pub use overlays::ToastSeverity;
pub use renderer::EguiRenderer;
pub use snapshot::SidebarChanges;

pub(crate) use actions::MenuActions;
pub(crate) use snapshot::{GuiSnapshot, HudInfo};
