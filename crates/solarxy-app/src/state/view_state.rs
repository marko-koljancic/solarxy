//! Re-exports for the view-related types [`crate::state::State`] holds.
//!
//! The state itself is [`solarxy_host::HostViewState`], shared with the web
//! shell. This module carries the [`solarxy_core::view_config`] re-exports the
//! rest of the crate reaches through, so the many `use super::{...}` sites did
//! not all have to change when the struct moved.

pub(crate) use solarxy_core::view_config::{
    BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout,
};
pub(crate) use solarxy_host::HostViewState as ViewState;
