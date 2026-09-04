//! What the terminal can actually do, and how to degrade when it cannot.
//!
//! Two independent axes. Colour resolves to one of four tiers and glyphs to
//! one of two, and a panel is drawn the same way at every combination: the
//! detail changes, the meaning does not.
//!
//! # Why the 16-colour tier is the interesting one
//!
//! Tier 1 is a bit-for-bit description of what the analyze shell painted
//! before it had a capability model at all: no background, [`Color::Reset`]
//! ink, a named grey for chrome, and the four semantic hues taken from the
//! shared palette. That is what makes every richer tier safe to add. The
//! terminal that produced the original regression, where the desktop
//! theme's near-black ink was painted into a dark terminal and vanished,
//! still gets exactly the behaviour that fixed it.
//!
//! # Why the override is load-bearing
//!
//! `SOLARXY_COLOR` is not a convenience. It is the escape hatch when a
//! theme misbehaves on a terminal nobody tested, and it makes rendering
//! reproducible. An unrecognised value therefore falls through to detection
//! with a warning rather than failing: an escape hatch that can itself stop
//! the tool is not an escape hatch.

use ratatui::style::Color;
use ratatui::symbols::border;

/// Selects a colour tier explicitly, ahead of every other signal.
pub const COLOR_ENV_VAR: &str = "SOLARXY_COLOR";

/// Forces the ASCII glyph tier (`1`) or the Unicode one (`0`).
pub const ASCII_ENV_VAR: &str = "SOLARXY_ASCII";

/// How much colour the terminal is believed to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorTier {
    /// No colour at all. Structure comes from border weight, bold, glyphs
    /// and layout, and every severity carries its own word.
    Mono,
    /// The shipped behaviour: terminal ink, named greys for chrome, the
    /// palette's semantic hues, and no painted ground.
    Ansi16,
    /// The theme quantised onto the 256-colour cube. Grounds survive and
    /// hues shift slightly; nothing is lost structurally.
    Ansi256,
    /// The theme as authored.
    TrueColor,
}

/// Which glyph repertoire may be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphTier {
    /// Box drawing, braille, and the full glyph set.
    Unicode,
    /// Every glyph has an ASCII stand-in. Detail is lost, meaning is not.
    Ascii,
}

/// The resolved capabilities of the terminal this run is drawing into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub color: ColorTier,
    pub glyphs: GlyphTier,
}

impl Default for Capabilities {
    /// The floor on both axes, which is always safe to draw.
    fn default() -> Self {
        Self {
            color: ColorTier::Ansi16,
            glyphs: GlyphTier::Unicode,
        }
    }
}

impl Capabilities {
    /// Resolve from the real environment.
    pub fn detect() -> Self {
        Self::from_env(|key| std::env::var(key).ok())
    }

    /// Resolve from an arbitrary environment.
    ///
    /// Detection takes a lookup rather than reading the process environment
    /// so that it is a pure function: `std::env::set_var` is unsafe in this
    /// edition and races across the test harness's threads, which would
    /// make a table of cases untestable.
    pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Self {
        // A dumb terminal constrains both axes, and is the only rung that
        // does. It cannot be assumed to render anything beyond ASCII.
        let dumb = lookup("TERM").as_deref().map(str::trim) == Some("dumb");
        Self {
            color: resolve_color(&lookup, dumb),
            glyphs: resolve_glyphs(&lookup, dumb),
        }
    }

    /// The glyph repertoire for these capabilities.
    pub fn glyphs(self) -> Glyphs {
        Glyphs::for_tier(self.glyphs)
    }
}

/// Colour detection, in the ratified order: the explicit override, the
/// no-colour convention, a dumb terminal, the truecolour hint, the terminal
/// name, then the 16-colour fallback.
fn resolve_color(lookup: &impl Fn(&str) -> Option<String>, dumb: bool) -> ColorTier {
    if let Some(raw) = lookup(COLOR_ENV_VAR) {
        let value = raw.trim().to_ascii_lowercase();
        match value.as_str() {
            "mono" | "0" => return ColorTier::Mono,
            "16" => return ColorTier::Ansi16,
            "256" => return ColorTier::Ansi256,
            "truecolor" | "24bit" => return ColorTier::TrueColor,
            "" => {}
            _ => tracing::warn!(
                "{COLOR_ENV_VAR}={raw:?} is not one of mono, 16, 256 or truecolor; \
                 detecting the tier instead"
            ),
        }
    }

    // The published convention: set and non-empty means no colour.
    if lookup("NO_COLOR").is_some_and(|value| !value.is_empty()) {
        return ColorTier::Mono;
    }
    if dumb {
        return ColorTier::Mono;
    }
    if matches!(
        lookup("COLORTERM").as_deref().map(str::trim),
        Some("truecolor" | "24bit")
    ) {
        return ColorTier::TrueColor;
    }
    if lookup("TERM").is_some_and(|term| term.contains("256color")) {
        return ColorTier::Ansi256;
    }
    ColorTier::Ansi16
}

/// Glyph detection: the explicit override, a dumb terminal, then the first
/// locale variable that is set.
fn resolve_glyphs(lookup: &impl Fn(&str) -> Option<String>, dumb: bool) -> GlyphTier {
    if let Some(raw) = lookup(ASCII_ENV_VAR) {
        match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => return GlyphTier::Ascii,
            "0" | "false" | "no" => return GlyphTier::Unicode,
            "" => {}
            _ => tracing::warn!(
                "{ASCII_ENV_VAR}={raw:?} is not 0 or 1; detecting the glyph tier instead"
            ),
        }
    }
    if dumb {
        return GlyphTier::Ascii;
    }
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Some(value) = lookup(key).filter(|value| !value.is_empty()) {
            let value = value.to_ascii_lowercase();
            return if value.contains("utf-8") || value.contains("utf8") {
                GlyphTier::Unicode
            } else {
                GlyphTier::Ascii
            };
        }
    }
    // No locale variable at all. Windows terminals and bare containers set
    // none and are overwhelmingly Unicode-capable, so the richer tier is the
    // better default; anyone it is wrong for has the override.
    GlyphTier::Unicode
}

impl ColorTier {
    /// Map an authored colour onto what this tier can render.
    ///
    /// Tier 1 passes through rather than degrading, because the theme it is
    /// handed is already the shipped palette one: at that tier and below the
    /// theme file is ignored entirely, which is what keeps tier 1 identical
    /// to the surface that shipped before this model existed.
    pub fn degrade(self, color: Color) -> Color {
        match self {
            ColorTier::Mono => Color::Reset,
            ColorTier::Ansi16 | ColorTier::TrueColor => color,
            ColorTier::Ansi256 => quantize(color),
        }
    }

    /// Whether a painted ground is allowed at this tier.
    ///
    /// Below 256 colours the terminal owns the ground: we cannot know what
    /// it is, so painting over it is how ink becomes invisible.
    pub fn paints_a_ground(self) -> bool {
        matches!(self, ColorTier::Ansi256 | ColorTier::TrueColor)
    }

    /// Whether a theme file is consulted at all at this tier.
    ///
    /// The same boundary as [`Self::paints_a_ground`], seen from the other
    /// side. One question underlies both: does this tier own the colour, or
    /// does the terminal? Where the terminal owns it, an authored ground
    /// cannot be painted and an authored ink cannot be trusted, so the theme
    /// is not read rather than read and then partly ignored.
    ///
    /// They are written separately because they will not always agree. A
    /// future tier that reads a theme for its hues while still leaving the
    /// ground alone would split them, and a caller asking the wrong question
    /// should not silently get the right answer today and the wrong one then.
    pub fn reads_a_theme(self) -> bool {
        matches!(self, ColorTier::Ansi256 | ColorTier::TrueColor)
    }
}

/// Nearest xterm-256 entry to an RGB colour.
///
/// Indices 0 to 15 are deliberately never chosen. Those are precisely the
/// slots a user's own terminal theme redefines, so selecting one would hand
/// a semantic hue to somebody else's colour scheme, which is the failure
/// this whole module exists to avoid.
fn quantize(color: Color) -> Color {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    let Color::Rgb(r, g, b) = color else {
        return color;
    };

    let nearest = |channel: u8| -> usize {
        let mut best = 0;
        for (index, &level) in LEVELS.iter().enumerate() {
            if channel.abs_diff(level) < channel.abs_diff(LEVELS[best]) {
                best = index;
            }
        }
        best
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = 16 + 36 * ri + 6 * gi + bi;
    let cube_error = squared_error((r, g, b), (LEVELS[ri], LEVELS[gi], LEVELS[bi]));

    // The 24-step grey ramp at indices 232 to 255, values 8 + 10n. A near
    // neutral lands closer here than anywhere in the cube.
    let average = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
    let step = (average.saturating_sub(8) + 5) / 10;
    let step = u8::try_from(step.min(23)).unwrap_or(23);
    let grey = 8 + step * 10;
    let grey_error = squared_error((r, g, b), (grey, grey, grey));

    if grey_error < cube_error {
        Color::Indexed(232 + step)
    } else {
        Color::Indexed(u8::try_from(cube).unwrap_or(231))
    }
}

fn squared_error(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let channel = |x: u8, y: u8| {
        let difference = u32::from(x.abs_diff(y));
        difference * difference
    };
    channel(a.0, b.0) + channel(a.1, b.1) + channel(a.2, b.2)
}

/// Every glyph the shell draws, in the repertoire the terminal supports.
///
/// The rule for every entry: lose detail, keep meaning. A tick becoming a
/// letter still says the same thing, and it still occupies one cell, which
/// is what keeps a column of them aligned.
///
/// Deliberately not `Copy`: two border sets make this several hundred
/// bytes, so it is passed by reference and held once per shell.
#[derive(Debug, Clone)]
pub struct Glyphs {
    /// The tier this repertoire was selected for.
    pub tier: GlyphTier,
    /// The mark beside the product name.
    pub sun: &'static str,
    /// An attribute is complete.
    pub check: &'static str,
    /// An attribute is absent.
    pub cross: &'static str,
    /// An attribute is partial, or a count disagrees.
    pub warn: &'static str,
    /// The cursor in a text entry.
    pub caret: &'static str,
    pub scroll_up: &'static str,
    pub scroll_down: &'static str,
    /// Separates inline items on one row.
    pub divider: &'static str,
    /// An unfocused panel: rounded single.
    pub border: border::Set,
    /// A focused panel, and every overlay: double. Focus is carried by
    /// border weight before colour, so it survives monochrome.
    pub border_focused: border::Set,
    /// How a spatial plot is drawn at this tier.
    pub plot: PlotStyle,
}

/// How the rasteriser encodes a dot grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlotStyle {
    /// Braille, two dots across and four down per cell.
    Braille,
    /// One cell per sample, density carried by [`Glyphs::ASCII_DENSITY`].
    Ascii,
}

/// Corners, sides and rails in ASCII, matching the design's fallback frame.
const ASCII_BORDER: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// The ASCII frame doubled up, so focus still reads as heavier without
/// leaving the repertoire.
const ASCII_BORDER_FOCUSED: border::Set = border::Set {
    top_left: "#",
    top_right: "#",
    bottom_left: "#",
    bottom_right: "#",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "=",
    horizontal_bottom: "=",
};

impl Glyphs {
    /// Density ramp for a plot at the ASCII tier, lightest first.
    pub const ASCII_DENSITY: [&'static str; 5] = [" ", ".", ":", "*", "#"];

    /// The same five rungs in block elements.
    ///
    /// Five and not more: the shade characters are the only ones that read as
    /// a continuous ramp, `\u{2588}` is already the meter's filled cell, and a
    /// longer ramp built out of anything else would be a set of marks rather
    /// than a gradient.
    pub const UNICODE_DENSITY: [&'static str; 5] =
        [" ", "\u{2591}", "\u{2592}", "\u{2593}", "\u{2588}"];

    /// The shading ramp at this tier, lightest first.
    ///
    /// One accessor rather than two constants at every call site, because a
    /// surface drawing a picture or a fill level should not be the thing that
    /// remembers which tier it is on.
    #[must_use]
    pub fn density(&self) -> &'static [&'static str; 5] {
        match self.tier {
            GlyphTier::Unicode => &Self::UNICODE_DENSITY,
            GlyphTier::Ascii => &Self::ASCII_DENSITY,
        }
    }

    /// The rung a fraction of one lands on, clamped.
    #[must_use]
    pub fn shade(&self, fraction: f64) -> &'static str {
        let ramp = self.density();
        let step = (fraction.clamp(0.0, 1.0) * (ramp.len() - 1) as f64).round() as usize;
        ramp[step.min(ramp.len() - 1)]
    }

    pub fn for_tier(tier: GlyphTier) -> Self {
        match tier {
            GlyphTier::Unicode => Self {
                tier,
                sun: "\u{2600}",
                check: "\u{2713}",
                cross: "\u{2717}",
                warn: "\u{26a0}",
                caret: "\u{2588}",
                scroll_up: "\u{2191}",
                scroll_down: "\u{2193}",
                divider: "\u{2502}",
                border: border::ROUNDED,
                border_focused: border::DOUBLE,
                plot: PlotStyle::Braille,
            },
            GlyphTier::Ascii => Self {
                tier,
                sun: "*",
                check: "v",
                cross: "X",
                warn: "!",
                caret: "_",
                scroll_up: "^",
                scroll_down: "v",
                divider: "|",
                border: ASCII_BORDER,
                border_focused: ASCII_BORDER_FOCUSED,
                plot: PlotStyle::Ascii,
            },
        }
    }

    /// A panel's jump address as it appears in the top border.
    ///
    /// The two tiers are deliberately different widths: a superscript is one
    /// cell and the bracketed form is three, so a caller measuring a border
    /// run has to ask rather than assume.
    pub fn address(&self, number: u8) -> String {
        const SUPERSCRIPTS: [&str; 9] = [
            "\u{b9}", "\u{b2}", "\u{b3}", "\u{2074}", "\u{2075}", "\u{2076}", "\u{2077}",
            "\u{2078}", "\u{2079}",
        ];
        let index = usize::from(number).saturating_sub(1);
        match SUPERSCRIPTS.get(index) {
            Some(mark) if self.tier == GlyphTier::Unicode => (*mark).to_owned(),
            _ => format!("[{number}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a lookup over a fixed table, so a case says exactly which
    /// variables were set and nothing else leaks in from the real
    /// environment.
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| {
            owned
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, value)| value.clone())
        }
    }

    fn color_of(pairs: &[(&str, &str)]) -> ColorTier {
        Capabilities::from_env(env(pairs)).color
    }

    fn glyphs_of(pairs: &[(&str, &str)]) -> GlyphTier {
        Capabilities::from_env(env(pairs)).glyphs
    }

    /// Each rung of the ladder, and each one beating the rung below it.
    #[test]
    fn the_detection_ladder_is_ordered() {
        assert_eq!(color_of(&[]), ColorTier::Ansi16, "the fallback");
        assert_eq!(color_of(&[("TERM", "xterm-256color")]), ColorTier::Ansi256);
        assert_eq!(
            color_of(&[("COLORTERM", "truecolor"), ("TERM", "xterm-256color")]),
            ColorTier::TrueColor,
            "the truecolour hint beats the terminal name"
        );
        assert_eq!(
            color_of(&[("TERM", "dumb"), ("COLORTERM", "truecolor")]),
            ColorTier::Mono,
            "a dumb terminal beats the truecolour hint"
        );
        assert_eq!(
            color_of(&[("NO_COLOR", "1"), ("TERM", "xterm-256color")]),
            ColorTier::Mono,
            "the no-colour convention beats the terminal name"
        );
        assert_eq!(
            color_of(&[("SOLARXY_COLOR", "truecolor"), ("NO_COLOR", "1")]),
            ColorTier::TrueColor,
            "the explicit override wins outright"
        );
    }

    #[test]
    fn every_override_spelling_resolves() {
        for (value, expected) in [
            ("mono", ColorTier::Mono),
            ("0", ColorTier::Mono),
            ("16", ColorTier::Ansi16),
            ("256", ColorTier::Ansi256),
            ("truecolor", ColorTier::TrueColor),
            ("24bit", ColorTier::TrueColor),
            ("TrueColor", ColorTier::TrueColor),
            ("  256  ", ColorTier::Ansi256),
        ] {
            assert_eq!(color_of(&[("SOLARXY_COLOR", value)]), expected, "{value}");
        }
    }

    /// An escape hatch that can itself stop the tool is not an escape hatch,
    /// so a typo falls through to detection rather than failing.
    #[test]
    fn an_unrecognised_override_falls_through() {
        assert_eq!(
            color_of(&[("SOLARXY_COLOR", "purple"), ("TERM", "xterm-256color")]),
            ColorTier::Ansi256
        );
    }

    /// The convention is presence and non-emptiness; an empty value is not a
    /// request for monochrome.
    #[test]
    fn an_empty_no_color_is_not_a_request() {
        assert_eq!(color_of(&[("NO_COLOR", "")]), ColorTier::Ansi16);
        assert_eq!(color_of(&[("NO_COLOR", "0")]), ColorTier::Mono);
    }

    #[test]
    fn the_locale_selects_the_glyph_tier() {
        assert_eq!(glyphs_of(&[("LANG", "en_US.UTF-8")]), GlyphTier::Unicode);
        assert_eq!(glyphs_of(&[("LANG", "C")]), GlyphTier::Ascii);
        assert_eq!(glyphs_of(&[("LC_ALL", "en_GB.utf8")]), GlyphTier::Unicode);
        assert_eq!(
            glyphs_of(&[("LC_ALL", "POSIX"), ("LANG", "en_US.UTF-8")]),
            GlyphTier::Ascii,
            "the first variable that is set decides"
        );
    }

    /// Windows terminals and bare containers set no locale variable and are
    /// overwhelmingly Unicode-capable, so nothing set means the richer tier.
    #[test]
    fn no_locale_at_all_keeps_unicode() {
        assert_eq!(glyphs_of(&[]), GlyphTier::Unicode);
    }

    #[test]
    fn the_ascii_override_wins_both_ways() {
        assert_eq!(
            glyphs_of(&[("SOLARXY_ASCII", "1"), ("LANG", "en_US.UTF-8")]),
            GlyphTier::Ascii
        );
        assert_eq!(
            glyphs_of(&[("SOLARXY_ASCII", "0"), ("LANG", "C")]),
            GlyphTier::Unicode
        );
    }

    /// The two axes are independent everywhere except a dumb terminal, which
    /// constrains both: it cannot be assumed to render anything beyond
    /// ASCII either.
    #[test]
    fn the_axes_are_independent_except_for_a_dumb_terminal() {
        let caps = Capabilities::from_env(env(&[
            ("SOLARXY_COLOR", "truecolor"),
            ("SOLARXY_ASCII", "1"),
        ]));
        assert_eq!(caps.color, ColorTier::TrueColor);
        assert_eq!(caps.glyphs, GlyphTier::Ascii);

        let dumb = Capabilities::from_env(env(&[("TERM", "dumb")]));
        assert_eq!(dumb.color, ColorTier::Mono);
        assert_eq!(dumb.glyphs, GlyphTier::Ascii);
    }

    #[test]
    fn monochrome_erases_every_colour_and_truecolor_erases_none() {
        let amber = Color::Rgb(230, 180, 80);
        assert_eq!(ColorTier::Mono.degrade(amber), Color::Reset);
        assert_eq!(ColorTier::Mono.degrade(Color::DarkGray), Color::Reset);
        assert_eq!(ColorTier::TrueColor.degrade(amber), amber);
        assert_eq!(ColorTier::Ansi16.degrade(amber), amber);
    }

    /// Indices 0 to 15 are the slots a user's terminal theme redefines, so
    /// quantising into them would hand a semantic hue to another scheme.
    #[test]
    fn quantising_never_lands_on_a_user_redefinable_slot() {
        for (r, g, b) in [
            (230, 180, 80),
            (85, 221, 153),
            (229, 72, 77),
            (0, 0, 0),
            (255, 255, 255),
            (17, 17, 14),
        ] {
            let quantised = ColorTier::Ansi256.degrade(Color::Rgb(r, g, b));
            let Color::Indexed(index) = quantised else {
                panic!("{r},{g},{b} did not quantise to an indexed colour");
            };
            assert!(index >= 16, "{r},{g},{b} landed on slot {index}");
        }
    }

    #[test]
    fn quantising_keeps_the_semantic_hues_apart() {
        let hues = [
            Color::Rgb(230, 180, 80),
            Color::Rgb(85, 221, 153),
            Color::Rgb(229, 72, 77),
        ];
        let mut quantised: Vec<Color> = hues
            .iter()
            .map(|&hue| ColorTier::Ansi256.degrade(hue))
            .collect();
        quantised.dedup();
        assert_eq!(quantised.len(), hues.len(), "two hues collapsed into one");
    }

    /// A named slot has no RGB to quantise and must survive untouched, or
    /// the adaptive greys stop adapting.
    #[test]
    fn quantising_leaves_named_slots_alone() {
        assert_eq!(ColorTier::Ansi256.degrade(Color::DarkGray), Color::DarkGray);
        assert_eq!(ColorTier::Ansi256.degrade(Color::Reset), Color::Reset);
    }

    /// The terminal owns the ground below 256 colours: we cannot know what
    /// it is, and painting over it is how ink becomes invisible.
    #[test]
    fn only_the_richer_tiers_paint_a_ground() {
        assert!(!ColorTier::Mono.paints_a_ground());
        assert!(!ColorTier::Ansi16.paints_a_ground());
        assert!(ColorTier::Ansi256.paints_a_ground());
        assert!(ColorTier::TrueColor.paints_a_ground());
    }

    /// Lose detail, keep meaning: every fallback is one cell of plain
    /// ASCII, so a column of them still lines up.
    #[test]
    fn every_ascii_glyph_is_single_cell_ascii() {
        let glyphs = Glyphs::for_tier(GlyphTier::Ascii);
        let singles = [
            glyphs.sun,
            glyphs.check,
            glyphs.cross,
            glyphs.warn,
            glyphs.caret,
            glyphs.scroll_up,
            glyphs.scroll_down,
            glyphs.divider,
        ];
        for glyph in singles {
            assert!(glyph.is_ascii(), "{glyph:?} is not ASCII");
            assert_eq!(glyph.chars().count(), 1, "{glyph:?} is not one cell");
        }
        let frame = glyphs.border;
        for part in [
            frame.top_left,
            frame.top_right,
            frame.bottom_left,
            frame.bottom_right,
            frame.vertical_left,
            frame.vertical_right,
            frame.horizontal_top,
            frame.horizontal_bottom,
        ] {
            assert!(part.is_ascii(), "{part:?} is not ASCII");
        }
        assert!(Glyphs::ASCII_DENSITY.iter().all(|d| d.is_ascii()));
    }

    /// Severity must never be carried by colour alone, so the three marks
    /// stay distinguishable at both tiers.
    #[test]
    fn the_severity_marks_are_distinct_at_both_tiers() {
        for tier in [GlyphTier::Unicode, GlyphTier::Ascii] {
            let glyphs = Glyphs::for_tier(tier);
            assert_ne!(glyphs.check, glyphs.cross);
            assert_ne!(glyphs.cross, glyphs.warn);
            assert_ne!(glyphs.check, glyphs.warn);
        }
    }

    /// Focus is border weight before colour, so the two frames must differ
    /// even where colour cannot.
    #[test]
    fn the_focused_frame_differs_from_the_unfocused_one() {
        for tier in [GlyphTier::Unicode, GlyphTier::Ascii] {
            let glyphs = Glyphs::for_tier(tier);
            assert_ne!(
                glyphs.border.top_left, glyphs.border_focused.top_left,
                "focus is invisible at {tier:?} in a monochrome terminal"
            );
        }
    }

    #[test]
    fn addresses_render_per_tier() {
        let unicode = Glyphs::for_tier(GlyphTier::Unicode);
        assert_eq!(unicode.address(1), "\u{b9}");
        assert_eq!(unicode.address(9), "\u{2079}");
        let ascii = Glyphs::for_tier(GlyphTier::Ascii);
        assert_eq!(ascii.address(1), "[1]");
        assert_eq!(
            unicode.address(10),
            "[10]",
            "past the superscripts the bracketed form is the only option"
        );
    }
}
