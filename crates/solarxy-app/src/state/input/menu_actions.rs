use super::super::State;

impl State {
    pub(in crate::state) fn handle_menu_actions(&mut self, actions: crate::gui::MenuActions) {
        if actions.open_model {
            self.open_model_dialog();
        }
        if actions.open_hdri {
            self.open_hdri_dialog();
        }
        if actions.save_screenshot {
            self.capture_requested = true;
        }
        if actions.close_model {
            self.close_model();
        }
        if actions.save_preferences {
            self.save_preferences();
        }
        if let Some(path) = actions.open_recent {
            self.spawn_load(path);
        }
        if actions.open_config_file
            && let Some(path) = solarxy_core::preferences::config_path()
            && let Err(e) = open::that(path)
        {
            tracing::warn!("Failed to open config file: {e}");
        }
        if actions.open_preferences {
            self.gui.open_preferences(self.preferences.clone());
        }
        if actions.open_shortcuts_modal {
            self.gui.open_shortcuts_modal();
        }
        if actions.open_wiki {
            let url = concat!(env!("CARGO_PKG_REPOSITORY"), "/wiki");
            if let Err(e) = open::that(url) {
                tracing::warn!("Failed to open wiki URL: {e}");
            }
        }
        if actions.open_about {
            self.gui.open_about();
        }
        if actions.check_for_updates {
            self.gui.check_for_updates();
        }
        if let Some(layout) = actions.set_layout {
            self.set_view_layout(layout);
        }
        if let Some(proj) = actions.set_projection {
            self.for_each_target_cam(|cam| cam.set_projection(proj));
        }
        if actions.quit {
            self.quit_requested = true;
        }
    }
}
