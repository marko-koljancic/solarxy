//! The guided tour: three tours the Help menu replays, and the first-run
//! introduction.
//!
//! Ported from the browser rather than invented, because a user who meets
//! the node canvas for the first time needs the same introduction whichever
//! shell they opened, and because the two halves worth getting right are
//! already data and a pure function there: the catalogue in [`steps`] and
//! the placement rule in [`placement`]. Both are held against the browser's
//! own source by tests, so the tours cannot come to say different things.
//!
//! **The one thing that does not port is how a step finds its subject.**
//! The browser writes a CSS selector and asks the document. There is no
//! document here, so a step names one of eight surfaces and the shell
//! supplies a rectangle for each from the previous frame, the way the
//! maximize key already learns which panel the pointer is over.

pub(in crate::gui) mod overlay;
pub(in crate::gui) mod placement;
pub(crate) mod steps;
