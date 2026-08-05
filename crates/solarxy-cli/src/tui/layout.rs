//! The split tree: what a layout is, and why it is a tree at all.
//!
//! # A structure, not a bag of rectangles
//!
//! Panels are leaves of a binary tree. Every split names a direction and a
//! ratio, so a layout is a small serialisable thing that survives a terminal
//! resize by re-solving rather than by remembering pixels, and that fits on
//! one line of a configuration file.
//!
//! The tree also carries a rule a bag of rectangles could not: **no
//! arrangement is reachable in which a panel is too small to read**. The
//! minimum is checked at mutation time by the tree rather than at draw time by
//! the renderer, so the failure every other tiling implementation has, the
//! sliver, does not exist here. A refused mutation returns its reason and
//! leaves the tree untouched, which is structural rather than careful:
//! mutations take `&self` and hand back a new tree, so there is no partially
//! applied state to roll back.
//!
//! # Two things that are views rather than state
//!
//! **Maximize** is an argument to the solve, not a node in the tree. Nothing
//! about the arrangement is lost while it is on, so restoring needs no undo.
//!
//! **Jump addresses** are assigned during the solve in reading order, so they
//! are a property of the frame rather than something stored that would have to
//! be renumbered every time the tree changed.

use std::fmt::Write as _;

use ratatui::layout::Rect;

/// The smallest panel that can still say anything, in cells.
pub const MIN_WIDTH: u16 = 24;
pub const MIN_HEIGHT: u16 = 6;

/// Below this width the arrangement sheds panels rather than shrinking them.
pub const NARROW_COLUMNS: u16 = 120;

/// The order panels leave in when the terminal is too narrow to hold them all.
///
/// Read it as a statement about what the tool is for at that width: the
/// question is whether the asset passes, not what it looks like, so the
/// picture goes first and the verdict goes last.
pub const ELISION_ORDER: [PanelType; 5] = [
    PanelType::Silhouette,
    PanelType::Materials,
    PanelType::Textures,
    PanelType::Bounds,
    PanelType::Distributions,
];

/// A stable handle for one leaf.
///
/// Per-panel state keys on this rather than on the panel type, so two
/// silhouette panels are two panels with their own axis and their own scroll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeafId(pub u32);

/// The ten panel types, plus the state a freshly split leaf sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelType {
    Geometry,
    Health,
    Meshes,
    Materials,
    Silhouette,
    Uv,
    Validation,
    Distributions,
    Textures,
    Bounds,
    /// A leaf that exists but has not been given a panel yet. A split creates
    /// one of these rather than guessing what the reader wanted.
    Catalogue,
}

impl PanelType {
    /// Every type a reader can choose, in catalogue order.
    pub const CHOOSABLE: [PanelType; 10] = [
        PanelType::Geometry,
        PanelType::Health,
        PanelType::Meshes,
        PanelType::Materials,
        PanelType::Silhouette,
        PanelType::Uv,
        PanelType::Validation,
        PanelType::Distributions,
        PanelType::Textures,
        PanelType::Bounds,
    ];

    /// The name as it appears in the top border, lowercase.
    pub fn name(self) -> &'static str {
        match self {
            Self::Geometry => "geometry",
            Self::Health => "health",
            Self::Meshes => "meshes",
            Self::Materials => "materials",
            Self::Silhouette => "silhouette",
            Self::Uv => "uv",
            Self::Validation => "validation",
            Self::Distributions => "distributions",
            Self::Textures => "textures",
            Self::Bounds => "bounds",
            Self::Catalogue => "+ add",
        }
    }

    /// The token used in the serialised form. Distinct from [`Self::name`]
    /// because the catalogue's display name has a space in it and the
    /// encoding cannot afford one.
    pub fn token(self) -> &'static str {
        match self {
            Self::Catalogue => "catalogue",
            other => other.name(),
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        Self::CHOOSABLE
            .into_iter()
            .chain(std::iter::once(Self::Catalogue))
            .find(|panel| panel.token() == token)
    }
}

/// Which way a split divides its area.
///
/// Named for how the children sit, taken from the design's own worked
/// example: a vertical split puts them side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Children side by side; the ratio is the left child's share of width.
    Vertical,
    /// Children stacked; the ratio is the top child's share of height.
    Horizontal,
}

impl Direction {
    fn token(self) -> char {
        match self {
            Self::Vertical => 'V',
            Self::Horizontal => 'H',
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Leaf {
        id: LeafId,
        panel: PanelType,
    },
    Split {
        dir: Direction,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// Why a mutation was refused, in the words the panel border shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The operation would take a panel below the minimum.
    TooSmall,
    /// Closing the last panel would leave nothing focused, and the invariant
    /// that exactly one panel holds focus has no state to fall back to.
    LastPanel,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooSmall => write!(f, "cannot split: {MIN_WIDTH}x{MIN_HEIGHT} min"),
            Self::LastPanel => write!(f, "cannot close the last panel"),
        }
    }
}

/// Where one leaf lands on the screen this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub id: LeafId,
    pub panel: PanelType,
    pub rect: Rect,
    /// The jump address, one-based, in reading order.
    pub address: u8,
    pub focused: bool,
}

/// A whole arrangement: the tree, who has focus, and the id counter.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    root: Node,
    focus: LeafId,
    next_id: u32,
}

impl Layout {
    /// A single panel filling the pane.
    ///
    /// The starting point for a reader who closes their way down to one, and
    /// the fixture most layout tests build from.
    #[allow(dead_code)]
    pub fn single(panel: PanelType) -> Self {
        Self {
            root: Node::Leaf {
                id: LeafId(0),
                panel,
            },
            focus: LeafId(0),
            next_id: 1,
        }
    }

    pub fn focus(&self) -> LeafId {
        self.focus
    }

    /// Every leaf in reading order.
    pub fn leaves(&self) -> Vec<(LeafId, PanelType)> {
        let mut out = Vec::new();
        collect(&self.root, &mut out);
        out
    }

    pub fn panel_of(&self, id: LeafId) -> Option<PanelType> {
        self.leaves()
            .into_iter()
            .find(|(leaf, _)| *leaf == id)
            .map(|(_, panel)| panel)
    }

    /// Move focus, ignoring a leaf that is not in this tree.
    #[must_use]
    pub fn with_focus(&self, id: LeafId) -> Self {
        let mut next = self.clone();
        if next.panel_of(id).is_some() {
            next.focus = id;
        }
        next
    }

    /// Focus by jump address as the last solve assigned them.
    #[must_use]
    pub fn with_focus_on_address(&self, area: Rect, address: u8) -> Self {
        match self
            .solve(area, None)
            .into_iter()
            .find(|p| p.address == address)
        {
            Some(found) => self.with_focus(found.id),
            None => self.clone(),
        }
    }

    /// Split the focused leaf, putting a catalogue leaf beside it.
    ///
    /// Focus stays with the original panel: the reader asked for more room to
    /// work, not to be moved somewhere they have not chosen yet.
    pub fn split(&self, area: Rect, dir: Direction) -> Result<Self, Refusal> {
        let Some(current) = self.rect_of(self.focus, area) else {
            return Err(Refusal::TooSmall);
        };
        let (first, second) = divide(current, dir, 0.5);
        if !fits(first) || !fits(second) {
            return Err(Refusal::TooSmall);
        }

        let mut next = self.clone();
        let new_id = LeafId(next.next_id);
        next.next_id += 1;
        split_leaf(&mut next.root, self.focus, dir, new_id);
        Ok(next)
    }

    /// Close the focused leaf; its sibling takes its place.
    pub fn close(&self) -> Result<Self, Refusal> {
        if matches!(self.root, Node::Leaf { .. }) {
            return Err(Refusal::LastPanel);
        }
        let mut next = self.clone();
        // Focus lands on the sibling that grew into the gap, which is the one
        // place the reader's eye is already going.
        let heir = remove_leaf(&mut next.root, self.focus).unwrap_or(self.focus);
        next.focus = heir;
        Ok(next)
    }

    /// Give the focused leaf a panel type.
    ///
    /// Called when a reader picks from the catalogue, which is arrange mode's
    /// `a` and therefore waits for the keymap.
    #[allow(dead_code)]
    #[must_use]
    pub fn assign(&self, panel: PanelType) -> Self {
        let mut next = self.clone();
        set_panel(&mut next.root, self.focus, panel);
        next
    }

    /// Move the divider above the focused leaf by a fraction.
    pub fn resize(&self, area: Rect, delta: f32) -> Result<Self, Refusal> {
        let mut next = self.clone();
        let focus = self.focus;
        if !adjust_ratio(&mut next.root, focus, delta) {
            return Err(Refusal::TooSmall);
        }
        if next.smallest(area).is_none() {
            return Err(Refusal::TooSmall);
        }
        Ok(next)
    }

    /// Reset every ratio to an even share.
    pub fn balance(&self, area: Rect) -> Result<Self, Refusal> {
        let mut next = self.clone();
        even(&mut next.root);
        if next.smallest(area).is_none() {
            return Err(Refusal::TooSmall);
        }
        Ok(next)
    }

    /// Solve the tree into placements for a given pane.
    ///
    /// `maximized` renders that leaf over the whole pane and every other leaf
    /// not at all, without the tree knowing anything about it.
    pub fn solve(&self, area: Rect, maximized: Option<LeafId>) -> Vec<Placement> {
        if let Some(id) = maximized
            && let Some(panel) = self.panel_of(id)
        {
            return vec![Placement {
                id,
                panel,
                rect: area,
                address: 1,
                focused: true,
            }];
        }

        let mut out = Vec::new();
        place(&self.root, area, &mut out);

        // A tree the terminal cannot satisfy is not mutated to fit. The
        // arrangement is the reader's, so the frame gives way instead: the
        // focused panel takes the pane until there is room again.
        if out.iter().any(|p| !fits(p.rect)) {
            let panel = self.panel_of(self.focus).unwrap_or(PanelType::Geometry);
            out = vec![Placement {
                id: self.focus,
                panel,
                rect: area,
                address: 1,
                focused: true,
            }];
        }

        // Reading order, left to right and top to bottom, so the addresses
        // match how the eye crosses the screen rather than how the tree
        // happens to nest.
        out.sort_by_key(|p| (p.rect.y, p.rect.x));
        for (index, placement) in out.iter_mut().enumerate() {
            placement.address = u8::try_from(index + 1).unwrap_or(u8::MAX);
            placement.focused = placement.id == self.focus;
        }
        out
    }

    /// Drop panels the terminal is too narrow to justify.
    ///
    /// A render-time view like maximize, not a mutation: what is persisted is
    /// always the reader's tree, so a narrow terminal can never quietly
    /// discard panels from the saved layout.
    #[must_use]
    pub fn elided(&self, columns: u16) -> Self {
        if columns >= NARROW_COLUMNS {
            return self.clone();
        }
        let mut next = self.clone();
        for panel in ELISION_ORDER {
            let victims: Vec<LeafId> = next
                .leaves()
                .into_iter()
                .filter(|(_, kind)| *kind == panel)
                .map(|(id, _)| id)
                .collect();
            for id in victims {
                if matches!(next.root, Node::Leaf { .. }) {
                    return next;
                }
                let heir = remove_leaf(&mut next.root, id);
                if next.focus == id {
                    next.focus = heir.unwrap_or(next.focus);
                }
            }
        }
        next
    }

    /// The smallest rect the tree solves to, or `None` if any is unusable.
    fn smallest(&self, area: Rect) -> Option<Rect> {
        let mut out = Vec::new();
        place(&self.root, area, &mut out);
        if out.iter().any(|p| !fits(p.rect)) {
            return None;
        }
        out.into_iter()
            .map(|p| p.rect)
            .min_by_key(|r| u32::from(r.width) * u32::from(r.height))
    }

    fn rect_of(&self, id: LeafId, area: Rect) -> Option<Rect> {
        let mut out = Vec::new();
        place(&self.root, area, &mut out);
        out.into_iter().find(|p| p.id == id).map(|p| p.rect)
    }
}

/// The serialised form.
///
/// One line per layout, so a reader can open the file, see the arrangement and
/// edit it. `V0.34(silhouette,V0.51(geometry,health))` reads as a vertical
/// split giving the left child 34% of the width, whose right child is itself a
/// vertical split.
///
/// The design source calls for JSON here, mirroring the desktop dock's
/// persisted blob. That was amended: the dock's blob is opaque by nature and
/// nobody opens it, while this file sits beside a user's other dotfiles and is
/// expected to be read. Going compact also keeps `serde` and `serde_json` out
/// of this crate, which has neither.
impl Layout {
    pub fn encode(&self) -> String {
        let mut out = String::new();
        write_node(&self.root, &mut out);
        out
    }

    /// Read a layout back, or say where it stopped making sense.
    pub fn decode(source: &str) -> Result<Self, String> {
        let mut cursor = Cursor {
            text: source.trim(),
            at: 0,
            next_id: 0,
        };
        let root = cursor.node()?;
        cursor.skip_space();
        if cursor.at < cursor.text.len() {
            return Err(format!("trailing text at byte {}", cursor.at));
        }
        let focus = first_leaf(&root);
        Ok(Self {
            root,
            focus,
            next_id: cursor.next_id,
        })
    }
}

fn write_node(node: &Node, out: &mut String) {
    match node {
        Node::Leaf { panel, .. } => out.push_str(panel.token()),
        Node::Split {
            dir,
            ratio,
            first,
            second,
        } => {
            out.push(dir.token());
            // Three decimals, because two is not finer than a cell: at 140
            // columns 0.01 is 1.4 cells, which is enough to move a divider
            // off the arrangement the design draws.
            let _ = write!(out, "{ratio:.3}");
            out.push('(');
            write_node(first, out);
            out.push(',');
            write_node(second, out);
            out.push(')');
        }
    }
}

struct Cursor<'a> {
    text: &'a str,
    at: usize,
    next_id: u32,
}

impl Cursor<'_> {
    fn skip_space(&mut self) {
        while self.text[self.at..].starts_with(' ') {
            self.at += 1;
        }
    }

    fn node(&mut self) -> Result<Node, String> {
        self.skip_space();
        let rest = &self.text[self.at..];
        let dir = match rest.chars().next() {
            Some('V') => Direction::Vertical,
            Some('H') => Direction::Horizontal,
            Some(_) => return self.leaf(),
            None => return Err(format!("expected a panel or a split at byte {}", self.at)),
        };
        self.at += 1;

        let digits: String = self.text[self.at..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if digits.is_empty() {
            return Err(format!("split at byte {} has no ratio", self.at));
        }
        self.at += digits.len();
        let ratio: f32 = digits
            .parse()
            .map_err(|_| format!("{digits} is not a ratio"))?;
        if !(0.0..=1.0).contains(&ratio) {
            return Err(format!("ratio {ratio} is outside 0 to 1"));
        }

        self.expect('(')?;
        let first = self.node()?;
        self.expect(',')?;
        let second = self.node()?;
        self.expect(')')?;
        Ok(Node::Split {
            dir,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        })
    }

    fn leaf(&mut self) -> Result<Node, String> {
        let token: String = self.text[self.at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if token.is_empty() {
            return Err(format!("expected a panel name at byte {}", self.at));
        }
        self.at += token.len();
        let panel = PanelType::from_token(&token)
            .ok_or_else(|| format!("{token} is not a panel this version knows"))?;
        let id = LeafId(self.next_id);
        self.next_id += 1;
        Ok(Node::Leaf { id, panel })
    }

    fn expect(&mut self, want: char) -> Result<(), String> {
        self.skip_space();
        if self.text[self.at..].starts_with(want) {
            self.at += want.len_utf8();
            Ok(())
        } else {
            Err(format!("expected {want:?} at byte {}", self.at))
        }
    }
}

/// The three curated arrangements, one per reason someone opens this tool.
///
/// Every ratio is read off the design's own mockups at their drawn size, which
/// is why they are not round numbers: 13 of 44 rows, 47 of 140 columns. A test
/// pins the Survey preset to those cells, so a ratio cannot drift unnoticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// First look at an unfamiliar asset: what it is, whether it is healthy,
    /// what is in it.
    Survey,
    /// Geometry work. The mesh table takes half the screen.
    Meshes,
    /// Delivery QA. Validation dominates and nothing decorative is on screen.
    Validation,
}

impl Preset {
    /// Every preset, for the tests that assert all three parse and render.
    /// The cycle a reader walks uses [`Self::next`] instead.
    #[allow(dead_code)]
    pub const ALL: [Preset; 3] = [Preset::Survey, Preset::Meshes, Preset::Validation];

    pub fn name(self) -> &'static str {
        match self {
            Self::Survey => "survey",
            Self::Meshes => "meshes",
            Self::Validation => "validation",
        }
    }

    pub fn encoded(self) -> &'static str {
        match self {
            // 13 of 44 rows to the top band; 47, 47 and 46 of 140 columns
            // across it; 16 of the remaining 31 rows to the middle band; 72
            // and 68 columns across that.
            Self::Survey => concat!(
                "H0.295(",
                "V0.336(silhouette,V0.505(geometry,health)),",
                "H0.516(V0.514(meshes,materials),validation)",
                ")"
            ),
            Self::Meshes => "H0.700(V0.650(meshes,H0.500(silhouette,distributions)),uv)",
            Self::Validation => "H0.350(V0.500(health,textures),validation)",
        }
    }

    /// The preset as a layout. Panics only if a preset's own encoding is
    /// malformed, which a test rules out.
    pub fn layout(self) -> Layout {
        Layout::decode(self.encoded())
            .unwrap_or_else(|e| panic!("the {} preset does not parse: {e}", self.name()))
    }

    /// The next preset in the cycle. Returns rather than mutates, so a caller
    /// that drops the result has cycled nothing.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Survey => Self::Meshes,
            Self::Meshes => Self::Validation,
            Self::Validation => Self::Survey,
        }
    }
}

/// Whether a rect can hold a panel at all.
///
/// The one predicate. Split, resize, balance and the solve all ask it, which
/// is what makes "a layout can never reach an unusable state" true rather
/// than nearly true.
pub fn fits(rect: Rect) -> bool {
    rect.width >= MIN_WIDTH && rect.height >= MIN_HEIGHT
}

/// Divide a rect, giving the first child `ratio` of the axis.
///
/// The second child takes the remainder rather than its own rounded share, so
/// the two always tile the parent exactly: no gap, no overlap, whatever the
/// rounding did.
fn divide(area: Rect, dir: Direction, ratio: f32) -> (Rect, Rect) {
    match dir {
        Direction::Vertical => {
            let first = scale(area.width, ratio);
            (
                Rect::new(area.x, area.y, first, area.height),
                Rect::new(area.x + first, area.y, area.width - first, area.height),
            )
        }
        Direction::Horizontal => {
            let first = scale(area.height, ratio);
            (
                Rect::new(area.x, area.y, area.width, first),
                Rect::new(area.x, area.y + first, area.width, area.height - first),
            )
        }
    }
}

fn scale(total: u16, ratio: f32) -> u16 {
    let raw = (f32::from(total) * ratio).round();
    let clamped = raw.clamp(0.0, f32::from(total));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cells = clamped as u16;
    cells
}

fn place(node: &Node, area: Rect, out: &mut Vec<Placement>) {
    match node {
        Node::Leaf { id, panel } => out.push(Placement {
            id: *id,
            panel: *panel,
            rect: area,
            address: 0,
            focused: false,
        }),
        Node::Split {
            dir,
            ratio,
            first,
            second,
        } => {
            let (a, b) = divide(area, *dir, *ratio);
            place(first, a, out);
            place(second, b, out);
        }
    }
}

fn collect(node: &Node, out: &mut Vec<(LeafId, PanelType)>) {
    match node {
        Node::Leaf { id, panel } => out.push((*id, *panel)),
        Node::Split { first, second, .. } => {
            collect(first, out);
            collect(second, out);
        }
    }
}

/// Turn a leaf into a split with the original on one side and a fresh
/// catalogue leaf on the other.
fn split_leaf(node: &mut Node, id: LeafId, dir: Direction, new_id: LeafId) -> bool {
    match node {
        Node::Leaf { id: leaf, .. } if *leaf == id => {
            let original = std::mem::replace(
                node,
                Node::Leaf {
                    id,
                    panel: PanelType::Catalogue,
                },
            );
            *node = Node::Split {
                dir,
                ratio: 0.5,
                first: Box::new(original),
                second: Box::new(Node::Leaf {
                    id: new_id,
                    panel: PanelType::Catalogue,
                }),
            };
            true
        }
        Node::Leaf { .. } => false,
        Node::Split { first, second, .. } => {
            split_leaf(first, id, dir, new_id) || split_leaf(second, id, dir, new_id)
        }
    }
}

fn set_panel(node: &mut Node, id: LeafId, panel: PanelType) -> bool {
    match node {
        Node::Leaf {
            id: leaf,
            panel: slot,
        } if *leaf == id => {
            *slot = panel;
            true
        }
        Node::Leaf { .. } => false,
        Node::Split { first, second, .. } => {
            set_panel(first, id, panel) || set_panel(second, id, panel)
        }
    }
}

/// Remove a leaf, collapsing its parent into the surviving sibling.
///
/// Returns a leaf from the survivor for focus to land on.
fn remove_leaf(node: &mut Node, id: LeafId) -> Option<LeafId> {
    let Node::Split { first, second, .. } = node else {
        return None;
    };

    let drop_first = matches!(**first, Node::Leaf { id: leaf, .. } if leaf == id);
    let drop_second = matches!(**second, Node::Leaf { id: leaf, .. } if leaf == id);

    if drop_first || drop_second {
        let survivor = if drop_first {
            std::mem::replace(
                &mut **second,
                Node::Leaf {
                    id,
                    panel: PanelType::Catalogue,
                },
            )
        } else {
            std::mem::replace(
                &mut **first,
                Node::Leaf {
                    id,
                    panel: PanelType::Catalogue,
                },
            )
        };
        let heir = first_leaf(&survivor);
        *node = survivor;
        return Some(heir);
    }

    remove_leaf(first, id).or_else(|| remove_leaf(second, id))
}

fn first_leaf(node: &Node) -> LeafId {
    match node {
        Node::Leaf { id, .. } => *id,
        Node::Split { first, .. } => first_leaf(first),
    }
}

/// Nudge the ratio of the split immediately above a leaf.
fn adjust_ratio(node: &mut Node, id: LeafId, delta: f32) -> bool {
    let Node::Split {
        ratio,
        first,
        second,
        ..
    } = node
    else {
        return false;
    };

    let in_first = contains(first, id);
    let in_second = contains(second, id);
    if (in_first || in_second) && (first.is_leaf() || second.is_leaf()) {
        // Growing the second child means shrinking the first, so the sign
        // depends on which side the focused leaf is.
        let signed = if in_first { delta } else { -delta };
        *ratio = (*ratio + signed).clamp(0.05, 0.95);
        return true;
    }
    if in_first {
        return adjust_ratio(first, id, delta);
    }
    if in_second {
        return adjust_ratio(second, id, delta);
    }
    false
}

fn contains(node: &Node, id: LeafId) -> bool {
    match node {
        Node::Leaf { id: leaf, .. } => *leaf == id,
        Node::Split { first, second, .. } => contains(first, id) || contains(second, id),
    }
}

fn even(node: &mut Node) {
    if let Node::Split {
        ratio,
        first,
        second,
        ..
    } = node
    {
        *ratio = 0.5;
        even(first);
        even(second);
    }
}

impl Node {
    fn is_leaf(&self) -> bool {
        matches!(self, Node::Leaf { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: Rect = Rect {
        x: 0,
        y: 0,
        width: 140,
        height: 44,
    };

    fn survey() -> Layout {
        Preset::Survey.layout()
    }

    #[test]
    fn a_single_panel_fills_the_pane_and_holds_focus() {
        let layout = Layout::single(PanelType::Geometry);
        let placements = layout.solve(PANE, None);
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].rect, PANE);
        assert!(placements[0].focused);
        assert_eq!(placements[0].address, 1);
    }

    /// Siblings tile their parent exactly. The second child takes the
    /// remainder rather than its own rounded share, so an odd width cannot
    /// leave a one-cell seam or a one-cell overlap.
    #[test]
    fn siblings_tile_their_parent_with_no_gap_or_overlap() {
        for width in [100u16, 101, 139, 140] {
            for ratio in [0.33, 0.5, 0.618] {
                let area = Rect::new(0, 0, width, 44);
                let (a, b) = divide(area, Direction::Vertical, ratio);
                assert_eq!(a.x, area.x);
                assert_eq!(a.x + a.width, b.x, "seam at {width} / {ratio}");
                assert_eq!(b.x + b.width, area.x + area.width);
            }
        }
    }

    #[test]
    fn every_leaf_lands_inside_the_pane_and_none_overlap() {
        let placements = survey().solve(PANE, None);
        assert_eq!(placements.len(), 6, "the survey preset is six panels");
        for placement in &placements {
            assert!(placement.rect.x + placement.rect.width <= PANE.width);
            assert!(placement.rect.y + placement.rect.height <= PANE.height);
            assert!(fits(placement.rect), "{:?} is unusable", placement.rect);
        }
        for (i, a) in placements.iter().enumerate() {
            for b in &placements[i + 1..] {
                assert_eq!(
                    a.rect.intersection(b.rect).area(),
                    0,
                    "{:?} overlaps {:?}",
                    a.panel,
                    b.panel
                );
            }
        }
        let covered: u32 = placements
            .iter()
            .map(|p| u32::from(p.rect.width) * u32::from(p.rect.height))
            .sum();
        assert_eq!(
            covered,
            u32::from(PANE.width) * u32::from(PANE.height),
            "the panels do not cover the pane"
        );
    }

    /// Addresses follow the eye, not the tree. A nested split must not put
    /// address 3 above address 2 on screen.
    #[test]
    fn addresses_are_assigned_in_reading_order() {
        let placements = survey().solve(PANE, None);
        let names: Vec<&str> = placements.iter().map(|p| p.panel.name()).collect();
        assert_eq!(
            names,
            vec![
                "silhouette",
                "geometry",
                "health",
                "meshes",
                "materials",
                "validation"
            ],
            "reading order does not match the design's numbering"
        );
        for (index, placement) in placements.iter().enumerate() {
            assert_eq!(placement.address, u8::try_from(index + 1).expect("small"));
        }
    }

    /// The design's own arrangement, reproduced to the cell at the size it was
    /// drawn for. This is the test that catches a ratio drifting.
    #[test]
    fn the_survey_preset_reproduces_the_design_at_its_target_size() {
        let placements = survey().solve(PANE, None);
        let by_name = |name: &str| {
            placements
                .iter()
                .find(|p| p.panel.name() == name)
                .unwrap_or_else(|| panic!("{name} missing"))
                .rect
        };
        assert_eq!(by_name("silhouette"), Rect::new(0, 0, 47, 13));
        assert_eq!(by_name("geometry"), Rect::new(47, 0, 47, 13));
        assert_eq!(by_name("health"), Rect::new(94, 0, 46, 13));
        assert_eq!(by_name("meshes"), Rect::new(0, 13, 72, 16));
        assert_eq!(by_name("materials"), Rect::new(72, 13, 68, 16));
        assert_eq!(by_name("validation"), Rect::new(0, 29, 140, 15));
    }

    #[test]
    fn a_split_keeps_focus_and_opens_the_catalogue() {
        let layout = Layout::single(PanelType::Meshes);
        let split = layout.split(PANE, Direction::Vertical).expect("room");
        assert_eq!(split.focus(), layout.focus(), "focus moved");
        assert_eq!(split.panel_of(split.focus()), Some(PanelType::Meshes));

        let leaves = split.leaves();
        assert_eq!(leaves.len(), 2);
        assert!(
            leaves.iter().any(|(_, p)| *p == PanelType::Catalogue),
            "the new leaf should be waiting for a choice"
        );
    }

    #[test]
    fn assigning_replaces_the_catalogue_leaf() {
        let split = Layout::single(PanelType::Meshes)
            .split(PANE, Direction::Vertical)
            .expect("room");
        let new_leaf = split
            .leaves()
            .into_iter()
            .find(|(_, p)| *p == PanelType::Catalogue)
            .expect("catalogue")
            .0;
        let chosen = split.with_focus(new_leaf).assign(PanelType::Bounds);
        assert_eq!(chosen.panel_of(new_leaf), Some(PanelType::Bounds));
    }

    /// A refusal leaves the tree byte-identical, asserted through the
    /// encoding so the comparison covers structure and ratios together.
    #[test]
    fn a_refused_split_changes_nothing() {
        let tight = Rect::new(0, 0, 40, 10);
        let layout = Layout::single(PanelType::Meshes);
        let before = layout.encode();

        let refused = layout
            .split(tight, Direction::Vertical)
            .expect_err("40 columns cannot hold two 24-wide panels");
        assert_eq!(refused, Refusal::TooSmall);
        assert_eq!(layout.encode(), before);
        assert_eq!(refused.to_string(), "cannot split: 24x6 min");
    }

    #[test]
    fn a_split_that_fits_is_allowed_and_one_that_does_not_is_not() {
        let layout = Layout::single(PanelType::Meshes);
        assert!(
            layout
                .split(Rect::new(0, 0, 48, 6), Direction::Vertical)
                .is_ok()
        );
        assert!(
            layout
                .split(Rect::new(0, 0, 47, 6), Direction::Vertical)
                .is_err()
        );
        assert!(
            layout
                .split(Rect::new(0, 0, 24, 12), Direction::Horizontal)
                .is_ok()
        );
        assert!(
            layout
                .split(Rect::new(0, 0, 24, 11), Direction::Horizontal)
                .is_err()
        );
    }

    #[test]
    fn closing_a_panel_gives_its_room_to_the_sibling_and_keeps_focus_valid() {
        let layout = survey();
        let closed = layout.close().expect("six panels");
        assert_eq!(closed.leaves().len(), 5);
        assert!(
            closed.panel_of(closed.focus()).is_some(),
            "focus points at a leaf that no longer exists"
        );
        let placements = closed.solve(PANE, None);
        let focused = placements.iter().filter(|p| p.focused).count();
        assert_eq!(focused, 1, "exactly one panel holds focus");
    }

    #[test]
    fn the_last_panel_cannot_be_closed() {
        let layout = Layout::single(PanelType::Geometry);
        assert_eq!(layout.close().expect_err("only panel"), Refusal::LastPanel);
    }

    /// Whatever a reader does, one panel holds focus and it is a real one.
    #[test]
    fn exactly_one_panel_holds_focus_through_a_sequence_of_mutations() {
        let mut layout = survey();
        for step in 0..5 {
            let placements = layout.solve(PANE, None);
            assert_eq!(
                placements.iter().filter(|p| p.focused).count(),
                1,
                "after {step} closes"
            );
            match layout.close() {
                Ok(next) => layout = next,
                Err(Refusal::LastPanel) => break,
                Err(other) => panic!("{other}"),
            }
        }
        assert!(layout.panel_of(layout.focus()).is_some());
    }

    #[test]
    fn maximize_takes_the_pane_and_leaves_the_tree_alone() {
        let layout = survey();
        let before = layout.encode();
        let placements = layout.solve(PANE, Some(layout.focus()));
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].rect, PANE);
        assert_eq!(layout.encode(), before, "maximize touched the tree");

        let restored = layout.solve(PANE, None);
        assert_eq!(restored.len(), 6, "restore lost panels");
    }

    #[test]
    fn resizing_moves_the_divider_and_is_refused_when_it_would_not_fit() {
        let layout = survey();
        let before = layout.solve(PANE, None)[0].rect;
        let wider = layout.resize(PANE, 0.05).expect("room to move");
        let after = wider.solve(PANE, None)[0].rect;
        assert_ne!(before, after, "the divider did not move");

        let tight = Layout::single(PanelType::Meshes)
            .split(Rect::new(0, 0, 50, 8), Direction::Vertical)
            .expect("just fits");
        assert!(
            tight.resize(Rect::new(0, 0, 50, 8), 0.2).is_err(),
            "a resize past the minimum should be refused"
        );
    }

    #[test]
    fn balancing_evens_every_ratio() {
        let layout = survey().balance(PANE).expect("room");
        let placements = layout.solve(PANE, None);
        assert_eq!(placements.len(), 6);
        for placement in &placements {
            assert!(fits(placement.rect));
        }
    }

    /// The tree is never mutated to fit a terminal. The frame gives way
    /// instead, so the arrangement is still there when there is room again.
    #[test]
    fn a_pane_too_small_for_the_tree_shows_the_focused_panel_whole() {
        let layout = survey();
        let cramped = Rect::new(0, 0, 60, 20);
        let placements = layout.solve(cramped, None);
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].rect, cramped);
        assert_eq!(placements[0].id, layout.focus());
        assert_eq!(
            layout.solve(PANE, None).len(),
            6,
            "the tree survived the cramped frame"
        );
    }

    #[test]
    fn re_solving_at_a_new_size_produces_new_rects_from_the_same_tree() {
        let layout = survey();
        let wide = layout.solve(PANE, None);
        let narrow = layout.solve(Rect::new(0, 0, 130, 40), None);
        assert_eq!(wide.len(), narrow.len());
        assert_ne!(wide[0].rect, narrow[0].rect);
        assert_eq!(
            wide.iter().map(|p| p.panel).collect::<Vec<_>>(),
            narrow.iter().map(|p| p.panel).collect::<Vec<_>>(),
            "re-solving changed which panels exist"
        );
    }

    /// Below the narrow threshold panels leave in the documented order, and
    /// applying that to Survey produces exactly the arrangement the design
    /// pictures as the floor.
    #[test]
    fn the_narrow_elision_produces_the_designs_floor_arrangement() {
        let floor = survey().elided(100);
        let names: Vec<&str> = floor
            .solve(Rect::new(0, 0, 100, 29), None)
            .iter()
            .map(|p| p.panel.name())
            .collect();
        assert_eq!(names, vec!["geometry", "health", "meshes", "validation"]);
    }

    #[test]
    fn a_wide_terminal_elides_nothing() {
        let layout = survey();
        assert_eq!(layout.elided(NARROW_COLUMNS).leaves().len(), 6);
        assert_eq!(layout.elided(200).leaves().len(), 6);
    }

    /// Elision is a view. What is persisted has to be the reader's tree, or a
    /// narrow terminal would quietly delete panels from the saved layout.
    #[test]
    fn elision_does_not_touch_the_original() {
        let layout = survey();
        let before = layout.encode();
        let _ = layout.elided(90);
        assert_eq!(layout.encode(), before);
    }

    #[test]
    fn every_preset_parses_and_round_trips() {
        for preset in Preset::ALL {
            let layout = preset.layout();
            assert_eq!(
                layout.encode(),
                preset.encoded(),
                "{} does not survive a round trip",
                preset.name()
            );
            let again = Layout::decode(&layout.encode()).expect("re-parses");
            assert_eq!(again.encode(), layout.encode());
        }
    }

    /// A tree built by mutation round-trips too, which is the case that
    /// actually gets persisted: nobody saves a pristine preset.
    #[test]
    fn a_mutated_tree_round_trips() {
        let built = Layout::single(PanelType::Meshes)
            .split(PANE, Direction::Vertical)
            .expect("room")
            .assign(PanelType::Geometry)
            .split(PANE, Direction::Horizontal)
            .expect("room");

        let text = built.encode();
        let read = Layout::decode(&text).expect("parses");
        assert_eq!(read.encode(), text);
        assert_eq!(
            read.leaves().iter().map(|(_, p)| *p).collect::<Vec<_>>(),
            built.leaves().iter().map(|(_, p)| *p).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_encoding_reads_the_way_it_looks() {
        let layout = Layout::decode("V0.336(silhouette,V0.505(geometry,health))").expect("parses");
        let placements = layout.solve(PANE, None);
        assert_eq!(placements.len(), 3);
        assert_eq!(placements[0].rect.width, 47);
        assert_eq!(placements[1].rect.width, 47);
        assert_eq!(placements[2].rect.width, 46);
    }

    #[test]
    fn a_single_leaf_is_a_bare_name() {
        assert_eq!(Layout::single(PanelType::Bounds).encode(), "bounds");
        assert_eq!(
            Layout::decode("bounds").expect("parses").leaves(),
            vec![(LeafId(0), PanelType::Bounds)]
        );
    }

    /// A malformed layout says where it stopped rather than failing silently
    /// and leaving the reader with the default and no idea why.
    #[test]
    fn a_malformed_layout_says_what_is_wrong() {
        for (source, expected) in [
            ("V0.5(meshes", "expected ','"),
            ("V(meshes,health)", "no ratio"),
            ("V0.5(meshes,nonesuch)", "not a panel"),
            ("V2.0(meshes,health)", "outside 0 to 1"),
            ("meshes extra", "trailing text"),
            ("", "expected a panel"),
        ] {
            let error = Layout::decode(source).expect_err("{source} should be refused");
            assert!(
                error.contains(expected),
                "{source:?} reported {error:?}, expected it to mention {expected:?}"
            );
        }
    }

    /// Decoding assigns fresh ids, so a tree read from disk can be split
    /// without the new leaf colliding with one that came from the file.
    #[test]
    fn a_decoded_tree_can_still_be_split() {
        let layout = Layout::decode(Preset::Survey.encoded()).expect("parses");
        let split = layout.split(PANE, Direction::Horizontal).expect("room");
        let ids: Vec<LeafId> = split.leaves().into_iter().map(|(id, _)| id).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "a new leaf reused an existing id");
    }

    #[test]
    fn presets_cycle_back_to_the_start() {
        let mut preset = Preset::Survey;
        for _ in 0..Preset::ALL.len() {
            preset = preset.next();
        }
        assert_eq!(preset, Preset::Survey);
    }
}
