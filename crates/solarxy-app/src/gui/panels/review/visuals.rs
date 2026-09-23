//! Shared category visuals for the Review System.
//!
//! Single source of truth for the four category colors, their letter
//! glyphs and their labels, over the engine's own category vocabulary.
//! Consumed by `panel` (chips), `popup` (the category picker) and
//! `overlay` (pins, cards), the siblings beside this file. The colors are
//! theme-scoped (see [`crate::gui::theme::ReviewColors`]) so they
//! re-contrast on the light theme; marker color and panel chip color stay in
//! step, since that is the reader's first correlation cue.

use solarxy_graph::review::ReviewCategory;

use crate::gui::theme::Theme;

/// The four categories in the order the chips and the picker list them,
/// which is the order the filter array is indexed in.
pub(in crate::gui) const CATEGORIES: [ReviewCategory; 4] = [
    ReviewCategory::Info,
    ReviewCategory::Warning,
    ReviewCategory::Question,
    ReviewCategory::Change,
];

/// A category's index into the panel's filter array.
pub(in crate::gui) fn category_index(c: ReviewCategory) -> usize {
    match c {
        ReviewCategory::Info => 0,
        ReviewCategory::Warning => 1,
        ReviewCategory::Question => 2,
        ReviewCategory::Change => 3,
    }
}

pub(in crate::gui) fn category_color(theme: Theme, c: ReviewCategory) -> egui::Color32 {
    match c {
        ReviewCategory::Info => theme.review.info,
        ReviewCategory::Warning => theme.review.warning,
        ReviewCategory::Question => theme.review.question,
        ReviewCategory::Change => theme.review.change,
    }
}

pub(in crate::gui) fn category_letter(c: ReviewCategory) -> &'static str {
    match c {
        ReviewCategory::Info => "i",
        ReviewCategory::Warning => "!",
        ReviewCategory::Question => "?",
        ReviewCategory::Change => "\u{270e}",
    }
}

pub(in crate::gui) fn category_label(c: ReviewCategory) -> &'static str {
    match c {
        ReviewCategory::Info => "Info",
        ReviewCategory::Warning => "Warning",
        ReviewCategory::Question => "Question",
        ReviewCategory::Change => "Change",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_category_order_indexes_the_filter_array() {
        for (i, c) in CATEGORIES.iter().enumerate() {
            assert_eq!(category_index(*c), i);
        }
    }
}
