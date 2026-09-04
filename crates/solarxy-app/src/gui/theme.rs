//! The egui adapter over the shared interface palette, plus the matching
//! `egui_dock` style.
//!
//! [`Theme`] is a flat bundle of `Color32` tokens. Because every field is
//! `Copy`, `Theme` itself is `Copy` and threads through draw code without
//! lifetime noise. `EguiRenderer` owns the active `Theme`; [`apply_theme`]
//! pushes it into the egui `Context` and is re-run on a live theme swap.
//!
//! **This file authors no colors.** Every value comes from
//! [`solarxy_core::theme::Palette`], which the web frontend (through
//! generated CSS) and the analyze TUI read too, so one edit reaches all
//! three shells. Before 0.7.1 each surface hand-authored its own values and
//! they drifted: the review "change" category was green here and error-red
//! on web. What lives here is only the *mapping* from semantic role to
//! egui's widget vocabulary.
//!
//! Two presets ship, selected via
//! [`solarxy_core::preferences::ThemeChoice`]: neutral grey with an amber
//! accent, and warm cream paper with a terracotta accent.
//!
//! All corner radii are zero — flat, professional, 3D-DCC-app feel.

use solarxy_core::preferences::ThemeChoice;
use solarxy_core::theme::{Palette, Rgb};

const fn rgb(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c.r, c.g, c.b)
}

/// The four Review-System category colors plus the shared selection
/// accent. Theme-scoped because the light palette re-contrasts the
/// category hues against its cream ground.
///
/// These four are the strongest visual-correlation cue in the product: the
/// same hue must color a viewport pin and its panel chip. `review_panel`
/// and `review_overlay` both read them from here.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReviewColors {
    pub info: egui::Color32,
    pub warning: egui::Color32,
    pub question: egui::Color32,
    pub change: egui::Color32,
    pub selection_accent: egui::Color32,
}

/// A complete interface palette. `Copy` — pass it by value freely.
#[derive(Debug, Clone, Copy)]
pub(super) struct Theme {
    pub dark: bool,
    pub bg: egui::Color32,
    pub bg_elevated: egui::Color32,
    pub fg: egui::Color32,
    pub muted: egui::Color32,
    pub accent: egui::Color32,
    pub selection: egui::Color32,
    pub widget_bg: egui::Color32,
    pub widget_hover: egui::Color32,
    pub border: egui::Color32,
    pub severity_error: egui::Color32,
    pub severity_warn: egui::Color32,
    /// Mapped for completeness; not yet read by any widget.
    #[allow(dead_code)]
    pub severity_info: egui::Color32,
    pub severity_success: egui::Color32,
    pub review: ReviewColors,
}

impl Theme {
    /// Resolve a persisted [`ThemeChoice`] into a concrete palette.
    pub(super) fn from_choice(choice: ThemeChoice) -> Self {
        Self::from_palette(&choice.palette())
    }

    /// Map the shared semantic roles onto egui's widget vocabulary.
    ///
    /// This mapping is the only thing this file decides; the colors
    /// themselves belong to `solarxy_core::theme`.
    pub(super) fn from_palette(palette: &Palette) -> Self {
        let r = &palette.roles;
        let accent = rgb(r.accent.rgb);
        Self {
            dark: palette.dark,
            bg: rgb(r.surface_app.rgb),
            bg_elevated: rgb(r.surface_raised.rgb),
            fg: rgb(r.ink_primary.rgb),
            muted: rgb(r.ink_muted.rgb),
            accent,
            selection: rgb(r.selection.rgb),
            // egui's "widget" is a raised interactive surface: the RAISED
            // role, not the overlay one. Mapping it to overlay put it on the
            // same value as `hover_bg` on the dark palette (both n-700), so
            // hovering any widget changed nothing at all. The two must stay
            // distinct — hover brightens on dark, darkens on cream, and both
            // read against the panel behind them.
            widget_bg: rgb(r.surface_raised.rgb),
            widget_hover: rgb(r.hover_bg.rgb),
            border: rgb(r.border_subtle.rgb),
            severity_error: rgb(r.status_error.rgb),
            // Warn rides the attention hue, always paired with a shape.
            severity_warn: rgb(r.state_attention.rgb),
            severity_info: rgb(r.display.rgb),
            severity_success: rgb(r.status_success.rgb),
            review: ReviewColors {
                info: rgb(palette.review.info),
                warning: rgb(palette.review.warning),
                question: rgb(palette.review.question),
                change: rgb(palette.review.change),
                selection_accent: accent,
            },
        }
    }
}

/// Push `theme` into the egui `Context`. Idempotent — safe to re-run on a
/// live theme swap. The egui `Visuals` base is picked from `theme.dark`
/// so the handful of fields this function does not override still land
/// on theme-appropriate values.
pub(super) fn apply_theme(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = if theme.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.dark_mode = theme.dark;

    visuals.panel_fill = theme.bg;
    visuals.window_fill = theme.bg;
    visuals.extreme_bg_color = theme.bg;
    visuals.faint_bg_color = theme.widget_hover;
    visuals.code_bg_color = theme.widget_hover;
    visuals.override_text_color = Some(theme.fg);
    visuals.hyperlink_color = theme.accent;
    visuals.selection.bg_fill = theme.selection;
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, theme.accent);

    let zero = egui::CornerRadius::ZERO;
    visuals.window_corner_radius = zero;
    visuals.menu_corner_radius = zero;

    visuals.widgets.noninteractive.bg_fill = theme.bg;
    visuals.widgets.noninteractive.weak_bg_fill = theme.bg;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, theme.widget_bg);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, theme.fg);
    visuals.widgets.noninteractive.corner_radius = zero;

    visuals.widgets.inactive.bg_fill = theme.widget_bg;
    visuals.widgets.inactive.weak_bg_fill = theme.widget_bg;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, theme.fg);
    visuals.widgets.inactive.corner_radius = zero;

    visuals.widgets.hovered.bg_fill = theme.widget_hover;
    visuals.widgets.hovered.weak_bg_fill = theme.widget_hover;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, theme.accent);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, theme.fg);
    visuals.widgets.hovered.corner_radius = zero;

    visuals.widgets.active.bg_fill = theme.selection;
    visuals.widgets.active.weak_bg_fill = theme.selection;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, theme.accent);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, theme.fg);
    visuals.widgets.active.corner_radius = zero;

    visuals.widgets.open.bg_fill = theme.widget_hover;
    visuals.widgets.open.weak_bg_fill = theme.widget_hover;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, theme.widget_bg);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, theme.fg);
    visuals.widgets.open.corner_radius = zero;

    let mut style = egui::Style {
        visuals,
        ..Default::default()
    };

    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(10.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(12.0, egui::FontFamily::Monospace),
    );

    style.spacing.item_spacing = egui::vec2(6.0, 2.0);
    style.spacing.button_padding = egui::vec2(4.0, 1.0);
    style.spacing.indent = 16.0;
    style.spacing.window_margin = egui::Margin::same(4);

    ctx.set_style(style);
}

/// Build the `egui_dock` style that pairs with [`apply_theme`]. Overrides
/// the defaults that produce the dark tab-bar strip + rounded leaf
/// corners: tab bar fill matches the panel (no contrasting strip),
/// every corner radius is zero, active tab uses the selection chip
/// fill, inactive tabs use the muted text color on the panel fill.
pub(super) fn make_dock_style(ctx: &egui::Context, theme: &Theme) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(ctx.style().as_ref());
    let zero = egui::CornerRadius::ZERO;

    style.main_surface_border_rounding = zero;
    style.main_surface_border_stroke = egui::Stroke::NONE;

    style.tab_bar.bg_fill = theme.bg;
    style.tab_bar.corner_radius = zero;
    style.tab_bar.hline_color = theme.widget_bg;

    style.separator.color_idle = theme.widget_bg;
    style.separator.color_hovered = theme.accent;
    style.separator.color_dragged = theme.accent;

    let make_tab = |bg: egui::Color32, text: egui::Color32| egui_dock::TabInteractionStyle {
        outline_color: theme.widget_bg,
        corner_radius: zero,
        bg_fill: bg,
        text_color: text,
    };
    style.tab.active = make_tab(theme.selection, theme.fg);
    style.tab.active_with_kb_focus = make_tab(theme.selection, theme.fg);
    style.tab.focused = make_tab(theme.selection, theme.fg);
    style.tab.focused_with_kb_focus = make_tab(theme.selection, theme.fg);
    style.tab.inactive = make_tab(theme.bg, theme.muted);
    style.tab.inactive_with_kb_focus = make_tab(theme.bg, theme.muted);
    style.tab.hovered = make_tab(theme.widget_hover, theme.fg);

    style.tab.tab_body.bg_fill = theme.bg;
    style.tab.tab_body.corner_radius = zero;
    style.tab.tab_body.stroke = egui::Stroke::NONE;
    style.tab.hline_below_active_tab_name = false;

    style.buttons.close_tab_color = theme.muted;
    style.buttons.close_tab_active_color = theme.fg;
    style.buttons.close_tab_bg_fill = theme.widget_hover;
    style.buttons.add_tab_color = theme.muted;
    style.buttons.add_tab_active_color = theme.fg;
    style.buttons.add_tab_bg_fill = theme.widget_hover;
    style.buttons.add_tab_border_color = theme.widget_bg;

    style
}

pub(super) fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Two bundled faces, one per family. Inter fronts the proportional
    // family: it is the same interface face the web application uses, so
    // the two shells read as one product, and it is already recorded in
    // THIRD-PARTY-NOTICES.md under the same license as the face below.
    // Lilex fronts the monospace family alone; putting it at the front of
    // both was what rendered every menu and dialog in a coding face. The
    // toolkit's default faces stay behind both for symbol and emoji
    // fallback, per the decision recorded on the egui line of Cargo.toml.
    // The renderer's committed glyph atlas is independent: it is baked
    // offline from the monospaced face and untouched by this chain.
    fonts.font_data.insert(
        "inter".to_owned(),
        egui::FontData::from_static(include_bytes!("../../../../res/Inter/Inter-Medium.ttf"))
            .into(),
    );
    fonts.font_data.insert(
        "lilex".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../../../res/Lilex/static/Lilex-Medium.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "lilex".to_owned());
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ink that names a colour instead of reading the theme is the bug class
    /// that made the console log invisible: `Color32::from_white_alpha` is
    /// white, which only resolves on a dark ground, and the light theme is
    /// warm cream paper.
    ///
    /// Scoped to the two known-safe exceptions:
    ///
    /// - `overlays.rs` paints white on its OWN `from_black_alpha` chip
    ///   (`overlay_frame`), so it is self-grounded and floats over the 3D
    ///   scene rather than over themed chrome.
    /// - `review_overlay.rs`/`review_panel.rs` put ink on a saturated
    ///   category hue, which is the `ink_on_attention` pattern.
    #[test]
    fn gui_chrome_does_not_hardcode_ink() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gui");
        let exempt = [
            "overlays.rs",
            "review_overlay.rs",
            "review_panel.rs",
            "theme.rs",
        ];

        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("src/gui must exist") {
            let path = entry.expect("dir entry").path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") || exempt.contains(&name) {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read");
            for (i, line) in src.lines().enumerate() {
                // Skip comments: this file's own prose names the pattern.
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                if code.contains("from_white_alpha") || code.contains("Color32::WHITE") {
                    offenders.push(format!("{name}:{}", i + 1));
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "these paint white ink onto themed chrome, which disappears on the light theme. \
             Read the colour from `Theme` instead (or `ui.visuals().weak_text_color()` where no \
             theme is threaded): {offenders:?}"
        );
    }

    /// The egui mapping must not collapse two roles onto one colour: a
    /// widget that fills the same as its hover has no hover state.
    #[test]
    fn interactive_states_are_distinguishable() {
        for palette in [Palette::dark(), Palette::light()] {
            let t = Theme::from_palette(&palette);
            assert_ne!(t.widget_bg, t.widget_hover, "hover is invisible");
            assert_ne!(t.bg, t.fg, "text matches its background");
            assert_ne!(t.selection, t.bg, "selection is invisible");
        }
    }

    /// The review categories are the strongest correlation cue in the
    /// product, so they must stay four distinct hues on both themes.
    #[test]
    fn review_categories_stay_distinct() {
        for palette in [Palette::dark(), Palette::light()] {
            let r = Theme::from_palette(&palette).review;
            let all = [r.info, r.warning, r.question, r.change];
            for (i, a) in all.iter().enumerate() {
                for b in &all[i + 1..] {
                    assert_ne!(a, b, "two review categories share a colour");
                }
            }
        }
    }
}
