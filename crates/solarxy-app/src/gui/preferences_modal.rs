use solarxy_core::preferences::{
    self, CustomBackground, CustomBgKind, MAX_RECENT_FILES_CAP, MAX_WINDOW_HEIGHT,
    MAX_WINDOW_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, Preferences, ThemeChoice,
    UpdaterChannel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefsTab {
    Startup,
    Appearance,
    View,
    Interface,
    Updater,
}

impl PrefsTab {
    const ALL: [Self; 5] = [
        Self::Startup,
        Self::Appearance,
        Self::View,
        Self::Interface,
        Self::Updater,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::Appearance => "Appearance",
            Self::View => "View",
            Self::Interface => "Interface",
            Self::Updater => "Updater",
        }
    }
}

#[derive(Debug)]
pub struct PreferencesModal {
    pub open: bool,
    draft: Preferences,
    snapshot: Preferences,
    active_tab: PrefsTab,
    save_error: Option<String>,
    committed: Option<Preferences>,
    /// Index into `draft.view.custom_backgrounds` of the custom currently
    /// open in the View tab's inline editor, if any.
    editing_custom: Option<usize>,
}

impl Default for PreferencesModal {
    fn default() -> Self {
        Self {
            open: false,
            draft: Preferences::default(),
            snapshot: Preferences::default(),
            active_tab: PrefsTab::Startup,
            save_error: None,
            committed: None,
            editing_custom: None,
        }
    }
}

impl PreferencesModal {
    pub fn open_with(&mut self, prefs: Preferences) {
        self.draft = prefs.clone();
        self.snapshot = prefs;
        self.active_tab = PrefsTab::Startup;
        self.save_error = None;
        self.committed = None;
        self.editing_custom = None;
        self.open = true;
    }

    pub fn take_committed(&mut self) -> Option<Preferences> {
        self.committed.take()
    }

    fn reset_active_tab(&mut self) {
        let defaults = Preferences::default();
        match self.active_tab {
            PrefsTab::Startup => {
                self.draft.window = defaults.window;
                self.draft.rendering.msaa_sample_count = defaults.rendering.msaa_sample_count;
            }
            PrefsTab::Appearance => {
                self.draft.ui.theme = defaults.ui.theme;
            }
            PrefsTab::View => {
                // Reset only the default-background choice; custom
                // backgrounds are user data — removed one at a time.
                self.draft.display.background = defaults.display.background;
                self.editing_custom = None;
            }
            PrefsTab::Interface => {
                self.draft.ui.max_recent_files = defaults.ui.max_recent_files;
                self.draft.ui.status_bar_visible = defaults.ui.status_bar_visible;
            }
            PrefsTab::Updater => {
                self.draft.updater = defaults.updater;
            }
        }
    }

    fn cancel(&mut self) {
        self.draft = self.snapshot.clone();
        self.save_error = None;
        self.open = false;
    }

    fn ok(&mut self) {
        match preferences::save(&self.draft) {
            Ok(()) => {
                self.committed = Some(self.draft.clone());
                self.save_error = None;
                self.open = false;
            }
            Err(e) => {
                self.save_error = Some(e);
            }
        }
    }
}

pub(super) fn draw_preferences_modal(ctx: &egui::Context, modal: &mut PreferencesModal) {
    if !modal.open {
        return;
    }

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        modal.cancel();
        return;
    }

    let mut open_flag = modal.open;
    let default_pos = ctx.content_rect().center() - egui::vec2(230.0, 240.0);
    egui::Window::new("Preferences")
        .open(&mut open_flag)
        .resizable(false)
        .collapsible(false)
        .default_pos(default_pos)
        .default_width(460.0)
        .movable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for tab in PrefsTab::ALL {
                    if ui
                        .selectable_label(modal.active_tab == tab, tab.label())
                        .clicked()
                    {
                        modal.active_tab = tab;
                    }
                }
            });
            ui.separator();
            ui.add_space(4.0);

            match modal.active_tab {
                PrefsTab::Startup => draw_startup_tab(ui, &mut modal.draft),
                PrefsTab::Appearance => draw_appearance_tab(ui, &mut modal.draft),
                PrefsTab::View => {
                    draw_view_tab(ui, &mut modal.draft, &mut modal.editing_custom);
                }
                PrefsTab::Interface => draw_interface_tab(ui, &mut modal.draft),
                PrefsTab::Updater => draw_updater_tab(ui, &mut modal.draft),
            }

            ui.add_space(8.0);
            if let Some(err) = &modal.save_error {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 110, 110),
                    format!("Save failed: {err}"),
                );
                ui.add_space(4.0);
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Reset to defaults").clicked() {
                    modal.reset_active_tab();
                }
                ui.add_space(8.0);
                let dirty = modal.draft != modal.snapshot;
                ui.add_enabled_ui(true, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("OK").clicked() {
                            modal.ok();
                        }
                        if ui.button("Cancel").clicked() {
                            modal.cancel();
                        }
                        if dirty {
                            ui.label(
                                egui::RichText::new("modified")
                                    .italics()
                                    .color(egui::Color32::from_rgb(210, 180, 90)),
                            );
                        }
                    });
                });
            });
        });

    if !open_flag && modal.open {
        modal.cancel();
    }
}

fn draw_startup_tab(ui: &mut egui::Ui, draft: &mut Preferences) {
    egui::Grid::new("prefs_startup")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Window width");
            ui.add(
                egui::DragValue::new(&mut draft.window.window_width)
                    .range(MIN_WINDOW_WIDTH..=MAX_WINDOW_WIDTH)
                    .speed(20.0)
                    .suffix(" px"),
            );
            ui.end_row();

            ui.label("Window height");
            ui.add(
                egui::DragValue::new(&mut draft.window.window_height)
                    .range(MIN_WINDOW_HEIGHT..=MAX_WINDOW_HEIGHT)
                    .speed(20.0)
                    .suffix(" px"),
            );
            ui.end_row();

            ui.label("MSAA");
            ui.horizontal(|ui| {
                for samples in [1u32, 2, 4] {
                    if ui
                        .selectable_label(
                            draft.rendering.msaa_sample_count == samples,
                            format!("{samples}x"),
                        )
                        .clicked()
                    {
                        draft.rendering.msaa_sample_count = samples;
                    }
                }
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("Window size and MSAA take effect on next launch.")
            .italics()
            .small()
            .weak(),
    );

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Config File").strong());
    ui.add_space(4.0);
    if let Some(path) = preferences::config_path() {
        ui.label(
            egui::RichText::new(path.display().to_string())
                .small()
                .weak(),
        );
        ui.add_space(4.0);
        if ui.button("Open config file").clicked()
            && let Err(e) = open::that(&path)
        {
            tracing::warn!("Failed to open config file: {e}");
        }
    } else {
        ui.label(
            egui::RichText::new("(config path unavailable)")
                .small()
                .italics()
                .weak(),
        );
    }
}

fn draw_appearance_tab(ui: &mut egui::Ui, draft: &mut Preferences) {
    egui::Grid::new("prefs_appearance")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Theme");
            ui.horizontal(|ui| {
                for choice in ThemeChoice::ALL {
                    if ui
                        .selectable_label(draft.ui.theme == *choice, choice.to_string())
                        .clicked()
                    {
                        draft.ui.theme = *choice;
                    }
                }
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("The theme applies immediately when you click OK.")
            .italics()
            .small()
            .weak(),
    );
}

fn draw_view_tab(ui: &mut egui::Ui, draft: &mut Preferences, editing: &mut Option<usize>) {
    ui.horizontal(|ui| {
        ui.label("Default background");
        // No HDRI is loaded at startup, so `HDRI Sky` is not offered as a
        // default — `false` hides it from the dropdown.
        super::pane_toolbar::background_combo(
            ui,
            "prefs_default_background",
            &mut draft.display.background,
            &draft.view.custom_backgrounds,
            false,
        );
    });
    ui.label(
        egui::RichText::new("The viewport background Solarxy starts with.")
            .italics()
            .small()
            .weak(),
    );

    ui.add_space(10.0);
    ui.label(egui::RichText::new("Custom Backgrounds").strong());
    ui.add_space(2.0);

    let mut to_delete: Option<usize> = None;
    for i in 0..draft.view.custom_backgrounds.len() {
        ui.horizontal(|ui| {
            let custom = &draft.view.custom_backgrounds[i];
            let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 16.0), egui::Sense::hover());
            paint_bg_swatch(ui, rect, custom);
            ui.label(&custom.name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Delete").clicked() {
                    to_delete = Some(i);
                }
                if ui.button("Edit").clicked() {
                    *editing = Some(i);
                }
            });
        });
    }
    if draft.view.custom_backgrounds.is_empty() {
        ui.label(
            egui::RichText::new("No custom backgrounds yet.")
                .italics()
                .small()
                .weak(),
        );
    }

    ui.add_space(2.0);
    if ui.button("+ Add").clicked() {
        let id = draft.view.next_custom_id;
        draft.view.next_custom_id = id.wrapping_add(1);
        draft.view.custom_backgrounds.push(CustomBackground {
            id,
            name: format!("Background {}", id.saturating_add(1)),
            kind: CustomBgKind::Solid,
            top: [0.45, 0.46, 0.50],
            bottom: [0.08, 0.08, 0.11],
        });
        *editing = Some(draft.view.custom_backgrounds.len() - 1);
    }

    if let Some(i) = to_delete {
        draft.view.custom_backgrounds.remove(i);
        // Keep the inline editor anchored to the same custom — or close
        // it if the deleted entry was the one being edited.
        *editing = match *editing {
            Some(e) if e == i => None,
            Some(e) if e > i => Some(e - 1),
            other => other,
        };
    }

    if let Some(i) = *editing {
        if draft.view.custom_backgrounds.get(i).is_some() {
            ui.add_space(8.0);
            ui.separator();
            draw_custom_editor(ui, &mut draft.view.custom_backgrounds[i], editing);
        } else {
            *editing = None;
        }
    }
}

/// The inline editor revealed below the custom list when one is being
/// edited (Add or Edit). `editing` is cleared when `Done` is pressed.
fn draw_custom_editor(
    ui: &mut egui::Ui,
    custom: &mut CustomBackground,
    editing: &mut Option<usize>,
) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Editing custom background").strong());
    ui.add_space(4.0);
    egui::Grid::new("prefs_custom_bg_editor")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut custom.name);
            ui.end_row();

            ui.label("Kind");
            ui.horizontal(|ui| {
                ui.radio_value(&mut custom.kind, CustomBgKind::Solid, "Solid");
                ui.radio_value(&mut custom.kind, CustomBgKind::Gradient, "Gradient");
            });
            ui.end_row();

            match custom.kind {
                CustomBgKind::Solid => {
                    ui.label("Color");
                    ui.color_edit_button_rgb(&mut custom.top);
                    ui.end_row();
                }
                CustomBgKind::Gradient => {
                    ui.label("Top color");
                    ui.color_edit_button_rgb(&mut custom.top);
                    ui.end_row();
                    ui.label("Bottom color");
                    ui.color_edit_button_rgb(&mut custom.bottom);
                    ui.end_row();
                }
            }

            ui.label("Preview");
            let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 48.0), egui::Sense::hover());
            paint_bg_swatch(ui, rect, custom);
            ui.end_row();
        });
    ui.add_space(4.0);
    if ui.button("Done").clicked() {
        *editing = None;
    }
}

/// Paint a custom background's appearance into `rect` — a flat fill for
/// `Solid`, a real vertical gradient mesh for `Gradient`.
fn paint_bg_swatch(ui: &egui::Ui, rect: egui::Rect, custom: &CustomBackground) {
    match custom.kind {
        CustomBgKind::Solid => {
            ui.painter().rect_filled(rect, 0.0, bg_color32(custom.top));
        }
        CustomBgKind::Gradient => {
            let mut mesh = egui::Mesh::default();
            let top = bg_color32(custom.top);
            let bottom = bg_color32(custom.bottom);
            mesh.colored_vertex(rect.left_top(), top);
            mesh.colored_vertex(rect.right_top(), top);
            mesh.colored_vertex(rect.left_bottom(), bottom);
            mesh.colored_vertex(rect.right_bottom(), bottom);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(2, 1, 3);
            ui.painter().add(mesh);
        }
    }
    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
        egui::StrokeKind::Inside,
    );
}

fn bg_color32(c: [f32; 3]) -> egui::Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(b(c[0]), b(c[1]), b(c[2]))
}

fn draw_interface_tab(ui: &mut egui::Ui, draft: &mut Preferences) {
    egui::Grid::new("prefs_interface")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Status bar visible at launch");
            ui.checkbox(&mut draft.ui.status_bar_visible, "");
            ui.end_row();

            ui.label("Recent files capacity");
            ui.add(
                egui::Slider::new(&mut draft.ui.max_recent_files, 1..=MAX_RECENT_FILES_CAP)
                    .integer(),
            );
            ui.end_row();
        });
}

fn draw_updater_tab(ui: &mut egui::Ui, draft: &mut Preferences) {
    egui::Grid::new("prefs_updater")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Check for updates on launch");
            ui.checkbox(&mut draft.updater.check_on_launch, "");
            ui.end_row();

            ui.label("Release channel");
            ui.horizontal(|ui| {
                for channel in [UpdaterChannel::Stable, UpdaterChannel::Prerelease] {
                    if ui
                        .selectable_label(draft.updater.channel == channel, channel.to_string())
                        .clicked()
                    {
                        draft.updater.channel = channel;
                    }
                }
            });
            ui.end_row();
        });
    if draft.updater.channel == UpdaterChannel::Prerelease {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "Prerelease channel includes release candidates and betas; \
                 the stable channel ships tagged releases only.",
            )
            .italics()
            .small()
            .weak(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_with_captures_snapshot_and_draft() {
        let mut m = PreferencesModal::default();
        let mut prefs = Preferences::default();
        prefs.window.window_width = 1440;
        m.open_with(prefs.clone());
        assert!(m.open);
        assert_eq!(m.draft.window.window_width, 1440);
        assert_eq!(m.snapshot.window.window_width, 1440);
        assert_eq!(m.active_tab, PrefsTab::Startup);
    }

    #[test]
    fn cancel_restores_snapshot() {
        let mut m = PreferencesModal::default();
        m.open_with(Preferences::default());
        m.draft.window.window_width = 2560;
        m.cancel();
        assert!(!m.open);
        assert_eq!(m.draft, Preferences::default());
    }

    #[test]
    fn reset_active_tab_only_mutates_that_tab() {
        let mut m = PreferencesModal::default();
        m.open_with(Preferences::default());
        m.draft.window.window_width = 2560;
        m.draft.ui.max_recent_files = 5;
        m.active_tab = PrefsTab::Startup;
        m.reset_active_tab();
        assert_eq!(
            m.draft.window.window_width,
            Preferences::default().window.window_width
        );
        assert_eq!(m.draft.ui.max_recent_files, 5);
    }

    #[test]
    fn ok_populates_committed_only_when_save_succeeds() {
        let mut m = PreferencesModal::default();
        assert!(m.take_committed().is_none());
    }
}
