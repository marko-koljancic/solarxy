use super::super::State;
use crate::gui::ToastSeverity;

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
            self.screenshot_expand_review = false;
        }
        if actions.render_still {
            self.start_still_render();
        }
        if actions.close_model {
            self.close_document();
        }
        if actions.save_preferences || actions.save_view_defaults {
            self.save_preferences();
        }
        if let Some(path) = actions.open_recent {
            // Through the router, not the model loader: the one list holds
            // scenes and models, and the routing on extension exists once.
            self.open_file(std::path::PathBuf::from(path));
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
        if actions.open_wiki
            && let Err(e) = open::that(solarxy_core::WIKI_URL)
        {
            tracing::warn!("Failed to open wiki URL: {e}");
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
        if let Some(ratio) = actions.set_split_ratio {
            self.view.display.split_ratio =
                solarxy_core::view_config::DisplaySettings::clamp_split_ratio(ratio);
        }
        if actions.cancel_reanchor {
            self.gui
                .set_toast("Re-anchor cancelled", ToastSeverity::Info);
        }
        if actions.exit_review_mode {
            self.gui
                .set_toast("Review mode: Off", ToastSeverity::Success);
        }
        if actions.toggle_review_mode {
            self.toggle_review_mode();
        }
        if actions.toggle_review_markers {
            self.review.markers_hidden = !self.review.markers_hidden;
        }
        if actions.save_review_notes {
            self.save_review_sidecar();
        }
        if actions.show_all_meshes {
            self.handle_outliner_action(crate::gui::OutlinerAction::ShowAll);
        }
        if actions.save_dock_layout {
            if let Some(json) = self.gui.serialize_layout() {
                self.preferences.dock.saved_layout_json = Some(json);
                self.gui.set_has_saved_layout(true);
                // Persist silently so the click yields one layout-specific
                // toast, not a generic "Preferences saved" stacked on top.
                match self.persist_preferences() {
                    Ok(()) => self.gui.set_toast("Layout saved.", ToastSeverity::Success),
                    Err(e) => self
                        .gui
                        .set_toast(&format!("Save failed: {}", e), ToastSeverity::Error),
                }
            } else {
                self.gui
                    .set_toast("Failed to save layout.", ToastSeverity::Warning);
            }
        }
        if actions.restore_saved_layout {
            if let Some(json) = self.preferences.dock.saved_layout_json.clone()
                && self.gui.apply_layout_json(&json)
            {
                self.gui.set_toast("Layout restored.", ToastSeverity::Info);
            } else {
                self.gui
                    .set_toast("No valid saved layout to restore.", ToastSeverity::Warning);
            }
        }
        if actions.reset_dock_layout {
            self.gui.reset_dock_layout();
            self.gui
                .set_toast("Layout reset to default.", ToastSeverity::Info);
        }
        if actions.quit {
            self.quit_requested = true;
        }
    }
}
