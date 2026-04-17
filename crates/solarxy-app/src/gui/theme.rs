pub(super) fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    let panel_bg = egui::Color32::from_rgba_unmultiplied(30, 30, 40, 217);
    visuals.panel_fill = panel_bg;
    visuals.window_fill = panel_bg;

    let corner = egui::CornerRadius::same(4);
    visuals.widgets.noninteractive.corner_radius = corner;
    visuals.widgets.inactive.corner_radius = corner;
    visuals.widgets.hovered.corner_radius = corner;
    visuals.widgets.active.corner_radius = corner;
    visuals.widgets.open.corner_radius = corner;
    visuals.window_corner_radius = corner;

    let accent = egui::Color32::from_rgb(80, 200, 190);
    visuals.selection.bg_fill = accent;
    visuals.hyperlink_color = accent;
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgba_unmultiplied(80, 200, 190, 40);

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
