//! Renderer-owned input vocabulary for the camera controller.
//!
//! The renderer is windowing-agnostic: shells (winit on desktop, the browser
//! on web) map their native event types onto these enums at the boundary.
//! Only the inputs the camera actually consumes are represented.

/// Pointer buttons the orbit camera reacts to. Anything else maps to
/// [`PointerButton::Other`], which every handler ignores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    /// Orbit drag.
    Left,
    /// Pan drag.
    Middle,
    /// Currently unused by the camera; reserved for shells to pass through.
    Right,
    /// Any other button; ignored.
    Other,
}

/// Keys the camera controller consumes (arrow-key nudges).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}
