//! The terminal shell's own state file.
//!
//! # Why this is not in `config.toml`
//!
//! The obvious place is a section beside the desktop shell's preferences, and
//! that is what the specification originally said. It was amended, for a
//! reason worth keeping written down: `Preferences::save` serialises the whole
//! structure and renames it over the file with no read-merge, and the desktop
//! shell saves on quit and on two explicit actions. Two processes sharing that
//! file is a real lost-update path, and a malformed blob would cost a reader
//! their dock layout, their window size and their recent files rather than
//! just their panel arrangement.
//!
//! So the terminal keeps its own file, `solarxy/tui.toml`, written by nothing
//! else.
//!
//! # Two layouts, one auto and one deliberate
//!
//! The same discipline the desktop dock uses: `last_layout` is overwritten on
//! quit so a reader comes back to where they were, and `saved_layout` is
//! written only when they ask, so there is always somewhere to get back to
//! after an afternoon of rearranging. A reset touches neither.

use std::fmt::Write as _;
use std::path::PathBuf;

use super::layout::{Layout, PanelType};

/// The file name, beside the desktop shell's `config.toml`.
pub const FILE_NAME: &str = "tui.toml";

/// What the terminal shell remembers between runs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TuiPrefs {
    /// The theme by name, or `None` to take the default.
    pub theme: Option<String>,
    /// The arrangement at the last quit.
    pub last_layout: Option<String>,
    /// The arrangement the reader explicitly saved.
    pub saved_layout: Option<String>,
}

impl TuiPrefs {
    /// Read the file, or return defaults.
    ///
    /// A missing file is the normal first run. A malformed one is worth a
    /// notice but never worth refusing to start: the arrangement is a
    /// convenience and the report is the point.
    pub fn load() -> (Self, Vec<String>) {
        let Some(path) = path() else {
            return (Self::default(), Vec::new());
        };
        let Ok(source) = std::fs::read_to_string(&path) else {
            return (Self::default(), Vec::new());
        };
        match Self::parse(&source) {
            Ok(prefs) => (prefs, Vec::new()),
            Err(reason) => (
                Self::default(),
                vec![format!(
                    "{} is malformed ({reason}); starting from defaults",
                    path.display()
                )],
            ),
        }
    }

    /// Write the file, creating the directory if it is not there.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml())
    }

    /// The arrangement to open with: what the reader last had, or the default
    /// preset if they have never quit out of one.
    pub fn opening_layout(&self) -> Option<Layout<PanelType>> {
        let text = self.last_layout.as_deref()?;
        Layout::decode(text).ok()
    }

    pub fn to_toml(&self) -> String {
        let mut out = String::from(
            "# Written by solarxy-cli. Safe to edit by hand.\n\
             #\n\
             # A layout reads as splits and panel names: V is a vertical split,\n\
             # whose children sit side by side, H stacks them, and the number is\n\
             # the first child's share.\n\n[tui]\n",
        );
        for (key, value) in [
            ("theme", self.theme.as_deref()),
            ("last_layout", self.last_layout.as_deref()),
            ("saved_layout", self.saved_layout.as_deref()),
        ] {
            if let Some(value) = value {
                let _ = writeln!(out, "{key} = \"{}\"", value.replace('"', "\\\""));
            }
        }
        out
    }

    /// Read by hand, the same way themes are, so a malformed value names its
    /// own key rather than failing the whole document anonymously.
    pub fn parse(source: &str) -> Result<Self, String> {
        let table: toml::Table = source.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let Some(section) = table.get("tui") else {
            return Ok(Self::default());
        };
        let section = section
            .as_table()
            .ok_or_else(|| "tui is not a table".to_owned())?;

        let string = |key: &str| -> Result<Option<String>, String> {
            match section.get(key) {
                None => Ok(None),
                Some(value) => value
                    .as_str()
                    .map(|s| Some(s.to_owned()))
                    .ok_or_else(|| format!("tui.{key} is not a string")),
            }
        };

        let prefs = Self {
            theme: string("theme")?,
            last_layout: string("last_layout")?,
            saved_layout: string("saved_layout")?,
        };

        // A layout that does not parse is refused here rather than at the
        // point of use, so the reader is told once and starts clean instead of
        // opening onto an arrangement that silently lost a panel.
        for (key, value) in [
            ("last_layout", &prefs.last_layout),
            ("saved_layout", &prefs.saved_layout),
        ] {
            if let Some(text) = value {
                Layout::<PanelType>::decode(text).map_err(|e| format!("tui.{key}: {e}"))?;
            }
        }
        Ok(prefs)
    }
}

/// Where the file lives on this platform.
pub fn path() -> Option<PathBuf> {
    solarxy_core::preferences::config_dir().map(|dir| dir.join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::layout::Preset;

    #[test]
    fn a_round_trip_through_the_file_format_is_exact() {
        let prefs = TuiPrefs {
            theme: Some("solarxy-cool".to_owned()),
            last_layout: Some(Preset::Survey.encoded().to_owned()),
            saved_layout: Some(Preset::Validation.encoded().to_owned()),
        };
        let read = TuiPrefs::parse(&prefs.to_toml()).expect("parses");
        assert_eq!(read, prefs);
    }

    #[test]
    fn an_empty_file_is_defaults_rather_than_an_error() {
        assert_eq!(TuiPrefs::parse("").expect("parses"), TuiPrefs::default());
        assert_eq!(
            TuiPrefs::parse("[tui]\n").expect("parses"),
            TuiPrefs::default()
        );
    }

    #[test]
    fn a_partial_file_keeps_what_it_has() {
        let prefs = TuiPrefs::parse("[tui]\ntheme = \"solarxy-paper\"\n").expect("parses");
        assert_eq!(prefs.theme.as_deref(), Some("solarxy-paper"));
        assert!(prefs.last_layout.is_none());
    }

    #[test]
    fn the_opening_layout_is_what_was_last_saved() {
        let prefs = TuiPrefs {
            last_layout: Some(Preset::Meshes.encoded().to_owned()),
            ..TuiPrefs::default()
        };
        let layout = prefs.opening_layout().expect("decodes");
        assert_eq!(layout.encode(), Preset::Meshes.encoded());
        assert!(TuiPrefs::default().opening_layout().is_none());
    }

    /// A layout that does not parse is caught when the file is read, so the
    /// reader is told once rather than opening onto a silently reduced
    /// arrangement.
    #[test]
    fn a_malformed_layout_names_its_key() {
        let error = TuiPrefs::parse("[tui]\nlast_layout = \"V0.5(meshes,nonesuch)\"\n")
            .expect_err("should be refused");
        assert!(error.contains("last_layout"), "{error}");
        assert!(error.contains("nonesuch"), "{error}");
    }

    #[test]
    fn a_non_string_value_names_its_key() {
        let error = TuiPrefs::parse("[tui]\ntheme = 3\n").expect_err("should be refused");
        assert!(error.contains("tui.theme"), "{error}");
    }

    /// The written file is meant to be opened, so it carries its own legend.
    #[test]
    fn the_written_file_explains_itself() {
        let prefs = TuiPrefs {
            theme: Some("solarxy-amber".to_owned()),
            last_layout: Some(Preset::Survey.encoded().to_owned()),
            saved_layout: None,
        };
        let text = prefs.to_toml();
        assert!(text.contains("[tui]"), "{text}");
        assert!(text.contains("vertical split"), "{text}");
        assert!(!text.contains("saved_layout"), "an absent key was written");
        assert!(TuiPrefs::parse(&text).is_ok(), "the legend broke the parse");
    }
}
