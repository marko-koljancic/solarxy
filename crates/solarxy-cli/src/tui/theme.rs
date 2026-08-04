//! Themes as files, and the one place a tier decides whether to read them.
//!
//! # Why this shell is the exception
//!
//! Every other Solarxy surface paints into a window it created, so it knows
//! its own background and `solarxy_core::theme::Palette` can own the whole
//! look. This one paints onto whatever the reader's terminal already was, and
//! a terminal application is expected to be themeable in a way a desktop app
//! is not. So the colours become a file here, and `design/README.md` carries
//! a stated exception rather than a rule anyone can see is violated.
//!
//! The exception is deliberately narrow. `default_theme_matches_the_palette`
//! pins the default's accent and severity slots to the palette's roles, so the
//! shipped identity still comes from the one source and cannot drift from the
//! desktop shell or the web app. Alternates and user themes are a feature
//! layered on top, not a second opinion about what Solarxy looks like.
//!
//! # One resolution point
//!
//! [`Theme::resolve`] is the only way to obtain a [`Theme`], and at the
//! monochrome and 16-colour tiers it ignores the file completely and returns
//! what the shell painted before themes existed. That is structural on
//! purpose. Scattering a tier check across every panel's draw call is how one
//! of them eventually forgets, and the thing it would forget is the 0.7.1
//! regression: a light theme's near-black ink painted into a dark terminal,
//! where it vanished.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::str::FromStr;

use ratatui::style::Color;
use solarxy_core::theme::{Palette, Rgb};

use super::caps::{Capabilities, ColorTier};
use super::contrast::{self, ContrastFailure};

/// The theme used when nothing else resolves, and the one pinned to the
/// palette by test.
pub const DEFAULT_THEME: &str = "solarxy-amber";

/// Subdirectory of the config directory holding user themes.
pub const USER_THEME_DIR: &str = "tui-themes";

const BUNDLED: [(&str, &str); 4] = [
    ("solarxy-amber", include_str!("themes/solarxy-amber.toml")),
    ("solarxy-cool", include_str!("themes/solarxy-cool.toml")),
    ("solarxy-paper", include_str!("themes/solarxy-paper.toml")),
    (
        "solarxy-contrast",
        include_str!("themes/solarxy-contrast.toml"),
    ),
];

/// Every slot a theme file may set.
///
/// The order here is the order the file documents and the order
/// `--list-tui-themes` prints, so a reader comparing the two never has to
/// translate between them.
pub const SLOT_NAMES: [&str; 11] = [
    "ground",
    "panel_ground",
    "ink",
    "ink_dim",
    "border",
    "border_focus",
    "accent",
    "success",
    "warning",
    "error",
    "selection",
];

/// A theme as read from disk: possibly partial, not yet judged.
#[derive(Debug, Clone)]
pub struct ThemeFile {
    pub name: String,
    pub author: Option<String>,
    /// The theme this one layers over. Absent means the default.
    pub base: Option<String>,
    /// Where it came from, for the listing.
    pub origin: Origin,
    colors: BTreeMap<String, Color>,
    chart: Option<Vec<Color>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Bundled,
    User(String),
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bundled => f.write_str("bundled"),
            Self::User(path) => f.write_str(path),
        }
    }
}

/// Something the loader wants the user to know, collected rather than printed.
///
/// The loader runs before the alternate screen is taken and may run again
/// inside the listing, so it hands notices back instead of writing to a
/// terminal it does not own.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// A file could not be parsed, naming the reason.
    Unreadable { name: String, reason: String },
    /// A theme fell below the contrast floor and was not used.
    BelowFloor {
        name: String,
        failure: ContrastFailure,
    },
    /// A `base` chain looped.
    BaseCycle { name: String, chain: String },
    /// A `base` named a theme nobody has.
    UnknownBase { name: String, base: String },
    /// The requested theme is not one we have.
    UnknownTheme {
        requested: String,
        known: Vec<String>,
    },
}

impl std::fmt::Display for Notice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable { name, reason } => {
                write!(f, "theme {name}: {reason}; ignoring it")
            }
            Self::BelowFloor { name, failure } => {
                write!(
                    f,
                    "theme {name} is unreadable: {failure}; using {DEFAULT_THEME}"
                )
            }
            Self::BaseCycle { name, chain } => {
                write!(
                    f,
                    "theme {name} has a base cycle ({chain}); using {DEFAULT_THEME}"
                )
            }
            Self::UnknownBase { name, base } => {
                write!(
                    f,
                    "theme {name} names an unknown base {base}; using {DEFAULT_THEME}"
                )
            }
            Self::UnknownTheme { requested, known } => {
                write!(
                    f,
                    "no theme named {requested}; known themes are {}; using {DEFAULT_THEME}",
                    known.join(", ")
                )
            }
        }
    }
}

/// The full slot set after merging, before any tier has had its say.
#[derive(Debug, Clone, PartialEq)]
pub struct Slots {
    pub ground: Color,
    pub panel_ground: Color,
    pub ink: Color,
    pub ink_dim: Color,
    pub border: Color,
    pub border_focus: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub selection: Color,
    pub chart: Vec<Color>,
}

impl Slots {
    fn get(&self, slot: &str) -> Color {
        match slot {
            "ground" => self.ground,
            "panel_ground" => self.panel_ground,
            "ink" => self.ink,
            "ink_dim" => self.ink_dim,
            "border" => self.border,
            "border_focus" => self.border_focus,
            "accent" => self.accent,
            "success" => self.success,
            "warning" => self.warning,
            "error" => self.error,
            "selection" => self.selection,
            _ => Color::Reset,
        }
    }

    /// The three pairs the floor judges, per the ratified decision: ink and
    /// dim ink against the ground, and ink against a panel's ground.
    ///
    /// Selection is deliberately not among them. It is a transient band under
    /// one row rather than the surface the report is read on, and folding it
    /// in would reject themes over a state the eye is on for a moment.
    pub fn contrast_failure(&self) -> Option<ContrastFailure> {
        [
            ("ink", self.ink, "ground", self.ground),
            ("ink_dim", self.ink_dim, "ground", self.ground),
            ("ink", self.ink, "panel_ground", self.panel_ground),
        ]
        .into_iter()
        .find_map(|(ink_name, ink, ground_name, ground)| {
            contrast::check(ink_name, ink, ground_name, ground)
        })
    }
}

const fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// What the shell painted before themes existed, which is what the two lower
/// tiers still paint.
///
/// Ink is [`Color::Reset`], so it is whatever foreground the reader
/// configured; chrome rides the named greys, which every terminal remaps into
/// its own scheme; and the semantic hues come from the palette. A ground of
/// `Color::Reset` is the terminal's own, which is the same thing as painting
/// none.
fn shipped_slots() -> Slots {
    let r = &Palette::dark().roles;
    Slots {
        ground: Color::Reset,
        panel_ground: Color::Reset,
        ink: Color::Reset,
        ink_dim: Color::DarkGray,
        border: Color::DarkGray,
        border_focus: rgb(r.accent.rgb),
        accent: rgb(r.accent.rgb),
        success: rgb(r.status_success.rgb),
        warning: rgb(r.state_attention.rgb),
        error: rgb(r.status_error.rgb),
        selection: Color::Reset,
        // One hue and one grey. A second authored hue here would be a colour
        // the tier has no way to promise, and the plots carry their own
        // density and glyph distinctions anyway.
        chart: vec![rgb(r.accent.rgb), Color::DarkGray],
    }
}

/// One line of the theme listing: what it is, where it came from, and either
/// the theme itself or the reason it cannot be used.
#[derive(Debug, Clone)]
pub struct ThemeRow {
    pub name: String,
    pub author: Option<String>,
    pub origin: Origin,
    pub outcome: Result<Theme, Notice>,
}

/// A theme resolved against a terminal, ready to draw with.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The theme actually in force, which is not always the one requested.
    pub name: String,
    pub slots: Slots,
}

impl Theme {
    /// The only way to get one.
    ///
    /// At the monochrome and 16-colour tiers the file is not consulted at all,
    /// so no theme can reach a terminal that cannot be trusted to render it.
    pub fn resolve(caps: Capabilities, name: &str, slots: &Slots) -> Self {
        if caps.color.reads_a_theme() {
            Self {
                name: name.to_owned(),
                slots: degrade(slots, caps.color),
            }
        } else {
            Self {
                name: name.to_owned(),
                slots: degrade(&shipped_slots(), caps.color),
            }
        }
    }
}

fn degrade(slots: &Slots, tier: ColorTier) -> Slots {
    Slots {
        ground: tier.degrade(slots.ground),
        panel_ground: tier.degrade(slots.panel_ground),
        ink: tier.degrade(slots.ink),
        ink_dim: tier.degrade(slots.ink_dim),
        border: tier.degrade(slots.border),
        border_focus: tier.degrade(slots.border_focus),
        accent: tier.degrade(slots.accent),
        success: tier.degrade(slots.success),
        warning: tier.degrade(slots.warning),
        error: tier.degrade(slots.error),
        selection: tier.degrade(slots.selection),
        chart: slots.chart.iter().map(|&c| tier.degrade(c)).collect(),
    }
}

/// Every theme this run knows about, bundled first and user themes after.
#[derive(Debug, Clone)]
pub struct ThemeSet {
    files: Vec<ThemeFile>,
    notices: Vec<Notice>,
}

impl ThemeSet {
    /// The compiled-in set only.
    ///
    /// Bundled themes are `include_str!`d rather than resolved from an install
    /// path, so a theme cannot go missing between a package manager and a home
    /// directory.
    pub fn bundled() -> Self {
        let mut files = Vec::new();
        let mut notices = Vec::new();
        for (name, source) in BUNDLED {
            match parse(name, source, Origin::Bundled) {
                Ok(file) => files.push(file),
                Err(reason) => notices.push(Notice::Unreadable {
                    name: name.to_owned(),
                    reason,
                }),
            }
        }
        Self { files, notices }
    }

    /// The bundled set plus whatever the user has dropped in the config
    /// directory.
    pub fn load() -> Self {
        let mut set = Self::bundled();
        let Some(dir) = user_theme_dir() else {
            return set;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // An absent directory is the normal case, not a problem worth
            // telling anyone about.
            return set;
        };
        let mut found: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "toml"))
            .collect();
        found.sort();

        for path in found {
            let display = path.display().to_string();
            let stem = path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            match std::fs::read_to_string(&path) {
                Ok(source) => match parse(&stem, &source, Origin::User(display.clone())) {
                    Ok(file) => {
                        // A user file wins its name outright, so a theme can be
                        // overridden without renaming every reference to it.
                        set.files.retain(|existing| existing.name != file.name);
                        set.files.push(file);
                    }
                    Err(reason) => set.notices.push(Notice::Unreadable {
                        name: display,
                        reason,
                    }),
                },
                Err(error) => set.notices.push(Notice::Unreadable {
                    name: display,
                    reason: error.to_string(),
                }),
            }
        }
        set
    }

    pub fn names(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.name.as_str()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&ThemeFile> {
        self.files.iter().find(|f| f.name == name)
    }

    /// Notices raised while loading, before any theme was asked for.
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// Merge a theme with its base chain into a full slot set.
    ///
    /// A user overriding one slot does not inherit responsibility for the
    /// other eleven, which is the whole reason `base` exists.
    pub fn slots_for(&self, name: &str) -> Result<Slots, Notice> {
        let mut chain = Vec::new();
        let mut cursor = name.to_owned();

        // Walk to the root of the base chain first, so the merge can then run
        // root-first and let each layer overwrite the one beneath it.
        loop {
            if chain.iter().any(|seen: &String| seen == &cursor) {
                chain.push(cursor);
                return Err(Notice::BaseCycle {
                    name: name.to_owned(),
                    chain: chain.join(" -> "),
                });
            }
            let Some(file) = self.get(&cursor) else {
                return Err(Notice::UnknownBase {
                    name: name.to_owned(),
                    base: cursor,
                });
            };
            chain.push(cursor.clone());
            match &file.base {
                Some(base) => cursor.clone_from(base),
                None => break,
            }
        }

        let mut colors: BTreeMap<String, Color> = BTreeMap::new();
        let mut chart: Option<Vec<Color>> = None;
        for layer in chain.iter().rev() {
            let file = self.get(layer).expect("walked above");
            for (slot, value) in &file.colors {
                colors.insert(slot.clone(), *value);
            }
            if let Some(series) = &file.chart {
                chart = Some(series.clone());
            }
        }

        let fallback = shipped_slots();
        let pick = |slot: &str| colors.get(slot).copied().unwrap_or(fallback.get(slot));
        Ok(Slots {
            ground: pick("ground"),
            panel_ground: pick("panel_ground"),
            ink: pick("ink"),
            ink_dim: pick("ink_dim"),
            border: pick("border"),
            border_focus: pick("border_focus"),
            accent: pick("accent"),
            success: pick("success"),
            warning: pick("warning"),
            error: pick("error"),
            selection: pick("selection"),
            chart: chart.unwrap_or(fallback.chart),
        })
    }

    /// Resolve a request into something drawable, saying what went wrong.
    ///
    /// Never fails. Every path that cannot honour the request falls back to
    /// the default and adds a notice, because a colour scheme is not a reason
    /// to refuse to show someone their model.
    pub fn resolve(&self, requested: Option<&str>, caps: Capabilities) -> (Theme, Vec<Notice>) {
        let mut notices = Vec::new();
        let wanted = requested.unwrap_or(DEFAULT_THEME);

        if self.get(wanted).is_none() {
            notices.push(Notice::UnknownTheme {
                requested: wanted.to_owned(),
                known: self.names().iter().map(|s| (*s).to_owned()).collect(),
            });
            return (self.default_theme(caps), notices);
        }

        match self.slots_for(wanted) {
            Ok(slots) => match slots.contrast_failure() {
                Some(failure) => {
                    notices.push(Notice::BelowFloor {
                        name: wanted.to_owned(),
                        failure,
                    });
                    (self.default_theme(caps), notices)
                }
                None => (Theme::resolve(caps, wanted, &slots), notices),
            },
            Err(notice) => {
                notices.push(notice);
                (self.default_theme(caps), notices)
            }
        }
    }

    /// Every theme with its verdict, for `--list-tui-themes`.
    ///
    /// A tool that discovers themes from a directory owes the reader a way to
    /// see what it found, including the ones it refused and why. Returned as
    /// data rather than printed so the decision of what to say is testable
    /// separately from the escape codes that say it.
    pub fn listing(&self, caps: Capabilities) -> Vec<ThemeRow> {
        self.files
            .iter()
            .map(|file| {
                let outcome = match self.slots_for(&file.name) {
                    Err(notice) => Err(notice),
                    Ok(slots) => match slots.contrast_failure() {
                        Some(failure) => Err(Notice::BelowFloor {
                            name: file.name.clone(),
                            failure,
                        }),
                        None => Ok(Theme::resolve(caps, &file.name, &slots)),
                    },
                };
                ThemeRow {
                    name: file.name.clone(),
                    author: file.author.clone(),
                    origin: file.origin.clone(),
                    outcome,
                }
            })
            .collect()
    }

    /// The default, and if even that will not load, the surface the shell
    /// painted before any of this existed.
    fn default_theme(&self, caps: Capabilities) -> Theme {
        self.slots_for(DEFAULT_THEME).map_or_else(
            |_| Theme::resolve(caps, DEFAULT_THEME, &shipped_slots()),
            |slots| Theme::resolve(caps, DEFAULT_THEME, &slots),
        )
    }
}

/// Read a theme file by hand rather than through a derive.
///
/// Hand-reading buys two things worth the twenty lines: a malformed slot names
/// itself instead of failing the whole document, and no serde derive joins a
/// manifest that otherwise has none.
fn parse(fallback_name: &str, source: &str, origin: Origin) -> Result<ThemeFile, String> {
    let table: toml::Table = source.parse().map_err(|e: toml::de::Error| e.to_string())?;

    let name = table
        .get("name")
        .and_then(toml::Value::as_str)
        .unwrap_or(fallback_name)
        .to_owned();
    let author = table
        .get("author")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let base = table
        .get("base")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);

    let mut colors = BTreeMap::new();
    let mut chart = None;
    if let Some(slots) = table.get("slots").and_then(toml::Value::as_table) {
        for (key, value) in slots {
            if key == "chart" {
                let Some(list) = value.as_array() else {
                    return Err("slots.chart must be an array of colours".to_owned());
                };
                let mut series = Vec::with_capacity(list.len());
                for entry in list {
                    series.push(color_of(entry).ok_or_else(|| {
                        format!("slots.chart contains {entry}, which is not a colour")
                    })?);
                }
                chart = Some(series);
                continue;
            }
            if !SLOT_NAMES.contains(&key.as_str()) {
                return Err(format!("slots.{key} is not a slot this version knows"));
            }
            let parsed =
                color_of(value).ok_or_else(|| format!("slots.{key} = {value} is not a colour"))?;
            colors.insert(key.clone(), parsed);
        }
    }

    Ok(ThemeFile {
        name,
        author,
        base,
        origin,
        colors,
        chart,
    })
}

fn color_of(value: &toml::Value) -> Option<Color> {
    Color::from_str(value.as_str()?).ok()
}

/// Where user themes are read from on this platform.
///
/// Not `~/.config` everywhere: `dirs::config_dir()` resolves to
/// `~/Library/Application Support` on macOS and `%APPDATA%` on Windows. The
/// documentation quotes the Linux form as shorthand, which is why the listing
/// prints the real path rather than repeating it.
pub fn user_theme_dir() -> Option<std::path::PathBuf> {
    solarxy_core::preferences::config_dir().map(|d| d.join(USER_THEME_DIR))
}

/// Print every theme with a swatch, at the tier this terminal resolves to.
///
/// A tool that discovers themes from a directory owes the reader a way to see
/// what it found. Rendering at the real tier rather than at the richest one is
/// the point: on a 16-colour terminal every swatch is deliberately identical,
/// because the theme is not read there, and four different rows would promise
/// something untrue.
pub fn print_listing() {
    let caps = Capabilities::detect();
    let set = ThemeSet::load();

    println!("Colour tier {:?}, glyphs {:?}", caps.color, caps.glyphs);
    if !caps.color.reads_a_theme() {
        println!("This terminal does not read themes, so every swatch below is the same.");
        println!("Try SOLARXY_COLOR=truecolor to see them as authored.");
    }
    // Naming the directory is not decoration. `dirs::config_dir()` is
    // `~/Library/Application Support` on macOS and `%APPDATA%` on Windows, so
    // anyone following the Linux path from the documentation drops a file
    // somewhere nothing will ever look and gets no feedback at all.
    match user_theme_dir() {
        Some(dir) => println!("User themes are read from {}", dir.display()),
        None => println!("This platform reports no config directory, so only bundled themes load."),
    }
    for notice in set.notices() {
        println!("  note: {notice}");
    }
    println!();

    for row in set.listing(caps) {
        match row.outcome {
            Err(notice) => println!("  {:<18} refused: {notice}", row.name),
            Ok(theme) => {
                // Painted as a background rather than a block glyph. A
                // block in the ground's own colour is invisible against a
                // terminal of that colour, which is precisely the swatch a
                // reader most needs to see.
                let mut swatch = String::new();
                for slot in SLOT_NAMES {
                    let color = crossterm::style::Color::from(theme.slots.get(slot));
                    let _ = write!(
                        swatch,
                        "{}  {}",
                        crossterm::style::SetBackgroundColor(color),
                        crossterm::style::ResetColor
                    );
                }
                let by = row
                    .author
                    .map_or_else(String::new, |author| format!(" by {author}"));
                println!("  {:<18} {swatch}  {}{by}", row.name, row.origin);
            }
        }
    }
    println!();
    println!("Slots, left to right: {}", SLOT_NAMES.join(", "));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::caps::GlyphTier;

    const TRUECOLOR: Capabilities = Capabilities {
        color: ColorTier::TrueColor,
        glyphs: GlyphTier::Unicode,
    };

    fn set() -> ThemeSet {
        let set = ThemeSet::bundled();
        assert!(set.notices().is_empty(), "{:?}", set.notices());
        set
    }

    /// The narrow but real form of the drift guarantee that survives themes
    /// becoming files. The palette still owns the shipped identity.
    #[test]
    fn default_theme_matches_the_palette() {
        let slots = set().slots_for(DEFAULT_THEME).expect("the default loads");
        let roles = &Palette::dark().roles;
        assert_eq!(slots.accent, rgb(roles.accent.rgb), "accent");
        assert_eq!(slots.success, rgb(roles.status_success.rgb), "success");
        assert_eq!(slots.warning, rgb(roles.state_attention.rgb), "warning");
        assert_eq!(slots.error, rgb(roles.status_error.rgb), "error");
    }

    /// The default's focus border is its accent. A relationship inside one
    /// theme rather than a palette pin, so it is asserted separately from the
    /// four slots the drift guarantee covers.
    #[test]
    fn the_defaults_focus_border_is_its_accent() {
        let slots = set().slots_for(DEFAULT_THEME).expect("the default loads");
        assert_eq!(slots.border_focus, slots.accent);
    }

    #[test]
    fn every_bundled_theme_loads_and_clears_the_floor() {
        let set = set();
        assert_eq!(set.names().len(), 4, "{:?}", set.names());
        for name in set.names() {
            let slots = set
                .slots_for(name)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                slots.contrast_failure(),
                None,
                "{name} ships below the readability floor"
            );
            assert!(slots.chart.len() >= 2, "{name} needs two chart series");
        }
    }

    /// Every slot has to be set outright by every shipped theme. Inheriting
    /// one silently from the fallback is how a theme ends up with a stray dark
    /// slot on a light ground.
    #[test]
    fn every_bundled_theme_sets_every_slot() {
        let set = set();
        for name in set.names() {
            let file = set.get(name).expect("named above");
            for slot in SLOT_NAMES {
                assert!(file.colors.contains_key(slot), "{name} leaves {slot} unset");
            }
            assert!(file.chart.is_some(), "{name} leaves chart unset");
        }
    }

    /// The regression this whole model exists to prevent, asserted against the
    /// mechanism that can actually break it rather than against one mapping
    /// function.
    #[test]
    fn the_lower_tiers_never_read_a_theme() {
        let set = set();
        let paper = set.slots_for("solarxy-paper").expect("loads");
        assert_eq!(
            paper.ink,
            Color::Rgb(0x11, 0x11, 0x0e),
            "the fixture's point"
        );

        for tier in [ColorTier::Mono, ColorTier::Ansi16] {
            let caps = Capabilities {
                color: tier,
                glyphs: GlyphTier::Unicode,
            };
            let theme = Theme::resolve(caps, "solarxy-paper", &paper);
            assert_ne!(
                theme.slots.ink, paper.ink,
                "a light theme's ink at {tier:?}"
            );
            assert_eq!(theme.slots.ground, Color::Reset, "a ground at {tier:?}");
            assert_eq!(theme.slots.panel_ground, Color::Reset, "at {tier:?}");
        }
    }

    #[test]
    fn the_upper_tiers_do_read_a_theme() {
        let set = set();
        let paper = set.slots_for("solarxy-paper").expect("loads");
        let theme = Theme::resolve(TRUECOLOR, "solarxy-paper", &paper);
        assert_eq!(theme.slots.ink, paper.ink);
        assert_eq!(theme.slots.ground, paper.ground);
    }

    #[test]
    fn a_user_theme_overriding_one_slot_inherits_the_rest() {
        let mut set = set();
        let amber = set.slots_for(DEFAULT_THEME).expect("loads");
        set.files.push(
            parse(
                "mine",
                "name = \"mine\"\nbase = \"solarxy-amber\"\n[slots]\naccent = \"#ff00ff\"\n",
                Origin::User("/tmp/mine.toml".to_owned()),
            )
            .expect("parses"),
        );

        let mine = set.slots_for("mine").expect("merges");
        assert_eq!(mine.accent, Color::Rgb(0xff, 0x00, 0xff), "the override");
        assert_eq!(mine.ground, amber.ground, "inherited");
        assert_eq!(mine.ink, amber.ink, "inherited");
        assert_eq!(mine.chart, amber.chart, "inherited");
    }

    /// A theme system able to render the tool unreadable is a defect, so the
    /// refusal names the pair and the default is substituted.
    #[test]
    fn an_unreadable_theme_is_refused_by_name() {
        let mut set = set();
        set.files.push(
            parse(
                "murk",
                "name = \"murk\"\nbase = \"solarxy-amber\"\n[slots]\nink = \"#222222\"\n",
                Origin::User("/tmp/murk.toml".to_owned()),
            )
            .expect("parses"),
        );

        let (theme, notices) = set.resolve(Some("murk"), TRUECOLOR);
        assert_eq!(theme.name, DEFAULT_THEME, "the default was substituted");
        let [Notice::BelowFloor { name, failure }] = &notices[..] else {
            panic!("expected one floor notice, got {notices:?}");
        };
        assert_eq!(name, "murk");
        assert_eq!(failure.ink, "ink");
        assert_eq!(failure.ground, "ground");
        assert!(notices[0].to_string().contains("murk"), "{notices:?}");
    }

    #[test]
    fn a_base_cycle_is_refused_and_names_the_chain() {
        let mut set = set();
        for (name, base) in [("a", "b"), ("b", "a")] {
            set.files.push(
                parse(
                    name,
                    &format!("name = \"{name}\"\nbase = \"{base}\"\n"),
                    Origin::User(format!("/tmp/{name}.toml")),
                )
                .expect("parses"),
            );
        }
        let (theme, notices) = set.resolve(Some("a"), TRUECOLOR);
        assert_eq!(theme.name, DEFAULT_THEME);
        let [Notice::BaseCycle { chain, .. }] = &notices[..] else {
            panic!("expected a cycle notice, got {notices:?}");
        };
        assert!(chain.contains("a -> b -> a"), "{chain}");
    }

    #[test]
    fn an_unknown_base_is_refused_by_name() {
        let mut set = set();
        set.files.push(
            parse(
                "orphan",
                "name = \"orphan\"\nbase = \"nonesuch\"\n",
                Origin::User("/tmp/orphan.toml".to_owned()),
            )
            .expect("parses"),
        );
        let (theme, notices) = set.resolve(Some("orphan"), TRUECOLOR);
        assert_eq!(theme.name, DEFAULT_THEME);
        assert!(
            matches!(&notices[..], [Notice::UnknownBase { base, .. }] if base == "nonesuch"),
            "{notices:?}"
        );
    }

    /// An escape hatch that can itself stop the tool is not an escape hatch,
    /// so an unknown name lists what there is and carries on.
    #[test]
    fn an_unknown_theme_name_lists_what_there_is() {
        let (theme, notices) = set().resolve(Some("solarxy-purple"), TRUECOLOR);
        assert_eq!(theme.name, DEFAULT_THEME);
        let rendered = notices[0].to_string();
        assert!(rendered.contains("solarxy-purple"), "{rendered}");
        assert!(rendered.contains(DEFAULT_THEME), "{rendered}");
    }

    #[test]
    fn no_request_resolves_to_the_default_without_complaint() {
        let (theme, notices) = set().resolve(None, TRUECOLOR);
        assert_eq!(theme.name, DEFAULT_THEME);
        assert!(notices.is_empty(), "{notices:?}");
    }

    /// A slot this version does not know is a mistake worth naming rather than
    /// dropping. Silently ignoring it means a typo'd slot leaves the theme
    /// looking almost right, which is harder to diagnose than a refusal.
    #[test]
    fn an_unknown_slot_names_itself() {
        let error = parse(
            "typo",
            "name = \"typo\"\n[slots]\nbackground = \"#000000\"\n",
            Origin::Bundled,
        )
        .expect_err("should refuse");
        assert!(error.contains("background"), "{error}");
    }

    #[test]
    fn a_malformed_colour_names_its_slot() {
        let error = parse(
            "bad",
            "name = \"bad\"\n[slots]\nink = \"not-a-colour\"\n",
            Origin::Bundled,
        )
        .expect_err("should refuse");
        assert!(error.contains("ink"), "{error}");
    }

    /// Named slots and indices are legal in a theme file: a user targeting a
    /// terminal whose palette they control should be able to say so.
    #[test]
    fn a_slot_may_name_an_ansi_colour_or_an_index() {
        let file = parse(
            "termish",
            "name = \"termish\"\n[slots]\nink = \"white\"\nground = \"16\"\n",
            Origin::Bundled,
        )
        .expect("parses");
        assert_eq!(file.colors.get("ink"), Some(&Color::White));
        assert_eq!(file.colors.get("ground"), Some(&Color::Indexed(16)));
    }

    #[test]
    fn the_bundled_set_reports_its_origin() {
        let set = set();
        for name in set.names() {
            assert_eq!(set.get(name).expect("named").origin, Origin::Bundled);
        }
        assert_eq!(Origin::Bundled.to_string(), "bundled");
    }
}
