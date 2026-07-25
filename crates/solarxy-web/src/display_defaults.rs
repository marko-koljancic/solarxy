//! The viewport display defaults pushed from the web preferences store:
//! wireframe weight and background, the values `default_pane_settings`
//! seeding starts from. Deliberately NOT wasm-gated (the `attr_viz`
//! convention): the parsing is pure string matching, so native CI runs the
//! tests, including the drift guards that pin the hand-matched names to the
//! enums' real serde output.

use solarxy_core::preferences::{BackgroundMode, BuiltinBg, LineWeight};

/// The app-level display defaults. Applied to every pane at boot and, per
/// field, when the matching preference changes; a scene file's saved
/// per-pane settings still win on load.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayDefaults {
    pub line_weight: LineWeight,
    pub background: BackgroundMode,
}

impl Default for DisplayDefaults {
    fn default() -> Self {
        Self {
            line_weight: LineWeight::default(),
            background: BackgroundMode::GRADIENT,
        }
    }
}

/// Parses a `LineWeight` serde name ("Light" / "Medium" / "Bold"); anything
/// else falls back to the `Light` default rather than erroring, because a
/// stale preference must never break boot.
#[must_use]
pub fn parse_line_weight(s: &str) -> LineWeight {
    match s {
        "Medium" => LineWeight::Medium,
        "Bold" => LineWeight::Bold,
        _ => LineWeight::default(),
    }
}

/// Parses a `BuiltinBg` serde name; anything else falls back to Gradient.
/// Custom backgrounds are a desktop concept; the web preference only offers
/// the builtins.
#[must_use]
pub fn parse_background(s: &str) -> BackgroundMode {
    BackgroundMode::Builtin(match s {
        "White" => BuiltinBg::White,
        "DarkGray" => BuiltinBg::DarkGray,
        "AyuMirage" => BuiltinBg::AyuMirage,
        "Black" => BuiltinBg::Black,
        "HdriSky" => BuiltinBg::HdriSky,
        _ => BuiltinBg::Gradient,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_light_on_gradient() {
        let d = DisplayDefaults::default();
        assert_eq!(d.line_weight, LineWeight::Light);
        assert_eq!(d.background, BackgroundMode::GRADIENT);
    }

    #[test]
    fn line_weight_parsing_tracks_the_serde_names() {
        // The drift guard: every variant's real serde name must round-trip
        // through the hand-written match, so a serde rename cannot silently
        // strand the web preference on the fallback.
        for weight in [LineWeight::Light, LineWeight::Medium, LineWeight::Bold] {
            let name = serde_json::to_value(weight).unwrap();
            let name = name.as_str().expect("LineWeight serializes as a string");
            assert_eq!(parse_line_weight(name), weight, "{name}");
        }
        assert_eq!(parse_line_weight("garbage"), LineWeight::Light);
        assert_eq!(parse_line_weight(""), LineWeight::Light);
    }

    #[test]
    fn background_parsing_tracks_the_serde_names() {
        for bg in [
            BuiltinBg::White,
            BuiltinBg::Gradient,
            BuiltinBg::DarkGray,
            BuiltinBg::AyuMirage,
            BuiltinBg::Black,
            BuiltinBg::HdriSky,
        ] {
            let name = serde_json::to_value(bg).unwrap();
            let name = name.as_str().expect("BuiltinBg serializes as a string");
            assert_eq!(
                parse_background(name),
                BackgroundMode::Builtin(bg),
                "{name}"
            );
        }
        assert_eq!(parse_background("garbage"), BackgroundMode::GRADIENT);
        assert_eq!(parse_background(""), BackgroundMode::GRADIENT);
    }
}
