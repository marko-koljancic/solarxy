//! In-scene review: the docked panel, the viewport marker overlay, the
//! annotation popup, and the category glyphs and colours the first two share.
//!
//! Four files rather than one because they draw in three different places and
//! agree on a fifth thing. `visuals` is that agreement: marker colour and panel
//! chip colour are the reader's first correlation cue, so they come from one
//! table.

pub(in crate::gui) mod overlay;
pub(in crate::gui) mod panel;
pub(in crate::gui) mod popup;
pub(in crate::gui) mod visuals;
