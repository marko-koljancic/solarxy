use solarxy_core::preferences::{
    self, MAX_RECENT_FILES_CAP, MAX_WINDOW_HEIGHT, MAX_WINDOW_WIDTH, MIN_WINDOW_HEIGHT,
    MIN_WINDOW_WIDTH, Preferences, UpdaterChannel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefsTab {
    Startup,
    Interface,
    Updater,
}

impl PrefsTab {
    const ALL: [Self; 3] = [Self::Startup, Self::Interface, Self::Updater];

    fn label(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
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
            PrefsTab::Interface => {
                self.draft.ui = defaults.ui;
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
            .color(egui::Color32::from_white_alpha(140)),
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
                .color(egui::Color32::from_white_alpha(140)),
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
                .color(egui::Color32::from_white_alpha(100)),
        );
    }
}

fn draw_interface_tab(ui: &mut egui::Ui, draft: &mut Preferences) {
    egui::Grid::new("prefs_interface")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Sidebar visible at launch");
            ui.checkbox(&mut draft.ui.default_sidebar_visible, "");
            ui.end_row();

            ui.label("FPS HUD visible at launch");
            ui.checkbox(&mut draft.ui.default_fps_hud_visible, "");
            ui.end_row();

            ui.label("Console docked at launch");
            ui.checkbox(&mut draft.ui.default_console_docked, "");
            ui.end_row();

            ui.label("Open Model Stats on model load");
            ui.checkbox(&mut draft.ui.open_stats_on_model_load, "");
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
            .color(egui::Color32::from_white_alpha(140)),
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
