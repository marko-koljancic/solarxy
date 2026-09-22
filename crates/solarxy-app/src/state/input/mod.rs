//! Keyboard and pointer input for the `State`-rooted application.
//!
//! Four files, because this is four things. `keyboard` is the key map and
//! the toggles it drives, `pointer` is what a click and a drag mean, `click`
//! is how the shell tells the two apart, and `dialogs` is the native file
//! pickers.
//!
//! A dozen `impl State` helpers used to live here too, for no better reason
//! than that they were written here. They have gone to where their callers
//! are: the drain arms and the recompute helpers beside the drain in
//! `state/intents.rs`, the framing to `state/camera.rs`, the show-and-hide to
//! `state/visibility.rs`, and the preference writers to
//! `state/persist.rs`.

pub(crate) mod click;
mod dialogs;
mod keyboard;
pub(crate) mod keymap;
mod pointer;

pub(crate) use keyboard::{key_scope, window_claims};
