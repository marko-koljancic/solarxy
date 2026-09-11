//! The Environment modal: the scene's HDRI, its image-based lighting mode,
//! and the rotation and intensity of the light it casts.
//!
//! The browser reaches the environment through this dialog from the
//! Viewport panel's View menu; this shell reaches it from the File menu
//! until the viewport menu bar exists, and the Properties panel, which
//! used to carry these rows, is the parameter panel now. The rows raise
//! the same intents they always did; only the surface moved.

use solarxy_core::preferences::IblMode;

use crate::gui::intent::{DisplayChange, Intent, Intents, PanelIntent};
use crate::gui::settings::PanelSettings;
use crate::gui::theme::Theme;
use crate::state::hdri_info::HdriInfo;

/// What the dialog says about the loaded map.
pub(crate) fn hdri_label(hdri: Option<&HdriInfo>) -> String {
    hdri.map_or_else(|| "None".to_string(), |h| h.filename.clone())
}

pub(in crate::gui) fn draw_environment_modal(
    ctx: &egui::Context,
    open: &mut bool,
    settings: PanelSettings<'_>,
    hdri: Option<&HdriInfo>,
    intents: &mut Intents,
    theme: &Theme,
) {
    if !*open {
        return;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        *open = false;
        return;
    }
    let mut done = false;
    egui::Window::new("Environment")
        .id(egui::Id::new("solarxy_environment"))
        .collapsible(false)
        .resizable(false)
        .movable(true)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(360.0);
            egui::Grid::new("environment_rows")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("HDRI")
                        .on_hover_text("An equirectangular .hdr or .exr map that lights the scene");
                    ui.horizontal(|ui| {
                        ui.label(hdri_label(hdri));
                        if ui.button("Load\u{2026}").clicked() {
                            intents.panel(PanelIntent::LoadHdri);
                        }
                        if ui
                            .add_enabled(hdri.is_some(), egui::Button::new("Clear"))
                            .clicked()
                        {
                            intents.panel(PanelIntent::ClearHdri);
                        }
                    });
                    ui.end_row();

                    ui.label("IBL mode")
                        .on_hover_text("How much of the map lights the scene");
                    egui::ComboBox::from_id_salt("environment_ibl")
                        .selected_text(settings.ibl_mode.to_string())
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            let mut mode = settings.ibl_mode;
                            for &variant in IblMode::ALL {
                                if ui
                                    .selectable_value(&mut mode, variant, variant.to_string())
                                    .changed()
                                {
                                    intents.raise(Intent::Ibl(variant));
                                }
                            }
                        });
                    ui.end_row();

                    ui.label("Rotation")
                        .on_hover_text("Yaw the map and the light it casts, in degrees");
                    let mut degrees = settings.display.hdri_rotation.to_degrees();
                    if ui
                        .add(
                            egui::DragValue::new(&mut degrees)
                                .speed(5.0)
                                .range(0.0..=360.0)
                                .suffix("\u{00b0}"),
                        )
                        .changed()
                    {
                        intents.raise(Intent::Display(DisplayChange::HdriRotation(
                            degrees.to_radians(),
                        )));
                    }
                    ui.end_row();

                    ui.label("Intensity")
                        .on_hover_text("Scale the light the map casts, without dimming the sky");
                    let mut intensity = settings.display.hdri_intensity;
                    if ui
                        .add(
                            egui::DragValue::new(&mut intensity)
                                .speed(0.1)
                                .range(
                                    solarxy_core::view_config::MIN_HDRI_INTENSITY
                                        ..=solarxy_core::view_config::MAX_HDRI_INTENSITY,
                                ),
                        )
                        .changed()
                    {
                        intents.raise(Intent::Display(DisplayChange::HdriIntensity(intensity)));
                    }
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "Show the HDRI as a pane background via the pane toolbar's background menu (Sky).",
                )
                .color(theme.muted)
                .size(10.0),
            );
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    done = true;
                }
            });
        });
    if done {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog names the loaded map by its file name and says None
    /// without one, which is what the browser's row shows.
    #[test]
    fn the_hdri_row_names_the_file_or_none() {
        assert_eq!(hdri_label(None), "None");
        let info = HdriInfo {
            filename: "studio.hdr".to_string(),
            path: "/maps/studio.hdr".to_string(),
        };
        assert_eq!(hdri_label(Some(&info)), "studio.hdr");
    }
}
