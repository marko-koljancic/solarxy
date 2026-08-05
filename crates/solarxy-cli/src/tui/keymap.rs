//! One table, and both surfaces generated from it.
//!
//! # Why generate rather than maintain
//!
//! Every hand-kept shortcut list reaches the same end: the footer says one
//! thing, the help says another, and the code does a third. The shipped shell
//! is already there, with twelve bindings of which six are advertised and one
//! shift-only key that appears nowhere at all.
//!
//! So the table is the source and the footer and the help overlay are both
//! views of it. A key absent here cannot appear in either, and a task whose
//! key the footer never shows cannot exist, because the footer is built from
//! the same rows the dispatcher reads.
//!
//! # Four contexts, not three
//!
//! The design names global, focused panel and arrange mode. A fourth is
//! needed: each panel's own actions, the words its border already carries.
//! Without it the border says a panel can cycle its axis and nothing tells a
//! reader how, which is the failure this module exists to prevent. Panels
//! declare their own bindings and the footer's right half changes with focus.
//!
//! # Case, and the one exception
//!
//! Every single-letter binding accepts both cases and the footer shows the
//! lowercase form. `g` and `G` are the documented exception: the vim
//! convention for first and last is older and stronger than the rule.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::caps::GlyphTier;

/// Where a binding applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Context {
    /// Available everywhere, including inside a panel.
    Global,
    /// Acts on the focused panel, whichever it is.
    Focused,
    /// Only while arranging.
    Arrange,
    /// The focused panel's own actions, which is its border menu.
    Panel,
}

impl Context {
    pub fn heading(self) -> &'static str {
        match self {
            Self::Global => "GLOBAL",
            Self::Focused => "FOCUSED PANEL",
            Self::Arrange => "ARRANGE MODE",
            Self::Panel => "THIS PANEL",
        }
    }
}

/// What a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    FocusAddress(u8),
    CyclePreset,
    EnterArrange,
    SaveLayout,
    Help,
    Quit,

    SelectUp,
    SelectDown,
    First,
    Last,
    Open,
    Restore,
    Filter,
    Sort,
    Export,

    ArrangeLeft,
    ArrangeDown,
    ArrangeUp,
    ArrangeRight,
    SplitHorizontal,
    SplitVertical,
    Close,
    Add,
    GrowDivider,
    ShrinkDivider,
    Balance,
    LeaveArrange,
}

/// One row of the table.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub context: Context,
    /// How the key is written in the footer and in help.
    pub label: &'static str,
    /// What the row says the key does.
    pub describes: &'static str,
    pub command: Command,
    key: Key,
}

/// The key itself, in the shape the dispatcher compares against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Char(char),
    /// A letter whose case is significant, which is only ever `G`.
    ExactChar(char),
    Control(char),
    Code(KeyCode),
    /// Any of the nine jump addresses.
    Digit,
    /// A row that exists to be read rather than pressed.
    ///
    /// A reader thinks of the arrows as one thing and a dispatcher cannot, so
    /// the pair is one row in both surfaces and two rows below. Marking the
    /// shown row as undispatchable is what stops it claiming both keys for
    /// whichever command it happens to carry.
    Display,
}

impl Binding {
    /// Whether this row claims a key event.
    pub fn matches(&self, event: KeyEvent) -> bool {
        let control = event.modifiers.contains(KeyModifiers::CONTROL);
        match self.key {
            Key::Char(want) => {
                !control
                    && matches!(event.code, KeyCode::Char(got)
                    if got.eq_ignore_ascii_case(&want))
            }
            Key::ExactChar(want) => !control && event.code == KeyCode::Char(want),
            Key::Control(want) => {
                control
                    && matches!(event.code, KeyCode::Char(got)
                        if got.eq_ignore_ascii_case(&want))
            }
            Key::Code(want) => !control && event.code == want,
            Key::Digit => matches!(event.code, KeyCode::Char('1'..='9')),
            Key::Display => false,
        }
    }

    /// The command, resolved against the actual key for rows that cover more
    /// than one.
    pub fn resolve(&self, event: KeyEvent) -> Command {
        match (self.key, event.code) {
            (Key::Digit, KeyCode::Char(digit @ '1'..='9')) => {
                Command::FocusAddress(digit as u8 - b'0')
            }
            _ => self.command,
        }
    }
}

const fn row(
    context: Context,
    label: &'static str,
    describes: &'static str,
    key: Key,
    command: Command,
) -> Binding {
    Binding {
        context,
        label,
        describes,
        command,
        key,
    }
}

/// The table. Ordered as the design lists it, because the help overlay is
/// grouped in the same order and a reader comparing the two should find them
/// the same shape.
pub const TABLE: &[Binding] = &[
    row(
        Context::Global,
        "1-9",
        "focus panel by address",
        Key::Digit,
        Command::FocusAddress(0),
    ),
    row(
        Context::Global,
        "p",
        "cycle preset",
        Key::Char('p'),
        Command::CyclePreset,
    ),
    row(
        Context::Global,
        "^w",
        "enter arrange mode",
        Key::Control('w'),
        Command::EnterArrange,
    ),
    row(
        Context::Global,
        "^s",
        "save this layout",
        Key::Control('s'),
        Command::SaveLayout,
    ),
    row(
        Context::Global,
        "?",
        "help overlay",
        Key::Char('?'),
        Command::Help,
    ),
    row(Context::Global, "q", "quit", Key::Char('q'), Command::Quit),
    row(
        Context::Focused,
        "\u{2191}\u{2193} jk",
        "move selection",
        Key::Display,
        Command::SelectUp,
    ),
    row(
        Context::Focused,
        "\u{21b5}",
        "maximize, or open the row",
        Key::Code(KeyCode::Enter),
        Command::Open,
    ),
    row(
        Context::Focused,
        "esc",
        "restore from maximized",
        Key::Code(KeyCode::Esc),
        Command::Restore,
    ),
    row(
        Context::Focused,
        "/",
        "filter this panel",
        Key::Char('/'),
        Command::Filter,
    ),
    row(
        Context::Focused,
        "s",
        "cycle sort column",
        Key::Char('s'),
        Command::Sort,
    ),
    row(
        Context::Focused,
        "g G",
        "first row, last row",
        Key::Char('g'),
        Command::First,
    ),
    row(
        Context::Focused,
        "e",
        "export",
        Key::Char('e'),
        Command::Export,
    ),
    row(
        Context::Arrange,
        "h j k l",
        "move focus",
        Key::Char('h'),
        Command::ArrangeLeft,
    ),
    row(
        Context::Arrange,
        "s v",
        "split horizontal, vertical",
        Key::Char('v'),
        Command::SplitVertical,
    ),
    row(
        Context::Arrange,
        "x",
        "close focused panel",
        Key::Char('x'),
        Command::Close,
    ),
    row(
        Context::Arrange,
        "a",
        "add a panel from the catalogue",
        Key::Char('a'),
        Command::Add,
    ),
    row(
        Context::Arrange,
        "\u{2190} \u{2192}",
        "resize the active divider",
        Key::Display,
        Command::ShrinkDivider,
    ),
    row(
        Context::Arrange,
        "=",
        "balance all splits",
        Key::Char('='),
        Command::Balance,
    ),
    row(
        Context::Arrange,
        "esc",
        "leave arrange mode",
        Key::Code(KeyCode::Esc),
        Command::LeaveArrange,
    ),
];

/// Rows the `g`/`G` pair needs that the table describes as one.
///
/// The pair is one row in both surfaces because that is how a reader thinks of
/// it, and two rows in the dispatcher because the keys do different things.
/// This is the only place case is significant, which is why it is stated here
/// rather than left to a reader of the table to notice.
pub const LAST_ROW: Binding = row(
    Context::Focused,
    "G",
    "last row",
    Key::ExactChar('G'),
    Command::Last,
);

/// The keys the two shown pair rows stand for.
const SELECTION_KEYS: &[Binding] = &[
    row(
        Context::Focused,
        "j",
        "move selection down",
        Key::Char('j'),
        Command::SelectDown,
    ),
    row(
        Context::Focused,
        "k",
        "move selection up",
        Key::Char('k'),
        Command::SelectUp,
    ),
    row(
        Context::Focused,
        "\u{2193}",
        "move selection down",
        Key::Code(KeyCode::Down),
        Command::SelectDown,
    ),
    row(
        Context::Focused,
        "\u{2191}",
        "move selection up",
        Key::Code(KeyCode::Up),
        Command::SelectUp,
    ),
];

const ARRANGE_KEYS: &[Binding] = &[
    row(
        Context::Arrange,
        "j",
        "move focus down",
        Key::Char('j'),
        Command::ArrangeDown,
    ),
    row(
        Context::Arrange,
        "k",
        "move focus up",
        Key::Char('k'),
        Command::ArrangeUp,
    ),
    row(
        Context::Arrange,
        "l",
        "move focus right",
        Key::Char('l'),
        Command::ArrangeRight,
    ),
    row(
        Context::Arrange,
        "s",
        "split horizontal",
        Key::Char('s'),
        Command::SplitHorizontal,
    ),
    row(
        Context::Arrange,
        "\u{2192}",
        "grow the divider",
        Key::Code(KeyCode::Right),
        Command::GrowDivider,
    ),
    row(
        Context::Arrange,
        "\u{2190}",
        "shrink the divider",
        Key::Code(KeyCode::Left),
        Command::ShrinkDivider,
    ),
];

/// Every row the dispatcher consults, in precedence order.
///
/// A superset of what the surfaces show: the pairs the table describes as one
/// row are separate keys here, because a reader thinks of arrows as a pair and
/// a dispatcher cannot.
fn dispatchable(context: Context) -> Vec<Binding> {
    let mut rows: Vec<Binding> = TABLE
        .iter()
        .copied()
        .filter(|binding| binding.context == context)
        .collect();
    match context {
        Context::Focused => {
            rows.push(LAST_ROW);
            rows.extend(SELECTION_KEYS.iter().copied());
        }
        Context::Arrange => rows.extend(ARRANGE_KEYS.iter().copied()),
        _ => {}
    }
    rows
}

/// Resolve a key in a context, or `None` if the table does not claim it.
///
/// `G` is tried before `g` because the general rule is case-insensitive and
/// the one documented exception has to win over it.
pub fn lookup(context: Context, event: KeyEvent) -> Option<Command> {
    let rows = dispatchable(context);
    rows.iter()
        .find(|binding| matches!(binding.key, Key::ExactChar(_)) && binding.matches(event))
        .or_else(|| rows.iter().find(|binding| binding.matches(event)))
        .map(|binding| binding.resolve(event))
}

/// The rows a surface shows for a context.
pub fn rows(context: Context) -> Vec<&'static Binding> {
    TABLE
        .iter()
        .filter(|binding| binding.context == context)
        .collect()
}

/// The strip along the bottom, generated rather than written.
///
/// The panel's own words come last and change with focus, which is the whole
/// point: a border that says a panel can cycle its axis is only useful beside
/// a strip that says which key does it.
pub fn footer(panel_menu: &[&'static str], tier: GlyphTier) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for context in [Context::Global, Context::Focused] {
        for binding in rows(context) {
            out.push((label(binding, tier), binding.describes.to_owned()));
        }
    }
    for word in panel_menu {
        out.push((panel_key_label(word, tier), (*word).to_owned()));
    }
    out
}

/// A label in the repertoire the terminal actually has.
///
/// The table is written in the design's notation, which is the right thing for
/// a table read by people. A terminal without those glyphs still has to be
/// told which key to press, so the arrows and the return mark transliterate
/// rather than the row disappearing.
pub fn label(binding: &Binding, tier: GlyphTier) -> String {
    if tier == GlyphTier::Unicode {
        return binding.label.to_owned();
    }
    binding
        .label
        .chars()
        .map(|c| match c {
            '\u{2191}' => "^".to_owned(),
            '\u{2193}' => "v".to_owned(),
            '\u{2190}' => "<".to_owned(),
            '\u{2192}' => ">".to_owned(),
            '\u{21b5}' => "ent".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

/// The key that invokes a border menu word.
///
/// The word is its own mnemonic, which is the design's rule, except where the
/// first letter is already claimed by a context above it. Two are: `group`
/// would take `g`, which is first-row, and `jump` would take `j`, which is
/// down. Jump is return in the design's own footer, and group takes the next
/// unclaimed letter of its own name.
pub fn panel_key(word: &str) -> char {
    match word {
        "jump" => '\u{21b5}',
        "group" => 'o',
        _ => word.chars().next().unwrap_or('?'),
    }
}

/// The same, in the repertoire the terminal has.
pub fn panel_key_label(word: &str, tier: GlyphTier) -> String {
    match (panel_key(word), tier) {
        ('\u{21b5}', GlyphTier::Ascii) => "ent".to_owned(),
        (key, _) => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn control(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// The criterion this module exists for: a key absent from the table
    /// cannot appear in either surface, because both are built from it.
    #[test]
    fn both_surfaces_show_exactly_the_tables_rows() {
        let strip = footer(&[], GlyphTier::Unicode);
        let shown: Vec<&str> = strip.iter().map(|(label, _)| label.as_str()).collect();
        let expected: Vec<&str> = [Context::Global, Context::Focused]
            .into_iter()
            .flat_map(rows)
            .map(|binding| binding.label)
            .collect();
        assert_eq!(shown, expected);

        // Help groups the same rows by context, so every row of the table
        // appears in it and nothing else does.
        let helped: Vec<&str> = [
            Context::Global,
            Context::Focused,
            Context::Arrange,
            Context::Panel,
        ]
        .into_iter()
        .flat_map(rows)
        .map(|binding| binding.label)
        .collect();
        assert_eq!(helped.len(), TABLE.len());
    }

    /// Every task has a key the footer shows, which is the half of the
    /// criterion that is easy to satisfy by accident and easy to break.
    #[test]
    fn a_panels_own_words_reach_the_footer_with_their_keys() {
        let strip = footer(&["axis", "fit"], GlyphTier::Unicode);
        let tail: Vec<(String, String)> = strip[strip.len() - 2..].to_vec();
        assert_eq!(tail[0], ("a".to_owned(), "axis".to_owned()));
        assert_eq!(tail[1], ("f".to_owned(), "fit".to_owned()));
    }

    /// Two menu words want keys a context above them already claims, and the
    /// resolution has to be stated rather than discovered by a reader whose
    /// key did the wrong thing.
    #[test]
    fn the_two_colliding_menu_words_take_keys_of_their_own() {
        assert_eq!(panel_key("group"), 'o', "g is first-row");
        assert_eq!(panel_key("jump"), '\u{21b5}', "j is down");
        assert_eq!(panel_key("sort"), 's');
        assert_eq!(panel_key("axis"), 'a');
    }

    #[test]
    fn every_global_binding_dispatches() {
        assert_eq!(
            lookup(Context::Global, press(KeyCode::Char('4'))),
            Some(Command::FocusAddress(4))
        );
        assert_eq!(
            lookup(Context::Global, press(KeyCode::Char('p'))),
            Some(Command::CyclePreset)
        );
        assert_eq!(
            lookup(Context::Global, control('w')),
            Some(Command::EnterArrange)
        );
        assert_eq!(
            lookup(Context::Global, control('s')),
            Some(Command::SaveLayout)
        );
        assert_eq!(
            lookup(Context::Global, press(KeyCode::Char('?'))),
            Some(Command::Help)
        );
        assert_eq!(
            lookup(Context::Global, press(KeyCode::Char('q'))),
            Some(Command::Quit)
        );
    }

    #[test]
    fn every_focused_panel_binding_dispatches() {
        for (code, expected) in [
            (KeyCode::Up, Command::SelectUp),
            (KeyCode::Down, Command::SelectDown),
            (KeyCode::Char('k'), Command::SelectUp),
            (KeyCode::Char('j'), Command::SelectDown),
            (KeyCode::Enter, Command::Open),
            (KeyCode::Esc, Command::Restore),
            (KeyCode::Char('/'), Command::Filter),
            (KeyCode::Char('s'), Command::Sort),
            (KeyCode::Char('g'), Command::First),
            (KeyCode::Char('e'), Command::Export),
        ] {
            assert_eq!(
                lookup(Context::Focused, press(code)),
                Some(expected),
                "{code:?}"
            );
        }
    }

    #[test]
    fn every_arrange_binding_dispatches() {
        for (code, expected) in [
            (KeyCode::Char('h'), Command::ArrangeLeft),
            (KeyCode::Char('j'), Command::ArrangeDown),
            (KeyCode::Char('k'), Command::ArrangeUp),
            (KeyCode::Char('l'), Command::ArrangeRight),
            (KeyCode::Char('s'), Command::SplitHorizontal),
            (KeyCode::Char('v'), Command::SplitVertical),
            (KeyCode::Char('x'), Command::Close),
            (KeyCode::Char('a'), Command::Add),
            (KeyCode::Char('='), Command::Balance),
            (KeyCode::Esc, Command::LeaveArrange),
        ] {
            assert_eq!(
                lookup(Context::Arrange, press(code)),
                Some(expected),
                "{code:?}"
            );
        }
    }

    /// Every single-letter binding takes both cases, so a reader with caps
    /// lock on is not locked out of their own tool.
    #[test]
    fn single_letters_accept_both_cases() {
        for letter in ['p', 'q', 's', 'e', 'j', 'k'] {
            let upper = letter.to_ascii_uppercase();
            let context = if letter == 'p' || letter == 'q' {
                Context::Global
            } else {
                Context::Focused
            };
            assert_eq!(
                lookup(context, press(KeyCode::Char(letter))),
                lookup(context, press(KeyCode::Char(upper))),
                "{letter} and {upper} differ"
            );
        }
    }

    /// The one documented exception, and the reason it is documented: the vim
    /// convention for first and last is older than the case rule.
    #[test]
    fn the_first_and_last_pair_is_the_one_case_sensitive_binding() {
        assert_eq!(
            lookup(Context::Focused, press(KeyCode::Char('g'))),
            Some(Command::First)
        );
        assert_eq!(
            lookup(Context::Focused, press(KeyCode::Char('G'))),
            Some(Command::Last)
        );
    }

    /// A control chord is not the same key as the letter, or every arrange
    /// entry would also cycle the preset.
    #[test]
    fn a_control_chord_is_distinct_from_its_letter() {
        assert_eq!(
            lookup(Context::Global, press(KeyCode::Char('w'))),
            None,
            "plain w should not enter arrange mode"
        );
        assert_eq!(
            lookup(Context::Focused, control('s')),
            None,
            "control-s should not be sort"
        );
    }

    #[test]
    fn a_key_the_table_does_not_claim_resolves_to_nothing() {
        assert_eq!(lookup(Context::Global, press(KeyCode::Char('z'))), None);
        assert_eq!(lookup(Context::Focused, press(KeyCode::F(5))), None);
    }

    /// Every row a surface shows has to be dispatchable, or the footer is
    /// advertising a key that does nothing.
    #[test]
    fn every_row_the_surfaces_show_actually_dispatches() {
        for context in [Context::Global, Context::Focused, Context::Arrange] {
            for binding in rows(context) {
                let probe = match binding.label {
                    "1-9" => press(KeyCode::Char('1')),
                    "^w" => control('w'),
                    "^s" => control('s'),
                    label => {
                        let first = label.chars().next().expect("a label");
                        match first {
                            '\u{2191}' => press(KeyCode::Up),
                            '\u{21b5}' => press(KeyCode::Enter),
                            '\u{2190}' => press(KeyCode::Left),
                            'e' if label == "esc" => press(KeyCode::Esc),
                            other => press(KeyCode::Char(other)),
                        }
                    }
                };
                assert!(
                    lookup(context, probe).is_some(),
                    "{context:?} row {:?} shows a key that does nothing",
                    binding.label
                );
            }
        }
    }
}
