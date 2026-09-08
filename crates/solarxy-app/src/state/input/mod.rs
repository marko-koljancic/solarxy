//! Keyboard and pointer input for the `State`-rooted application.
//!
//! Three files, because this was three things. `keyboard` is the key map and
//! the toggles it drives, `pointer` is what a click and a drag mean, and
//! `dialogs` is the native file pickers.
//!
//! A dozen `impl State` helpers used to live here too, for no better reason
//! than that they were written here. They have gone to where their callers
//! are: the drain arms and the recompute helpers beside the drain in
//! `state/intents.rs`, the framing to `state/camera.rs`, the show-and-hide to
//! `state/visibility.rs`, and the preference writers to
//! `state/preferences.rs`.

mod dialogs;
mod keyboard;
mod pointer;
