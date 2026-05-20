//! Shared category visuals for the Review System.
//!
//! Single source of truth for the four category colors + their letter
//! glyphs. Consumed by `review_panel` (chips) and `review_overlay` (pins,
//! cards). The colors are theme-scoped (see [`super::theme::ReviewColors`])
//! so they re-contrast on the light theme — keep marker color and panel
//! chip color in sync (the user's #1 visual-correlation cue).

use solarxy_core::review::AnnotationCategory;

use super::theme::Theme;

pub(super) fn category_color(theme: Theme, c: AnnotationCategory) -> egui::Color32 {
    match c {
        AnnotationCategory::Info => theme.review.info,
        AnnotationCategory::Warning => theme.review.warning,
        AnnotationCategory::Question => theme.review.question,
        AnnotationCategory::Change => theme.review.change,
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
