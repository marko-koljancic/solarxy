//! Ayu Mirage theme system — Dark + Light presets + matching `egui_dock`
//! style.
//!
//! [`Theme`] is a flat bundle of `Color32` tokens. Because every field is
//! `Copy`, `Theme` itself is `Copy` and threads through draw code without
//! lifetime noise. `EguiRenderer` owns the active `Theme`; [`apply_theme`]
//! pushes it into the egui `Context` and is re-run on a live theme swap.
//!
//! Two presets ship: [`Theme::ayu_mirage_dark`] (the original warm
//! dark-blue-grey palette) and [`Theme::ayu_mirage_light`] (Ayu Light's
//! near-white surface with the orange accent). Both are selected via
//! [`solarxy_core::preferences::ThemeChoice`].
//!
//! All corner radii are zero — flat, professional, 3D-DCC-app feel.

use solarxy_core::preferences::ThemeChoice;

const fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// The four Review-System category colors plus the shared selection
/// accent. Kept theme-scoped so the light preset can re-contrast the
/// category hues against a near-white background.
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
    #[allow(dead_code)]
    pub severity_error: egui::Color32,
    pub severity_warn: egui::Color32,
    #[allow(dead_code)]
    pub severity_info: egui::Color32,
    #[allow(dead_code)]
    pub severity_success: egui::Color32,
    pub review: ReviewColors,
}

impl Theme {
    /// Resolve a persisted [`ThemeChoice`] into a concrete palette.
    pub(super) fn from_choice(choice: ThemeChoice) -> Self {
        match choice {
            ThemeChoice::AyuMirageDark => Self::ayu_mirage_dark(),
            ThemeChoice::AyuMirageLight => Self::ayu_mirage_light(),
        }
    }

    /// Warm dark blue-grey with an amber accent — the original palette.
    pub(super) fn ayu_mirage_dark() -> Self {
        Self {
            dark: true,
            bg: rgb(0x1F, 0x24, 0x30),
            bg_elevated: rgb(0x23, 0x28, 0x34),
            fg: rgb(0xCC, 0xCA, 0xC2),
            muted: rgb(0x5C, 0x67, 0x73),
            accent: rgb(0xFF, 0xC4, 0x4C),
            selection: rgb(0x33, 0x41, 0x5E),
            widget_bg: rgb(0x3D, 0x42, 0x4D),
            widget_hover: rgb(0x2D, 0x32, 0x3D),
            border: rgb(0x3D, 0x42, 0x4D),
            severity_error: rgb(0xFF, 0x33, 0x33),
            severity_warn: rgb(0xFF, 0xC4, 0x4C),
            severity_info: rgb(0x78, 0xA0, 0xEE),
            severity_success: rgb(0x7F, 0xD9, 0x62),
            review: ReviewColors {
                info: rgb(0x5C, 0x9E, 0xFF),
                warning: rgb(0xFF, 0xB2, 0x3D),
                question: rgb(0xA0, 0x6D, 0xFF),
                change: rgb(0x3D, 0xC9, 0x7A),
                selection_accent: rgb(0xFF, 0xC4, 0x4C),
            },
        }
    }

    /// Ayu Light's near-white surface with the orange accent. Review
    /// category hues are darkened for AA contrast on the light ground.
    pub(super) fn ayu_mirage_light() -> Self {
        Self {
            dark: false,
            bg: rgb(0xFA, 0xFA, 0xFA),
            bg_elevated: rgb(0xF0, 0xF0, 0xF0),
            fg: rgb(0x5C, 0x67, 0x73),
            muted: rgb(0x82, 0x8C, 0x99),
            accent: rgb(0xFF, 0x6A, 0x00),
            selection: rgb(0xF0, 0xEE, 0xE4),
            widget_bg: rgb(0xE5, 0xE5, 0xE6),
            widget_hover: rgb(0xE8, 0xE8, 0xE8),
            border: rgb(0xD0, 0xD0, 0xD0),
            severity_error: rgb(0xC7, 0x37, 0x3B),
            severity_warn: rgb(0xF2, 0xAE, 0x49),
            severity_info: rgb(0x31, 0x99, 0xE1),
            severity_success: rgb(0x86, 0xB3, 0x00),
            review: ReviewColors {
                info: rgb(0x25, 0x63, 0xC9),
                warning: rgb(0xB7, 0x79, 0x1F),
                question: rgb(0x7C, 0x3A, 0xED),
                change: rgb(0x2F, 0x85, 0x5A),
                selection_accent: rgb(0xFF, 0x6A, 0x00),
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
    visuals.selection.stroke = egui::Stroke::new(1.0, theme.accent);

    let zero = egui::CornerRadius::ZERO;
    visuals.window_corner_radius = zero;
    visuals.menu_corner_radius = zero;

    visuals.widgets.noninteractive.bg_fill = theme.bg;
    visuals.widgets.noninteractive.weak_bg_fill = theme.bg;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, theme.widget_bg);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, theme.fg);
    visuals.widgets.noninteractive.corner_radius = zero;

    visuals.widgets.inactive.bg_fill = theme.widget_bg;
    visuals.widgets.inactive.weak_bg_fill = theme.widget_bg;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, theme.fg);
    visuals.widgets.inactive.corner_radius = zero;

    visuals.widgets.hovered.bg_fill = theme.widget_hover;
    visuals.widgets.hovered.weak_bg_fill = theme.widget_hover;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, theme.accent);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, theme.fg);
    visuals.widgets.hovered.corner_radius = zero;

    visuals.widgets.active.bg_fill = theme.selection;
    visuals.widgets.active.weak_bg_fill = theme.selection;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, theme.accent);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, theme.fg);
    visuals.widgets.active.corner_radius = zero;

    visuals.widgets.open.bg_fill = theme.widget_hover;
    visuals.widgets.open.weak_bg_fill = theme.widget_hover;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, theme.widget_bg);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, theme.fg);
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
        .insert(0, "lilex".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "lilex".to_owned());
    ctx.set_fonts(fonts);
}
