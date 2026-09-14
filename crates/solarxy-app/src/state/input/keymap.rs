//! The binding table: every keyboard binding the shell dispatches, declared
//! once, in the shape `web/src/input/keymap.ts` already uses.
//!
//! Before this existed the same fact was written in four places: the window's
//! pre-pass claim list, the key map's match arms, the shortcuts modal's
//! hand-written sections, and the hint strings typed into the menu bar. They
//! disagreed, and the modal already omitted bindings the map had. The table is
//! the one declaration; the dispatcher reads it, the reference is generated
//! from it, and a menu asks it for a hint rather than spelling one out.
//!
//! **A binding here is a discrete press.** The arrow keys are deliberately not
//! in the table: they are a held-state camera gesture handled on press *and*
//! release, which is a pointer-style gesture rather than a command, and they
//! stay with the camera handling they belong to.
//!
//! The key spelling is the browser's exactly, so the two tables can be
//! compared literally: lowercase, `"+"`-joined, modifiers in the order `mod`,
//! `shift`, `alt`, where `mod` is the platform's command or control key.

// The dispatcher reads the table; the reference and the menus do not yet.
// Waiting on a consumer, and nothing else is: `KeyGroup` and its ordering,
// `Action::id`, the `group`, `description`, `note` and `listed` columns, and
// the three display helpers `binding_for`, `hint` and `format_keys`. The
// generated shortcuts reference takes the first set and the menu bars take
// the second; this allow goes with them.
#![allow(dead_code)]

use winit::keyboard::KeyCode;

/// Which surface a binding belongs to, resolved by what the pointer is over.
///
/// A key may mean one thing over the node canvas and another over the
/// viewport; that is what makes the map fit in one keyboard, and the note on
/// the binding is what makes it discoverable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum KeyScope {
    /// Fires anywhere outside a text field.
    Global,
    /// Fires only while the pointer is over the node canvas.
    Canvas,
    /// Fires only while the pointer is over the 3D viewport.
    Viewport,
}

/// A section heading in the generated shortcuts reference, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyGroup {
    File,
    Edit,
    NodeCanvas,
    ViewportAndLayout,
    Inspection,
    Review,
    Playback,
}

impl KeyGroup {
    /// Every group, in the order the reference lists them.
    pub(crate) const ALL: &'static [Self] = &[
        Self::File,
        Self::Edit,
        Self::NodeCanvas,
        Self::ViewportAndLayout,
        Self::Inspection,
        Self::Review,
        Self::Playback,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::NodeCanvas => "Node Canvas",
            Self::ViewportAndLayout => "Viewport & Layout",
            Self::Inspection => "Inspection",
            Self::Review => "Review",
            Self::Playback => "Playback",
        }
    }
}

/// What a binding does.
///
/// An enum rather than the browser's string id, so the dispatcher matches
/// exhaustively and a binding with no handler fails the build. [`Action::id`]
/// carries the string the browser uses, which is what the drift test compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    // File
    NewScene,
    OpenScene,
    Save,
    SaveAs,
    ShowShortcuts,
    OpenPreferences,
    // Edit
    Undo,
    Redo,
    RedoAlt,
    Copy,
    Paste,
    Duplicate,
    CookNow,
    // The node canvas, every one of them consumed by the node panel.
    Bypass,
    OpenNodePalette,
    DisplayFlag,
    Rename,
    NodeInfo,
    CanvasGrid,
    CanvasMinimap,
    CanvasControls,
    AutoLayout,
    EdgeStyle,
    CanvasFit,
    // Inspection
    InspectShaded,
    InspectMaterialId,
    ToggleUvPane,
    InspectTexelDensity,
    InspectDepth,
    InspectOverdraw,
    InspectAoPreview,
    // Viewport and layout
    LayoutSingle,
    LayoutSplitVertical,
    LayoutSplitHorizontal,
    LayoutQuad,
    LayoutThreeLeftBig,
    FitView,
    Screenshot,
    ViewTop,
    ViewFront,
    ViewLeft,
    ViewBottom,
    ProjectionPerspective,
    ProjectionOrthographic,
    // Review
    ToggleReviewMode,
    ToggleReviewPanel,
    ReviewCancel,
    // Debug harness, debug builds only
    #[cfg(debug_assertions)]
    DevObjects,
    #[cfg(debug_assertions)]
    DevEnvironment,
}

impl Action {
    /// The stable string the shells compare by. Matches
    /// `web/src/input/keymap.ts` wherever both shells have the binding.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::NewScene => "new-scene",
            Self::OpenScene => "open-scene",
            Self::Save => "save",
            Self::SaveAs => "save-as",
            Self::ShowShortcuts => "shortcuts",
            Self::OpenPreferences => "preferences",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::RedoAlt => "redo-alt",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::Duplicate => "duplicate",
            Self::CookNow => "cook",
            Self::Bypass => "bypass",
            Self::OpenNodePalette => "palette",
            Self::DisplayFlag => "display-flag",
            Self::Rename => "rename",
            Self::NodeInfo => "node-info",
            Self::CanvasGrid => "flow-grid",
            Self::CanvasMinimap => "flow-minimap",
            Self::CanvasControls => "flow-controls",
            Self::AutoLayout => "layout-cycle",
            Self::EdgeStyle => "edge-style-cycle",
            Self::CanvasFit => "canvas-fit",
            Self::InspectShaded => "inspect-shaded",
            Self::InspectMaterialId => "inspect-material",
            Self::ToggleUvPane => "uv-pane-toggle",
            Self::InspectTexelDensity => "inspect-texel",
            Self::InspectDepth => "inspect-depth",
            Self::InspectOverdraw => "inspect-overdraw",
            Self::InspectAoPreview => "inspect-ao",
            Self::LayoutSingle => "layout-single",
            Self::LayoutSplitVertical => "layout-split-v",
            Self::LayoutSplitHorizontal => "layout-split-h",
            Self::LayoutQuad => "layout-quad",
            Self::LayoutThreeLeftBig => "layout-three",
            Self::FitView => "fit",
            Self::Screenshot => "screenshot",
            Self::ViewTop => "view-top",
            Self::ViewFront => "view-front",
            Self::ViewLeft => "view-left",
            Self::ViewBottom => "view-bottom",
            Self::ProjectionPerspective => "view-perspective",
            Self::ProjectionOrthographic => "view-ortho",
            Self::ToggleReviewMode => "review-mode",
            Self::ToggleReviewPanel => "review-panel",
            Self::ReviewCancel => "review-cancel",
            #[cfg(debug_assertions)]
            Self::DevObjects => "dev-objects",
            #[cfg(debug_assertions)]
            Self::DevEnvironment => "dev-environment",
        }
    }
}

/// A pressed key with its modifiers resolved for this platform.
/// When the window takes a press out from under the interface.
///
/// This is what the split between the window's pre-pass and the key map used
/// to encode as two functions: a claimed press never reaches the map, which is
/// what stops one press running two handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Claim {
    /// The interface sees it first. Runs only for a press it did not want.
    Never,
    /// The window claims it whatever has focus. The function keys and the
    /// chords a focused text field has no use for.
    Always,
    /// The window claims it unless a text field has focus, which keeps the
    /// field's own undo and clipboard.
    UnlessTyping,
    /// A panel consumes it during the interface pass, and neither dispatcher
    /// runs it. The surface that owns the key is the one that knows where
    /// the pointer is inside itself and what to open, which a dispatcher
    /// outside the pass does not.
    Panel,
}

/// One declared binding.
pub(crate) struct Binding {
    pub action: Action,
    /// The browser's spelling: lowercase, `"+"`-joined, `mod` before `shift`
    /// before `alt`.
    pub keys: &'static str,
    pub scope: KeyScope,
    pub group: KeyGroup,
    pub description: &'static str,
    /// Shown in the reference where the same key means something else in
    /// another scope, or where the platform imposes something.
    pub note: Option<&'static str>,
    /// Whether the window takes the press before the interface sees it.
    pub claim: Claim,
    /// Appears in the generated reference. False only for the debug harness,
    /// which is dispatched but is not a user-facing binding.
    pub listed: bool,
}

/// A binding with nothing unusual about it: the interface sees it first, and
/// it is listed in the reference.
const fn b(
    action: Action,
    keys: &'static str,
    scope: KeyScope,
    group: KeyGroup,
    description: &'static str,
) -> Binding {
    Binding {
        action,
        keys,
        scope,
        group,
        description,
        note: None,
        claim: Claim::Never,
        listed: true,
    }
}

/// A binding the window claims before the interface sees it. Always global:
/// a claim resolved by where the pointer is would be decided before the
/// interface has said whether it wants the key at all.
const fn claimed(
    action: Action,
    keys: &'static str,
    group: KeyGroup,
    description: &'static str,
    claim: Claim,
) -> Binding {
    Binding {
        claim,
        ..b(action, keys, KeyScope::Global, group, description)
    }
}

/// A binding a panel consumes during the interface pass.
const fn panel(
    action: Action,
    keys: &'static str,
    scope: KeyScope,
    group: KeyGroup,
    description: &'static str,
) -> Binding {
    Binding {
        claim: Claim::Panel,
        ..b(action, keys, scope, group, description)
    }
}

const fn with_note(mut binding: Binding, note: &'static str) -> Binding {
    binding.note = Some(note);
    binding
}

/// Every binding the shell dispatches.
///
/// This is the browser's table entry for entry, minus the eleven bindings
/// whose capability the desktop has not built yet, and plus the four file
/// chords the browser advertises in its menus without binding at all. Both
/// sets are named in the drift test with their reason, so the list of
/// differences shrinks to nothing as the release proceeds.
///
/// Descriptions are the browser's verbatim wherever both shells have the
/// binding, because the drift test compares them. Notes are not compared: a
/// note says what a key means *elsewhere*, and elsewhere differs.
pub(crate) static BINDINGS: &[Binding] = &[
    // File. The first four have no counterpart in the browser's table,
    // which advertises their hints in its menus and binds none of them.
    claimed(
        Action::NewScene,
        "mod+n",
        KeyGroup::File,
        "New scene",
        Claim::Always,
    ),
    claimed(
        Action::OpenScene,
        "mod+o",
        KeyGroup::File,
        "Open a scene or a model",
        Claim::Always,
    ),
    claimed(
        Action::Save,
        "mod+s",
        KeyGroup::File,
        "Save the scene (.slxy)",
        Claim::Always,
    ),
    claimed(
        Action::SaveAs,
        "mod+shift+s",
        KeyGroup::File,
        "Save the scene as\u{2026}",
        Claim::Always,
    ),
    b(
        Action::ShowShortcuts,
        "?",
        KeyScope::Global,
        KeyGroup::File,
        "Show keyboard shortcuts",
    ),
    // The preferences modal claims its own chord during the pass, where
    // the dialog state lives.
    panel(
        Action::OpenPreferences,
        "mod+,",
        KeyScope::Global,
        KeyGroup::File,
        "Open preferences",
    ),
    // Edit.
    claimed(
        Action::Undo,
        "mod+z",
        KeyGroup::Edit,
        "Undo",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::Redo,
        "mod+shift+z",
        KeyGroup::Edit,
        "Redo",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::RedoAlt,
        "mod+y",
        KeyGroup::Edit,
        "Redo",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::Copy,
        "mod+c",
        KeyGroup::Edit,
        "Copy selection",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::Paste,
        "mod+v",
        KeyGroup::Edit,
        "Paste",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::Duplicate,
        "mod+d",
        KeyGroup::Edit,
        "Duplicate selection",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::CookNow,
        "mod+enter",
        KeyGroup::Edit,
        "Cook now (manual mode)",
        Claim::Always,
    ),
    // The node canvas. Every one is consumed by the node panel, which is
    // the surface that knows where the pointer is inside itself.
    with_note(
        panel(
            Action::Bypass,
            "b",
            KeyScope::Canvas,
            KeyGroup::NodeCanvas,
            "Toggle bypass on selection",
        ),
        "Over the viewport, B is the Bottom view",
    ),
    with_note(
        panel(
            Action::OpenNodePalette,
            "tab",
            KeyScope::Global,
            KeyGroup::NodeCanvas,
            "Open the node palette",
        ),
        "When the canvas has focus",
    ),
    panel(
        Action::DisplayFlag,
        "e",
        KeyScope::Canvas,
        KeyGroup::NodeCanvas,
        "Set the display flag on the selection (subflow)",
    ),
    panel(
        Action::Rename,
        "f2",
        KeyScope::Canvas,
        KeyGroup::NodeCanvas,
        "Rename the first selected node (inline)",
    ),
    with_note(
        panel(
            Action::NodeInfo,
            "i",
            KeyScope::Canvas,
            KeyGroup::NodeCanvas,
            "Show info for the selected node",
        ),
        "Ports, parameters and cook status; also on the hover radial",
    ),
    panel(
        Action::CanvasGrid,
        "g",
        KeyScope::Canvas,
        KeyGroup::NodeCanvas,
        "Toggle the canvas grid",
    ),
    panel(
        Action::CanvasMinimap,
        "m",
        KeyScope::Canvas,
        KeyGroup::NodeCanvas,
        "Toggle the minimap",
    ),
    panel(
        Action::CanvasControls,
        "c",
        KeyScope::Canvas,
        KeyGroup::NodeCanvas,
        "Toggle the zoom controls",
    ),
    // One layout engine here rather than the browser's two, which is a
    // decision about payload budget and not about the key.
    panel(
        Action::AutoLayout,
        "l",
        KeyScope::Canvas,
        KeyGroup::NodeCanvas,
        "Auto-layout the graph",
    ),
    panel(
        Action::EdgeStyle,
        "s",
        KeyScope::Canvas,
        KeyGroup::NodeCanvas,
        "Cycle the connection style",
    ),
    panel(
        Action::CanvasFit,
        "f",
        KeyScope::Canvas,
        KeyGroup::NodeCanvas,
        "Fit the node graph in the pane",
    ),
    // Inspection.
    b(
        Action::InspectShaded,
        "1",
        KeyScope::Viewport,
        KeyGroup::Inspection,
        "Inspection: Shaded",
    ),
    b(
        Action::InspectMaterialId,
        "2",
        KeyScope::Viewport,
        KeyGroup::Inspection,
        "Inspection: Material ID",
    ),
    b(
        Action::ToggleUvPane,
        "3",
        KeyScope::Viewport,
        KeyGroup::Inspection,
        "Toggle the UV pane",
    ),
    b(
        Action::InspectTexelDensity,
        "4",
        KeyScope::Viewport,
        KeyGroup::Inspection,
        "Inspection: Texel Density",
    ),
    b(
        Action::InspectDepth,
        "5",
        KeyScope::Viewport,
        KeyGroup::Inspection,
        "Inspection: Depth",
    ),
    b(
        Action::InspectOverdraw,
        "6",
        KeyScope::Viewport,
        KeyGroup::Inspection,
        "Inspection: Overdraw",
    ),
    b(
        Action::InspectAoPreview,
        "7",
        KeyScope::Viewport,
        KeyGroup::Inspection,
        "Inspection: AO Preview",
    ),
    // Viewport and layout.
    b(
        Action::LayoutSingle,
        "f1",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "Layout: Single",
    ),
    b(
        Action::LayoutSplitVertical,
        "f2",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "Layout: Split Vertical",
    ),
    b(
        Action::LayoutSplitHorizontal,
        "f3",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "Layout: Split Horizontal",
    ),
    b(
        Action::LayoutQuad,
        "f4",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "Layout: Quad",
    ),
    b(
        Action::LayoutThreeLeftBig,
        "f5",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "Layout: Three Left Big",
    ),
    with_note(
        b(
            Action::FitView,
            "z",
            KeyScope::Viewport,
            KeyGroup::ViewportAndLayout,
            "Fit view to the scene",
        ),
        "Moved from H; F is now the Front view",
    ),
    b(
        Action::Screenshot,
        "c",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "Screenshot the active pane",
    ),
    b(
        Action::ViewTop,
        "t",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "View: Top",
    ),
    b(
        Action::ViewFront,
        "f",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "View: Front",
    ),
    b(
        Action::ViewLeft,
        "l",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "View: Left",
    ),
    b(
        Action::ViewBottom,
        "b",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "View: Bottom",
    ),
    b(
        Action::ProjectionPerspective,
        "p",
        KeyScope::Viewport,
        KeyGroup::ViewportAndLayout,
        "Perspective projection",
    ),
    with_note(
        b(
            Action::ProjectionOrthographic,
            "o",
            KeyScope::Viewport,
            KeyGroup::ViewportAndLayout,
            "Orthographic projection",
        ),
        "In a UV pane, O toggles the overlap display instead",
    ),
    // Review.
    b(
        Action::ToggleReviewMode,
        "shift+r",
        KeyScope::Viewport,
        KeyGroup::Review,
        "Toggle review mode (click geometry to pin a note)",
    ),
    b(
        Action::ToggleReviewPanel,
        "n",
        KeyScope::Global,
        KeyGroup::Review,
        "Toggle the review panel",
    ),
    // The escape ladder runs inside the interface pass, where the state
    // each rung cancels lives.
    panel(
        Action::ReviewCancel,
        "escape",
        KeyScope::Global,
        KeyGroup::Review,
        "Cancel the note editor, the re-anchor, or review mode",
    ),
    // The debug harness. Dispatched, so it is declared, but not a user
    // binding and therefore not in the reference.
    #[cfg(debug_assertions)]
    Binding {
        listed: false,
        ..b(
            Action::DevEnvironment,
            "f8",
            KeyScope::Global,
            KeyGroup::Inspection,
            "Synthesized environment, debug builds only",
        )
    },
    #[cfg(debug_assertions)]
    Binding {
        listed: false,
        ..b(
            Action::DevObjects,
            "f9",
            KeyScope::Global,
            KeyGroup::Inspection,
            "Multi-object dev cubes, debug builds only",
        )
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Chord {
    pub code: KeyCode,
    /// Command on macOS, control elsewhere.
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub(crate) const fn new(code: KeyCode, cmd: bool, shift: bool, alt: bool) -> Self {
        Self {
            code,
            cmd,
            shift,
            alt,
        }
    }

    /// The chord a binding's spelling denotes, or `None` if the spelling names
    /// a key this shell has no code for. `every_binding_parses` holds that to
    /// never happening.
    pub(crate) fn parse(keys: &str) -> Option<Self> {
        let mut cmd = false;
        let mut shift = false;
        let mut alt = false;
        let mut code = None;
        for part in keys.split('+') {
            match part {
                "mod" => cmd = true,
                "shift" => shift = true,
                "alt" => alt = true,
                name => code = Some(key_code(name)?),
            }
        }
        Some(Self::new(code?, cmd, shift, alt))
    }
}

/// The key a binding spelling names.
fn key_code(name: &str) -> Option<KeyCode> {
    Some(match name {
        "a" => KeyCode::KeyA,
        "b" => KeyCode::KeyB,
        "c" => KeyCode::KeyC,
        "d" => KeyCode::KeyD,
        "e" => KeyCode::KeyE,
        "f" => KeyCode::KeyF,
        "g" => KeyCode::KeyG,
        "h" => KeyCode::KeyH,
        "i" => KeyCode::KeyI,
        "j" => KeyCode::KeyJ,
        "k" => KeyCode::KeyK,
        "l" => KeyCode::KeyL,
        "m" => KeyCode::KeyM,
        "n" => KeyCode::KeyN,
        "o" => KeyCode::KeyO,
        "p" => KeyCode::KeyP,
        "q" => KeyCode::KeyQ,
        "r" => KeyCode::KeyR,
        "s" => KeyCode::KeyS,
        "t" => KeyCode::KeyT,
        "u" => KeyCode::KeyU,
        "v" => KeyCode::KeyV,
        "w" => KeyCode::KeyW,
        "x" => KeyCode::KeyX,
        "y" => KeyCode::KeyY,
        "z" => KeyCode::KeyZ,
        "0" => KeyCode::Digit0,
        "1" => KeyCode::Digit1,
        "2" => KeyCode::Digit2,
        "3" => KeyCode::Digit3,
        "4" => KeyCode::Digit4,
        "5" => KeyCode::Digit5,
        "6" => KeyCode::Digit6,
        "7" => KeyCode::Digit7,
        "8" => KeyCode::Digit8,
        "9" => KeyCode::Digit9,
        "f1" => KeyCode::F1,
        "f2" => KeyCode::F2,
        "f3" => KeyCode::F3,
        "f4" => KeyCode::F4,
        "f5" => KeyCode::F5,
        "f8" => KeyCode::F8,
        "f9" => KeyCode::F9,
        "f10" => KeyCode::F10,
        "f11" => KeyCode::F11,
        "tab" => KeyCode::Tab,
        "enter" => KeyCode::Enter,
        "escape" => KeyCode::Escape,
        "space" => KeyCode::Space,
        "home" => KeyCode::Home,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "`" => KeyCode::Backquote,
        "," => KeyCode::Comma,
        "." => KeyCode::Period,
        // `?` is stored bare, as the browser stores it: the shift that
        // produces it on most layouts is folded away so a lookup matches,
        // which is why it shares the slash's physical code.
        "/" | "?" => KeyCode::Slash,
        _ => return None,
    })
}

/// The binding a chord fires in a scope, falling back to the global scope the
/// way the browser's `lookupBinding` does.
pub(crate) fn lookup(chord: Chord, scope: KeyScope) -> Option<&'static Binding> {
    let matches = |b: &&Binding| Chord::parse(b.keys) == Some(chord);
    if let Some(found) = BINDINGS.iter().find(|b| b.scope == scope && matches(b)) {
        return Some(found);
    }
    if scope == KeyScope::Global {
        return None;
    }
    BINDINGS
        .iter()
        .find(|b| b.scope == KeyScope::Global && matches(b))
}

/// The binding an action is bound to, which is how a menu renders a hint
/// without spelling one out.
pub(crate) fn binding_for(action: Action) -> Option<&'static Binding> {
    BINDINGS.iter().find(|b| b.action == action)
}

/// An action's shortcut as a menu shows it, or `None` when nothing is bound.
pub(crate) fn hint(action: Action) -> Option<String> {
    binding_for(action).map(|b| format_keys(b.keys))
}

/// A binding's spelling as a reader sees it: the platform modifier glyph or
/// word, specials named, single letters uppercased.
pub(crate) fn format_keys(keys: &str) -> String {
    keys.split('+')
        .map(|part| match part {
            "mod" => crate::gui::MOD.to_string(),
            "shift" => "Shift".to_string(),
            "alt" => ALT.to_string(),
            "enter" => "Enter".to_string(),
            "escape" => "Esc".to_string(),
            "tab" => "Tab".to_string(),
            "space" => "Space".to_string(),
            "home" => "Home".to_string(),
            "backspace" => "Backspace".to_string(),
            "delete" => "Delete".to_string(),
            other if other.len() == 1 => other.to_uppercase(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(target_os = "macos")]
const ALT: &str = "\u{2325}";
#[cfg(not(target_os = "macos"))]
const ALT: &str = "Alt";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_binding_parses() {
        for binding in BINDINGS {
            assert!(
                Chord::parse(binding.keys).is_some(),
                "{} spells a key this shell has no code for: {:?}",
                binding.action.id(),
                binding.keys
            );
        }
    }

    #[test]
    fn no_key_means_two_things_within_one_scope() {
        let mut seen = HashSet::new();
        for binding in BINDINGS {
            let chord = Chord::parse(binding.keys).expect("parses");
            assert!(
                seen.insert((chord, binding.scope)),
                "{:?} is bound twice in {:?}, the second time by {}",
                binding.keys,
                binding.scope,
                binding.action.id()
            );
        }
    }

    #[test]
    fn every_action_is_declared_once_and_has_its_own_id() {
        let mut actions = HashSet::new();
        let mut ids = HashSet::new();
        for binding in BINDINGS {
            assert!(
                actions.insert(binding.action),
                "{} is bound twice; `binding_for` would answer with the first",
                binding.action.id()
            );
            assert!(
                ids.insert(binding.action.id()),
                "{} is not a unique id",
                binding.action.id()
            );
        }
    }

    /// A letter can mean one thing over the canvas and another over the
    /// viewport, and that is what makes the map fit in one keyboard.
    ///
    /// Each of these resolved to a single global meaning before the browser's
    /// table was adopted, so this is the shape of the change rather than a
    /// detail of it.
    #[test]
    fn a_letter_can_mean_two_things_in_two_scopes() {
        let both = [
            ("b", Action::Bypass, Action::ViewBottom),
            ("c", Action::CanvasControls, Action::Screenshot),
            ("f", Action::CanvasFit, Action::ViewFront),
            ("l", Action::AutoLayout, Action::ViewLeft),
        ];
        for (keys, canvas, viewport) in both {
            let chord = Chord::parse(keys).expect("parses");
            assert_eq!(
                lookup(chord, KeyScope::Canvas).map(|b| b.action),
                Some(canvas),
                "{keys} over the canvas"
            );
            assert_eq!(
                lookup(chord, KeyScope::Viewport).map(|b| b.action),
                Some(viewport),
                "{keys} over the viewport"
            );
        }
    }

    /// Two of the browser's double-bound letters have only one half here,
    /// because the other half is a capability this release builds later.
    ///
    /// A scope with no binding resolves to nothing rather than falling back,
    /// which is what stops a canvas key firing over the viewport by accident
    /// once the second half arrives.
    #[test]
    fn a_letter_whose_other_half_is_unbuilt_resolves_in_one_scope_only() {
        let rotate_tool = Chord::parse("e").expect("parses");
        assert_eq!(
            lookup(rotate_tool, KeyScope::Canvas).map(|b| b.action),
            Some(Action::DisplayFlag)
        );
        assert_eq!(
            lookup(rotate_tool, KeyScope::Viewport).map(|b| b.action),
            None
        );

        let floating_properties = Chord::parse("p").expect("parses");
        assert_eq!(
            lookup(floating_properties, KeyScope::Viewport).map(|b| b.action),
            Some(Action::ProjectionPerspective)
        );
        assert_eq!(
            lookup(floating_properties, KeyScope::Canvas).map(|b| b.action),
            None
        );
    }

    /// Tab is the palette's, declared globally with a note, as the browser
    /// declares it. The sidebar's Tab is gone with the browser's map.
    #[test]
    fn tab_is_the_palette_in_every_scope() {
        let tab = Chord::parse("tab").expect("parses");
        for scope in [KeyScope::Global, KeyScope::Canvas, KeyScope::Viewport] {
            let found = lookup(tab, scope).expect("tab is bound");
            assert_eq!(found.action, Action::OpenNodePalette);
            assert_eq!(found.claim, Claim::Panel);
        }
    }

    /// The debug harness is dispatched, so it is declared, but a user-facing
    /// reference must not list it.
    #[test]
    fn only_the_debug_harness_is_unlisted() {
        for binding in BINDINGS.iter().filter(|b| !b.listed) {
            assert!(
                binding.keys == "f8" || binding.keys == "f9",
                "{} is hidden from the reference for no stated reason",
                binding.action.id()
            );
        }
    }
}
