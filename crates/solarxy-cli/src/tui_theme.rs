//! The ratatui adapter over the shared interface palette.
//!
//! Third and last consumer of `solarxy_core::theme::Palette`, alongside the
//! egui GUI and (through generated CSS) the web frontend. Before 0.7.1 the
//! analyze TUI had no theme system at all: forty-odd `Color::Yellow` and
//! `Color::Cyan` literals sat inline in `tui_analysis.rs`, so the one
//! surface a CLI user actually looks at shared nothing with the product's
//! identity.
//!
//! # The terminal owns the ground; we only tint
//!
//! The obvious reading of "align the TUI with the app theme" is to resolve
//! `UiPrefs::theme` into a palette and paint everything from it. That is
//! wrong, and it was tried: `UiPrefs::theme` describes the GUI's **window
//! background**, which says nothing about the user's **terminal**
//! background. Selecting Light in the GUI then painted the light palette's
//! near-black ink (`#11110e`) into a dark terminal, where it vanished.
//!
//! So the split here is deliberate:
//!
//! - **Ink is the terminal's.** Body text, headings and labels use
//!   [`Color::Reset`], i.e. whatever foreground the user has configured.
//!   Structure comes from bold and layout, not from a colour we picked.
//!   This is unreadable in exactly zero terminals, by construction.
//! - **Chrome is adaptive.** Borders and de-emphasised text use the named
//!   ANSI greys, which every terminal maps into its own scheme.
//! - **Semantic hues are ours**, and come from the shared palette: the
//!   accent, and the success/warning/error trio. These are what "aligned
//!   with the app" actually means — the same amber, the same red.
//!
//! The palette's **dark** hues are used regardless of the GUI's theme. They
//! are the mid-tone set (amber `#e6b450`, red `#e5484d`, green `#55dd99`),
//! chosen to carry on both a light and a dark ground; the light palette's
//! hues are darkened for cream paper and go muddy on a dark terminal.
//!
//! Terminals without truecolor degrade RGB to their nearest palette entry,
//! which is the standard ratatui fallback and remains legible.

use ratatui::style::Color;
use solarxy_core::theme::{Palette, Rgb};

use super::tui::caps::{Capabilities, ColorTier};

const fn color(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// The TUI's slice of the palette, in the vocabulary this shell draws with.
///
/// The analyze TUI's visual language is structural-vs-emphasis plus the
/// severity trio. Mapping it onto the shared roles is what this type is; it
/// authors no colors of its own.
#[derive(Debug, Clone, Copy)]
pub struct TuiTheme {
    /// Brand emphasis: the sun glyph, the active tab, measured values.
    pub accent: Color,
    /// Section headings. Terminal ink; the weight carries the hierarchy.
    pub heading: Color,
    /// Primary body text. Terminal ink.
    pub text: Color,
    /// Field labels beside a value.
    pub label: Color,
    /// Inactive tabs and de-emphasised hints.
    pub muted: Color,
    /// Block borders.
    pub border: Color,
    pub success: Color,
    /// Warnings. Rides the attention hue, as it does on every other shell.
    pub warning: Color,
    pub error: Color,
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::dark())
    }
}

impl TuiTheme {
    /// The analyze TUI's palette.
    ///
    /// Takes no theme choice, deliberately: see the module docs. The GUI's
    /// light/dark preference describes a window background and has no
    /// bearing on the terminal the CLI is printing into.
    pub fn resolve() -> Self {
        Self::default()
    }

    /// The palette mapped onto what the terminal can render.
    ///
    /// At the 16-colour tier this is exactly [`Self::resolve`], which is
    /// what makes that tier a description of the shipped surface rather
    /// than a reconstruction of it. Above it the same values are quantised;
    /// below it they are erased.
    pub fn for_capabilities(caps: Capabilities) -> Self {
        Self::resolve().degraded(caps.color)
    }

    /// Map every slot through a tier's degradation rule.
    #[must_use]
    pub fn degraded(self, tier: ColorTier) -> Self {
        Self {
            accent: tier.degrade(self.accent),
            heading: tier.degrade(self.heading),
            text: tier.degrade(self.text),
            label: tier.degrade(self.label),
            muted: tier.degrade(self.muted),
            border: tier.degrade(self.border),
            success: tier.degrade(self.success),
            warning: tier.degrade(self.warning),
            error: tier.degrade(self.error),
        }
    }

    /// Adapt a resolved theme onto this shell's nine-slot vocabulary.
    ///
    /// Hues only. This shell has no concept of a panel ground and paints
    /// none, so `ground`, `panel_ground` and `selection` are dropped rather
    /// than approximated: the tiled panels that own those slots arrive with
    /// the workspace, and painting a ground here would fight the terminal for
    /// no gain.
    ///
    /// `label` takes `ink_dim` rather than the success hue. The two were the
    /// same colour before themes existed, which made every field label green
    /// and left the label slot reachable only by empty-state text. A theme
    /// that names the two separately is what makes the distinction available.
    pub fn from_theme(theme: &super::tui::theme::Theme) -> Self {
        let s = &theme.slots;
        Self {
            accent: s.accent,
            heading: s.ink,
            text: s.ink,
            label: s.ink_dim,
            muted: s.ink_dim,
            border: s.border,
            success: s.success,
            warning: s.warning,
            error: s.error,
        }
    }

    /// Map the shared palette onto ratatui, keeping the ink terminal-native.
    ///
    /// Pass `Palette::dark()`: its hues are the mid-tone set that carries on
    /// either ground. This takes a palette rather than reading one so the
    /// tests can prove the light palette's ink never reaches the screen.
    pub fn from_palette(palette: &Palette) -> Self {
        let r = &palette.roles;
        Self {
            accent: color(r.accent.rgb),
            // Ink stays the terminal's. Headings were cyan and are now the
            // terminal's own foreground, bold: legible in every scheme,
            // which no colour we choose can promise.
            heading: Color::Reset,
            text: Color::Reset,
            label: color(r.status_success.rgb),
            // The named greys adapt to the user's scheme; an RGB grey would
            // not.
            muted: Color::DarkGray,
            border: Color::DarkGray,
            success: color(r.status_success.rgb),
            warning: color(r.state_attention.rgb),
            error: color(r.status_error.rgb),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression this module was rewritten for: selecting the GUI's
    /// light theme painted `#11110e` ink into a dark terminal, which is
    /// invisible. Ink must never be a colour we chose — whatever palette is
    /// passed.
    #[test]
    fn ink_is_always_the_terminals_own() {
        for palette in [Palette::dark(), Palette::light()] {
            let t = TuiTheme::from_palette(&palette);
            assert_eq!(t.text, Color::Reset, "body text must be terminal ink");
            assert_eq!(t.heading, Color::Reset, "headings must be terminal ink");
        }
    }

    /// A light palette's near-black ink must not reach the screen through
    /// any field. This is the specific value that vanished.
    #[test]
    fn the_light_palettes_ink_never_reaches_a_terminal() {
        let light = Palette::light();
        let ink = color(light.roles.ink_primary.rgb);
        let strong = color(light.roles.ink_strong.rgb);
        let t = TuiTheme::from_palette(&light);
        for c in [t.accent, t.heading, t.text, t.label, t.muted, t.border] {
            assert_ne!(c, ink, "light ink reached the terminal");
            assert_ne!(c, strong, "light strong-ink reached the terminal");
        }
    }

    /// Chrome must ride the named ANSI slots, which the terminal remaps into
    /// its own scheme; an RGB grey would not adapt.
    #[test]
    fn chrome_adapts_to_the_terminal() {
        let t = TuiTheme::resolve();
        assert_eq!(t.muted, Color::DarkGray);
        assert_eq!(t.border, Color::DarkGray);
    }

    /// The semantic hues are the part that IS shared with the app, so they
    /// must genuinely come from the palette rather than a named slot.
    #[test]
    fn semantic_hues_come_from_the_shared_palette() {
        let t = TuiTheme::resolve();
        let dark = Palette::dark();
        assert_eq!(t.accent, color(dark.roles.accent.rgb));
        assert_eq!(t.error, color(dark.roles.status_error.rgb));
        assert_eq!(t.warning, color(dark.roles.state_attention.rgb));
        assert_eq!(t.success, color(dark.roles.status_success.rgb));
        for c in [t.accent, t.success, t.warning, t.error] {
            assert!(matches!(c, Color::Rgb(..)));
        }
    }

    /// Mid-tone: readable on a light ground AND a dark one. The light
    /// palette's hues are darkened for cream and go muddy on a dark
    /// terminal, which is why the dark set is used regardless of the GUI.
    #[test]
    fn semantic_hues_are_mid_tone() {
        let t = TuiTheme::resolve();
        for (name, c) in [
            ("accent", t.accent),
            ("success", t.success),
            ("warning", t.warning),
            ("error", t.error),
        ] {
            let Color::Rgb(r, g, b) = c else {
                panic!("rgb")
            };
            let luma = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
            assert!(
                (60.0..=210.0).contains(&luma),
                "{name} luma {luma:.0} is too close to one end to carry on both grounds",
            );
        }
    }

    #[test]
    fn severities_are_distinct() {
        let t = TuiTheme::resolve();
        assert_ne!(t.error, t.warning);
        assert_ne!(t.warning, t.success);
        assert_ne!(t.error, t.success);
    }
}
