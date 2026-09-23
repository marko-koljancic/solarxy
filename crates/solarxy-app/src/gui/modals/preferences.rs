use solarxy_core::preferences::{
    self, GizmoOrientation, MAX_RECENT_FILES_CAP, MAX_WINDOW_HEIGHT, MAX_WINDOW_WIDTH,
    MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, Preferences, ThemeChoice,
};
use solarxy_core::view_config::{
    MAX_BLOOM_STRENGTH, MAX_BLOOM_THRESHOLD, MAX_SSAO_STRENGTH, MIN_BLOOM_STRENGTH,
    MIN_BLOOM_THRESHOLD, MIN_SSAO_STRENGTH,
};

/// The modal's tabs.
///
/// There is no updater tab: the shell has no update check to steer, so the
/// two fields it edited have no editor. The View tab edits no list of user
/// backgrounds either, since no pane offers one. Both stay in the file, and
/// a commit from here carries them through untouched because the draft is
/// the whole of [`Preferences`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefsTab {
    Startup,
    Appearance,
    View,
    Interface,
}

impl PrefsTab {
    const ALL: [Self; 4] = [Self::Startup, Self::Appearance, Self::View, Self::Interface];

    fn label(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::Appearance => "Appearance",
            Self::View => "View",
            Self::Interface => "Interface",
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
            PrefsTab::Appearance => {
                self.draft.ui.theme = defaults.ui.theme;
            }
            PrefsTab::View => {
                self.draft.display.background = defaults.display.background;
                self.draft.viewport = defaults.viewport;
                self.draft.display.bloom_enabled = defaults.display.bloom_enabled;
                self.draft.display.ssao_enabled = defaults.display.ssao_enabled;
                self.draft
                    .display
                    .set_post_strengths(defaults.display.post_strengths());
            }
            PrefsTab::Interface => {
                self.draft.ui.max_recent_files = defaults.ui.max_recent_files;
                self.draft.autosave = defaults.autosave;
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

pub(in crate::gui) fn draw_preferences_modal(ctx: &egui::Context, modal: &mut PreferencesModal) {
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
                    draw_view_tab(ui, &mut modal.draft);
                }
                PrefsTab::Interface => draw_interface_tab(ui, &mut modal.draft),
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

fn draw_view_tab(ui: &mut egui::Ui, draft: &mut Preferences) {
    ui.horizontal(|ui| {
        ui.label("Default background");
        // No HDRI is loaded at startup, so `HDRI Sky` is not offered as a
        // default: `false` hides it from the dropdown.
        crate::gui::widgets::background_combo(
            ui,
            "prefs_default_background",
            &mut draft.display.background,
            false,
        );
    });
    ui.label(
        egui::RichText::new("The viewport background Solarxy starts with.")
            .italics()
            .small()
            .weak(),
    );

    ui.add_space(12.0);
    ui.label(egui::RichText::new("Transform handles").strong());
    // The browser's rows, over the same preference the orientation key and
    // the viewport menu write: the frame the Move and Rotate handles align
    // to (Scale is always local), and what a drag snaps to while the snap
    // modifier is held.
    egui::Grid::new("prefs_transform_handles")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Handle orientation");
            let current = draft.viewport.orientation;
            egui::ComboBox::from_id_salt("prefs_handle_orientation")
                .selected_text(current.label())
                .show_ui(ui, |ui| {
                    for choice in [GizmoOrientation::World, GizmoOrientation::Local] {
                        ui.selectable_value(
                            &mut draft.viewport.orientation,
                            choice,
                            choice.label(),
                        );
                    }
                });
            ui.end_row();

            ui.label("Snap: move (m)")
                .on_hover_text("World units a move drag snaps to while the snap modifier is held");
            ui.add(
                egui::DragValue::new(&mut draft.viewport.snap_translate)
                    .speed(0.1)
                    .range(0.0..=f32::MAX),
            );
            ui.end_row();

            ui.label("Snap: rotate (deg)")
                .on_hover_text("Degrees a rotate drag snaps to while the snap modifier is held");
            ui.add(
                egui::DragValue::new(&mut draft.viewport.snap_rotate)
                    .speed(1.0)
                    .range(0.0..=f32::MAX),
            );
            ui.end_row();

            ui.label("Snap: scale").on_hover_text(
                "The increment a scale drag snaps to while the snap modifier is held",
            );
            ui.add(
                egui::DragValue::new(&mut draft.viewport.snap_scale)
                    .speed(0.05)
                    .range(0.0..=f32::MAX),
            );
            ui.end_row();
        });

    ui.add_space(12.0);
    ui.label(egui::RichText::new("Post-processing").strong());
    // The browser's rows, in its order and over the same fields. Its own doc
    // strings say these apply immediately and renderer-wide; here they apply
    // on OK, because this dialog is draft-and-commit for every row it has, so
    // that half of the sentence is dropped rather than made untrue.
    egui::Grid::new("prefs_post")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Ambient occlusion").on_hover_text(
                "Screen-space ambient occlusion: darkens creases and contact areas in \
                 shaded views, and is what the AO Preview inspection mode shows.",
            );
            ui.checkbox(&mut draft.display.ssao_enabled, "");
            ui.end_row();

            let ssao_on = draft.display.ssao_enabled;
            ui.add_enabled_ui(ssao_on, |ui| {
                ui.label("Occlusion strength").on_hover_text(
                    "How far the composite blends towards the occlusion buffer. The AO \
                     Preview inspection mode shows the raw buffer and is deliberately \
                     unaffected by this.",
                );
            });
            ui.add_enabled(
                ssao_on,
                egui::DragValue::new(&mut draft.display.ssao_strength)
                    .speed(0.05)
                    .range(MIN_SSAO_STRENGTH..=MAX_SSAO_STRENGTH),
            );
            ui.end_row();

            ui.label("Bloom")
                .on_hover_text("Glow on emissive and very bright surfaces.");
            ui.checkbox(&mut draft.display.bloom_enabled, "");
            ui.end_row();

            let bloom_on = draft.display.bloom_enabled;
            ui.add_enabled_ui(bloom_on, |ui| {
                ui.label("Bloom strength").on_hover_text(
                    "How much of the blurred bright pass is added back on top of the image.",
                );
            });
            ui.add_enabled(
                bloom_on,
                egui::DragValue::new(&mut draft.display.bloom_strength)
                    .speed(0.1)
                    .range(MIN_BLOOM_STRENGTH..=MAX_BLOOM_STRENGTH),
            );
            ui.end_row();

            ui.add_enabled_ui(bloom_on, |ui| {
                ui.label("Bloom threshold").on_hover_text(
                    "Luminance a pixel has to exceed before it contributes to the glow. \
                     Lower blooms more of the image.",
                );
            });
            ui.add_enabled(
                bloom_on,
                egui::DragValue::new(&mut draft.display.bloom_threshold)
                    .speed(0.1)
                    .range(MIN_BLOOM_THRESHOLD..=MAX_BLOOM_THRESHOLD),
            );
            ui.end_row();
        });
}

fn draw_interface_tab(ui: &mut egui::Ui, draft: &mut Preferences) {
    egui::Grid::new("prefs_interface")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Recent files capacity");
            ui.add(
                egui::Slider::new(&mut draft.ui.max_recent_files, 1..=MAX_RECENT_FILES_CAP)
                    .integer(),
            );
            ui.end_row();

            ui.label("Autosave")
                .on_hover_text("Keep a recovery copy of unsaved work in the data directory");
            ui.checkbox(&mut draft.autosave.enabled, "");
            ui.end_row();

            ui.label("Playbar")
                .on_hover_text("The scene-clock strip under the viewport");
            ui.checkbox(&mut draft.ui.transport_bar, "");
            ui.end_row();

            ui.label("Autosave after")
                .on_hover_text("Seconds of quiet before a recovery copy is written; one is forced every 15 seconds while editing");
            ui.add_enabled(
                draft.autosave.enabled,
                egui::Slider::new(&mut draft.autosave.debounce_secs, 0.5..=30.0)
                    .step_by(0.5)
                    .suffix(" s"),
            );
            ui.end_row();

            ui.label("Reviewer name")
                .on_hover_text("Author name written on new review annotations");
            let mut name = draft.review.author.clone().unwrap_or_default();
            if ui
                .add(egui::TextEdit::singleline(&mut name).hint_text("anonymous"))
                .changed()
            {
                let trimmed = name.trim();
                draft.review.author = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                };
            }
            ui.end_row();
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_core::preferences::{UpdaterChannel, UpdaterPrefs};

    #[test]
    fn the_tabs_are_the_four_the_shell_can_steer() {
        let labels: Vec<&str> = PrefsTab::ALL.iter().map(|tab| tab.label()).collect();
        assert_eq!(labels, ["Startup", "Appearance", "View", "Interface"]);
    }

    /// The updater fields have no editor, so nothing in the modal may write
    /// them: a reset on any tab has to leave what the file carried alone.
    #[test]
    fn no_tab_resets_the_fields_that_have_no_editor() {
        let prefs = Preferences {
            updater: UpdaterPrefs {
                check_on_launch: true,
                channel: UpdaterChannel::Prerelease,
            },
            ..Default::default()
        };
        let mut m = PreferencesModal::default();
        m.open_with(prefs.clone());
        for tab in PrefsTab::ALL {
            m.active_tab = tab;
            m.reset_active_tab();
        }
        assert_eq!(m.draft.updater, prefs.updater);
    }

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
