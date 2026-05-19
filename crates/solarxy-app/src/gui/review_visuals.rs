//! Shared category visuals for the Review System.
//!
//! Single source of truth for the four category colors + their letter
//! glyphs. Consumed by `review_panel` (chips) and `review_overlay` (pins,
//! cards). Keep these in sync — drift between marker color and panel chip
//! color is the user's #1 visual-correlation cue.

use solarxy_core::review::AnnotationCategory;

pub(super) const COLOR_INFO: egui::Color32 = egui::Color32::from_rgb(0x5C, 0x9E, 0xFF);
pub(super) const COLOR_WARNING: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xB2, 0x3D);
pub(super) const COLOR_QUESTION: egui::Color32 = egui::Color32::from_rgb(0xA0, 0x6D, 0xFF);
pub(super) const COLOR_CHANGE: egui::Color32 = egui::Color32::from_rgb(0x3D, 0xC9, 0x7A);
pub(super) const SELECTION_ACCENT: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xC4, 0x4C);

pub(super) fn category_color(c: AnnotationCategory) -> egui::Color32 {
    match c {
        AnnotationCategory::Info => COLOR_INFO,
        AnnotationCategory::Warning => COLOR_WARNING,
        AnnotationCategory::Question => COLOR_QUESTION,
        AnnotationCategory::Change => COLOR_CHANGE,
    }
}

pub(super) fn category_letter(c: AnnotationCategory) -> &'static str {
    match c {
        AnnotationCategory::Info => "i",
        AnnotationCategory::Warning => "!",
        AnnotationCategory::Question => "?",
        AnnotationCategory::Change => "\u{270e}",
    }
}

pub(super) fn category_label(c: AnnotationCategory) -> &'static str {
    match c {
        AnnotationCategory::Info => "Info",
        AnnotationCategory::Warning => "Warning",
        AnnotationCategory::Question => "Question",
        AnnotationCategory::Change => "Change",
    }
}
