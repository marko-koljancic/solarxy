//! Ayu Mirage-inspired flat dark theme + matching `egui_dock` style.
//!
//! Palette (warm dark blue-grey with an amber accent — designed to feel
//! at home alongside a 3D viewport without competing with the rendered
//! content):
//!
//! - `BG` `#1F2430` — panel + window background
//! - `FG` `#CCCAC2` — primary text
//! - `MUTED` `#5C6773` — secondary text, inactive tabs
//! - `ACCENT` `#FFC44C` — amber highlight (hyperlink, active hover,
//!   review-mode banner + edge stripe)
//! - `SELECTION` `#33415E` — selection fill + active tab body
//! - `WIDGET_BG` `#3D424D` — inactive widget fill (button, combo)
//! - `WIDGET_HOVER` `#2D323D` — widget hover fill
//!
//! All corner radii are zero — flat, professional, 3D-DCC-app feel.

pub(super) const BG: egui::Color32 = egui::Color32::from_rgb(0x1F, 0x24, 0x30);
pub(super) const FG: egui::Color32 = egui::Color32::from_rgb(0xCC, 0xCA, 0xC2);
pub(super) const MUTED: egui::Color32 = egui::Color32::from_rgb(0x5C, 0x67, 0x73);
pub(super) const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xC4, 0x4C);
pub(super) const SELECTION: egui::Color32 = egui::Color32::from_rgb(0x33, 0x41, 0x5E);
pub(super) const WIDGET_BG: egui::Color32 = egui::Color32::from_rgb(0x3D, 0x42, 0x4D);
pub(super) const WIDGET_HOVER: egui::Color32 = egui::Color32::from_rgb(0x2D, 0x32, 0x3D);

pub(super) fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = WIDGET_HOVER;
    visuals.code_bg_color = WIDGET_HOVER;
    visuals.override_text_color = Some(FG);
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = SELECTION;
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);

    let zero = egui::CornerRadius::ZERO;
    visuals.window_corner_radius = zero;
    visuals.menu_corner_radius = zero;

    // Widget palette across all interaction states.
    visuals.widgets.noninteractive.bg_fill = BG;
    visuals.widgets.noninteractive.weak_bg_fill = BG;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, WIDGET_BG);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, FG);
    visuals.widgets.noninteractive.corner_radius = zero;

    visuals.widgets.inactive.bg_fill = WIDGET_BG;
    visuals.widgets.inactive.weak_bg_fill = WIDGET_BG;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, FG);
    visuals.widgets.inactive.corner_radius = zero;

    visuals.widgets.hovered.bg_fill = WIDGET_HOVER;
    visuals.widgets.hovered.weak_bg_fill = WIDGET_HOVER;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, FG);
    visuals.widgets.hovered.corner_radius = zero;

    visuals.widgets.active.bg_fill = SELECTION;
    visuals.widgets.active.weak_bg_fill = SELECTION;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, FG);
    visuals.widgets.active.corner_radius = zero;

    visuals.widgets.open.bg_fill = WIDGET_HOVER;
    visuals.widgets.open.weak_bg_fill = WIDGET_HOVER;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, WIDGET_BG);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, FG);
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

/// Build the `egui_dock` style that pairs with `apply_theme`. Overrides
/// the defaults that produce the dark tab-bar strip + rounded leaf
/// corners: tab bar fill matches the panel (no contrasting strip),
/// every corner radius is zero, active tab uses the selection chip
/// fill, inactive tabs use the muted text color on the dim widget fill.
pub(super) fn make_dock_style(ctx: &egui::Context) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(ctx.style().as_ref());
    let zero = egui::CornerRadius::ZERO;

    style.main_surface_border_rounding = zero;
    style.main_surface_border_stroke = egui::Stroke::NONE;

    style.tab_bar.bg_fill = BG;
    style.tab_bar.corner_radius = zero;
    style.tab_bar.hline_color = WIDGET_BG;

    style.separator.color_idle = WIDGET_BG;
    style.separator.color_hovered = ACCENT;
    style.separator.color_dragged = ACCENT;

    let make_tab = |bg: egui::Color32, text: egui::Color32| egui_dock::TabInteractionStyle {
        outline_color: WIDGET_BG,
        corner_radius: zero,
        bg_fill: bg,
        text_color: text,
    };
    style.tab.active = make_tab(SELECTION, FG);
    style.tab.active_with_kb_focus = make_tab(SELECTION, FG);
    style.tab.focused = make_tab(SELECTION, FG);
    style.tab.focused_with_kb_focus = make_tab(SELECTION, FG);
    style.tab.inactive = make_tab(BG, MUTED);
    style.tab.inactive_with_kb_focus = make_tab(BG, MUTED);
    style.tab.hovered = make_tab(WIDGET_HOVER, FG);

    style.tab.tab_body.bg_fill = BG;
    style.tab.tab_body.corner_radius = zero;
    style.tab.tab_body.stroke = egui::Stroke::NONE;
    style.tab.hline_below_active_tab_name = false;

    style.buttons.close_tab_color = MUTED;
    style.buttons.close_tab_active_color = FG;
    style.buttons.close_tab_bg_fill = WIDGET_HOVER;
    style.buttons.add_tab_color = MUTED;
    style.buttons.add_tab_active_color = FG;
    style.buttons.add_tab_bg_fill = WIDGET_HOVER;
    style.buttons.add_tab_border_color = WIDGET_BG;

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
