use solarxy_core::install_source::{InstallSource, UpdateHint, detect, releases_url, update_hint};

pub(super) struct UpdateModalState {
    pub open: bool,
    source: InstallSource,
    hint: UpdateHint,
}

impl UpdateModalState {
    pub fn new() -> Self {
        let source = InstallSource::Unknown;
        let hint = update_hint(source);
        Self {
            open: false,
            source,
            hint,
        }
    }

    pub fn refresh(&mut self) {
        self.source = detect();
        self.hint = update_hint(self.source);
        self.open = true;
    }
}

pub(super) fn draw_update_modal(ctx: &egui::Context, state: &mut UpdateModalState) {
    if !state.open {
        return;
    }

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        state.open = false;
        return;
    }

    let title = "Check for Updates";
    let mut open = state.open;

    egui::Window::new(title)
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Check for Updates");
                ui.add_space(6.0);
                ui.label(format!("Solarxy v{}", env!("CARGO_PKG_VERSION")));
                ui.add_space(8.0);
                ui.label(install_label(state.source));
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                match &state.hint {
                    UpdateHint::OpenUrl(url) => {
                        ui.label("Updates are managed by your installation source:");
                        ui.add_space(4.0);
                        ui.hyperlink_to(url.as_str(), url.as_str());
                    }
                    UpdateHint::ShowCommand(cmd) => {
                        ui.label("Run the following command to update:");
                        ui.add_space(6.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut cmd.as_str())
                                .desired_width(380.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.add_space(8.0);
                        if ui.button("Copy command").clicked() {
                            ui.ctx().copy_text(cmd.clone());
                        }
                    }
                    UpdateHint::OpenReleasesPage => {
                        ui.label("Download the latest release from GitHub:");
                        ui.add_space(4.0);
                        ui.hyperlink_to(releases_url(), releases_url());
                    }
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(80.0);
                    if ui.button("Open releases page").clicked()
                        && let Err(e) = open::that(releases_url())
                    {
                        tracing::warn!("Failed to open releases page: {e}");
                    }
                    if ui.button("Close").clicked() {
                        state.open = false;
                    }
                });
            });
        });

    state.open = open && state.open;
}

fn install_label(source: InstallSource) -> String {
    let kind = match source {
        InstallSource::Flatpak => "Flatpak (Flathub)",
        InstallSource::AppImage => "AppImage",
        InstallSource::HomebrewCask => "Homebrew Cask",
        InstallSource::HomebrewFormula => "Homebrew formula",
        InstallSource::Msi => "Windows MSI",
        InstallSource::Winget => "Windows (winget)",
        InstallSource::DmgDirect => "macOS .dmg",
        InstallSource::CargoInstall => "cargo install",
        InstallSource::Unknown => "Unknown",
    };
    format!("Install source: {kind}")
}
