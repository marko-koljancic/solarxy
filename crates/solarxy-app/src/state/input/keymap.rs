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

// The table lands before the dispatcher that reads it, so that the rewiring
// commit is a change of behaviour with no new data in it and this one is data
// with no change of behaviour. `the_table_agrees_with_the_window_claim_list`
// is what makes that split safe. The allow goes when the dispatcher lands.
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
    // Edit
    Undo,
    Redo,
    RedoAlt,
    Copy,
    Paste,
    Duplicate,
    CookNow,
    // Chrome
    ToggleSidebar,
    ToggleMenuBar,
    ToggleFullscreen,
    ToggleConsole,
    ToggleViewportPanel,
    // Framing and views
    FitView,
    ViewTop,
    ViewFront,
    ViewLeft,
    ViewRight,
    ProjectionPerspective,
    ProjectionOrthographic,
    LinkCameras,
    // Pane layouts
    LayoutSingle,
    LayoutSplitVertical,
    LayoutSplitHorizontal,
    LayoutQuad,
    LayoutThreeLeftBig,
    // Display and overlays
    ToggleGrid,
    ToggleAxisGizmo,
    ToggleLocalAxes,
    CycleBackground,
    CycleBounds,
    CycleNormals,
    CycleUvMode,
    ToggleTurntable,
    ToggleValidationOverlay,
    // Shading and post
    CycleViewMode,
    CycleLineWeight,
    ToggleGhosted,
    SetShaded,
    ToggleMaterialOverride,
    NextMaterialOverride,
    ToggleIbl,
    LockLights,
    ToggleToneMode,
    ToggleBloom,
    ToggleSsao,
    ExposureUp,
    ExposureDown,
    // Inspection modes
    InspectShaded,
    InspectMaterialId,
    InspectUvMap,
    InspectTexelDensity,
    InspectDepth,
    InspectOverdraw,
    InspectAoPreview,
    // Capture and review
    Screenshot,
    ToggleReviewMode,
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
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::RedoAlt => "redo-alt",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::Duplicate => "duplicate",
            Self::CookNow => "cook",
            Self::ToggleSidebar => "toggle-sidebar",
            Self::ToggleMenuBar => "toggle-menu-bar",
            Self::ToggleFullscreen => "toggle-fullscreen",
            Self::ToggleConsole => "toggle-console",
            Self::ToggleViewportPanel => "toggle-viewport-panel",
            Self::FitView => "fit-view",
            Self::ViewTop => "view-top",
            Self::ViewFront => "view-front",
            Self::ViewLeft => "view-left",
            Self::ViewRight => "view-right",
            Self::ProjectionPerspective => "perspective",
            Self::ProjectionOrthographic => "orthographic",
            Self::LinkCameras => "link-cameras",
            Self::LayoutSingle => "layout-single",
            Self::LayoutSplitVertical => "layout-split-vertical",
            Self::LayoutSplitHorizontal => "layout-split-horizontal",
            Self::LayoutQuad => "layout-quad",
            Self::LayoutThreeLeftBig => "layout-three-left-big",
            Self::ToggleGrid => "grid",
            Self::ToggleAxisGizmo => "axis-gizmo",
            Self::ToggleLocalAxes => "local-axes",
            Self::CycleBackground => "background-cycle",
            Self::CycleBounds => "bounds-cycle",
            Self::CycleNormals => "normals-cycle",
            Self::CycleUvMode => "uv-mode-cycle",
            Self::ToggleTurntable => "turntable",
            Self::ToggleValidationOverlay => "validation-overlay",
            Self::CycleViewMode => "view-mode-cycle",
            Self::CycleLineWeight => "line-weight-cycle",
            Self::ToggleGhosted => "ghosted",
            Self::SetShaded => "shaded",
            Self::ToggleMaterialOverride => "material-override",
            Self::NextMaterialOverride => "material-override-next",
            Self::ToggleIbl => "ibl",
            Self::LockLights => "lock-lights",
            Self::ToggleToneMode => "tone-mode",
            Self::ToggleBloom => "bloom",
            Self::ToggleSsao => "ssao",
            Self::ExposureUp => "exposure-up",
            Self::ExposureDown => "exposure-down",
            Self::InspectShaded => "inspect-shaded",
            Self::InspectMaterialId => "inspect-material-id",
            Self::InspectUvMap => "inspect-uv-map",
            Self::InspectTexelDensity => "inspect-texel-density",
            Self::InspectDepth => "inspect-depth",
            Self::InspectOverdraw => "inspect-overdraw",
            Self::InspectAoPreview => "inspect-ao-preview",
            Self::Screenshot => "screenshot",
            Self::ToggleReviewMode => "review-mode",
            #[cfg(debug_assertions)]
            Self::DevObjects => "dev-objects",
            #[cfg(debug_assertions)]
            Self::DevEnvironment => "dev-environment",
        }
    }
}

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
        action,
        keys,
        scope: KeyScope::Global,
        group,
        description,
        note: None,
        claim,
        listed: true,
    }
}

const fn with_note(mut binding: Binding, note: &'static str) -> Binding {
    binding.note = Some(note);
    binding
}

/// Every binding the shell dispatches.
///
/// These are the desktop's bindings as they stand. Adopting the browser's set
/// is a change to this data and nothing else, which is the point of having it
/// in one place.
pub(crate) static BINDINGS: &[Binding] = &[
    // File
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
        "Save the scene",
        Claim::Always,
    ),
    claimed(
        Action::SaveAs,
        "mod+shift+s",
        KeyGroup::File,
        "Save the scene as\u{2026}",
        Claim::Always,
    ),
    // Edit
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
    with_note(
        claimed(
            Action::RedoAlt,
            "mod+y",
            KeyGroup::Edit,
            "Redo",
            Claim::UnlessTyping,
        ),
        "The alternate redo chord",
    ),
    claimed(
        Action::Copy,
        "mod+c",
        KeyGroup::Edit,
        "Copy the selected nodes",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::Paste,
        "mod+v",
        KeyGroup::Edit,
        "Paste nodes",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::Duplicate,
        "mod+d",
        KeyGroup::Edit,
        "Duplicate the selected nodes",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::CookNow,
        "mod+enter",
        KeyGroup::Edit,
        "Cook now, in manual cook mode",
        Claim::Always,
    ),
    // Chrome
    with_note(
        claimed(
            Action::ToggleSidebar,
            "tab",
            KeyGroup::ViewportAndLayout,
            "Show or hide the sidebar",
            Claim::UnlessTyping,
        ),
        "Over the node canvas, Tab opens the node palette",
    ),
    claimed(
        Action::ToggleMenuBar,
        "f10",
        KeyGroup::ViewportAndLayout,
        "Show or hide the menu bar",
        Claim::Always,
    ),
    claimed(
        Action::ToggleFullscreen,
        "f11",
        KeyGroup::ViewportAndLayout,
        "Full screen",
        Claim::Always,
    ),
    claimed(
        Action::ToggleConsole,
        "`",
        KeyGroup::ViewportAndLayout,
        "Show or hide the console",
        Claim::UnlessTyping,
    ),
    claimed(
        Action::ToggleViewportPanel,
        "mod+1",
        KeyGroup::ViewportAndLayout,
        "Show or hide the viewport",
        Claim::UnlessTyping,
    ),
    // Framing and views
    b(
        Action::FitView,
        "h",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Frame the scene",
    ),
    b(
        Action::ViewTop,
        "t",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "View from the top",
    ),
    b(
        Action::ViewFront,
        "f",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "View from the front",
    ),
    b(
        Action::ViewLeft,
        "l",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "View from the left",
    ),
    b(
        Action::ViewRight,
        "r",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "View from the right",
    ),
    b(
        Action::ProjectionPerspective,
        "p",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Perspective projection",
    ),
    with_note(
        b(
            Action::ProjectionOrthographic,
            "o",
            KeyScope::Global,
            KeyGroup::ViewportAndLayout,
            "Orthographic projection",
        ),
        "In a UV pane, O toggles the overlap overlay",
    ),
    b(
        Action::LinkCameras,
        "mod+l",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Link the pane cameras, in a split layout",
    ),
    // Pane layouts
    b(
        Action::LayoutSingle,
        "f1",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Single pane",
    ),
    b(
        Action::LayoutSplitVertical,
        "f2",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Split vertically",
    ),
    b(
        Action::LayoutSplitHorizontal,
        "f3",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Split horizontally",
    ),
    b(
        Action::LayoutQuad,
        "f4",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Four panes",
    ),
    b(
        Action::LayoutThreeLeftBig,
        "f5",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Three panes, one large",
    ),
    // Display and overlays
    b(
        Action::ToggleGrid,
        "g",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Show or hide the grid",
    ),
    b(
        Action::ToggleAxisGizmo,
        "a",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Show or hide the axis gizmo",
    ),
    b(
        Action::ToggleLocalAxes,
        "shift+a",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Show or hide the local axes",
    ),
    b(
        Action::CycleBackground,
        "b",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Cycle the background",
    ),
    b(
        Action::CycleBounds,
        "shift+b",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Cycle the bounds display",
    ),
    b(
        Action::CycleNormals,
        "n",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Cycle the normals display",
    ),
    with_note(
        b(
            Action::CycleUvMode,
            "u",
            KeyScope::Global,
            KeyGroup::Inspection,
            "Cycle the UV overlay",
        ),
        "In a UV pane, U cycles the UV background",
    ),
    b(
        Action::ToggleTurntable,
        "v",
        KeyScope::Global,
        KeyGroup::Playback,
        "Spin the turntable",
    ),
    b(
        Action::ToggleValidationOverlay,
        "shift+v",
        KeyScope::Global,
        KeyGroup::Review,
        "Show or hide the validation overlay",
    ),
    // Shading and post
    with_note(
        b(
            Action::CycleViewMode,
            "w",
            KeyScope::Global,
            KeyGroup::Inspection,
            "Cycle the shading mode",
        ),
        "In a ghosted pane, W toggles the ghosted wireframe",
    ),
    b(
        Action::CycleLineWeight,
        "shift+w",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Cycle the wireframe weight",
    ),
    b(
        Action::ToggleGhosted,
        "x",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Ghosted shading",
    ),
    b(
        Action::SetShaded,
        "s",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Shaded",
    ),
    b(
        Action::ToggleMaterialOverride,
        "m",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Clay override on or off",
    ),
    b(
        Action::NextMaterialOverride,
        "shift+m",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Next material override",
    ),
    b(
        Action::ToggleIbl,
        "i",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Cycle the image-based lighting mode",
    ),
    b(
        Action::LockLights,
        "shift+l",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Lock the lights to the scene",
    ),
    b(
        Action::ToggleToneMode,
        "shift+t",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Cycle the tone mapping",
    ),
    b(
        Action::ToggleBloom,
        "shift+d",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Bloom on or off",
    ),
    b(
        Action::ToggleSsao,
        "shift+o",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Ambient occlusion on or off",
    ),
    b(
        Action::ExposureUp,
        "e",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Exposure up",
    ),
    b(
        Action::ExposureDown,
        "shift+e",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Exposure down",
    ),
    // Inspection modes
    b(
        Action::InspectShaded,
        "1",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Shaded",
    ),
    b(
        Action::InspectMaterialId,
        "2",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Material ID",
    ),
    b(
        Action::InspectUvMap,
        "3",
        KeyScope::Global,
        KeyGroup::Inspection,
        "UV map pane, on or off",
    ),
    b(
        Action::InspectTexelDensity,
        "4",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Texel density",
    ),
    b(
        Action::InspectDepth,
        "5",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Depth",
    ),
    b(
        Action::InspectOverdraw,
        "6",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Overdraw",
    ),
    b(
        Action::InspectAoPreview,
        "7",
        KeyScope::Global,
        KeyGroup::Inspection,
        "Ambient occlusion preview",
    ),
    // Capture and review
    b(
        Action::Screenshot,
        "c",
        KeyScope::Global,
        KeyGroup::ViewportAndLayout,
        "Save a screenshot\u{2026}",
    ),
    b(
        Action::ToggleReviewMode,
        "shift+r",
        KeyScope::Global,
        KeyGroup::Review,
        "Review mode on or off",
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

/// A pressed key with its modifiers resolved for this platform.
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
    use crate::state::input::shell_key;
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

    /// The table says the same thing the window's claim list does.
    ///
    /// This is what makes the table trustworthy before the dispatcher reads
    /// it: the two are still separate here, so the only thing holding them
    /// together is this comparison.
    #[test]
    fn the_table_agrees_with_the_window_claim_list() {
        for binding in BINDINGS {
            let chord = Chord::parse(binding.keys).expect("parses");
            let claimed = |typing: bool| {
                shell_key(chord.code, chord.cmd, chord.shift, typing, false).is_some()
            };
            match binding.claim {
                Claim::Always => {
                    assert!(
                        claimed(false) && claimed(true),
                        "{} is not claimed",
                        binding.action.id()
                    );
                }
                Claim::UnlessTyping => {
                    assert!(claimed(false), "{} is not claimed", binding.action.id());
                    assert!(
                        !claimed(true),
                        "{} is claimed while a text field has focus",
                        binding.action.id()
                    );
                }
                Claim::Never => {
                    assert!(
                        !claimed(false) && !claimed(true),
                        "{} is claimed by the window but the table says otherwise",
                        binding.action.id()
                    );
                }
            }
        }
    }

    /// Tab is the sidebar's everywhere and the palette's over the canvas,
    /// which is the one place the shell already resolves a binding by where
    /// the pointer is. The scope column is what generalizes it.
    #[test]
    fn tab_over_the_canvas_is_not_the_sidebars() {
        assert!(shell_key(KeyCode::Tab, false, false, false, false).is_some());
        assert!(shell_key(KeyCode::Tab, false, false, false, true).is_none());
    }

    #[test]
    fn a_scoped_lookup_falls_back_to_the_global_scope() {
        let grid = Chord::parse("g").expect("parses");
        assert_eq!(
            lookup(grid, KeyScope::Global).map(|b| b.action),
            Some(Action::ToggleGrid)
        );
        assert_eq!(
            lookup(grid, KeyScope::Canvas).map(|b| b.action),
            Some(Action::ToggleGrid)
        );
        assert_eq!(
            lookup(grid, KeyScope::Viewport).map(|b| b.action),
            Some(Action::ToggleGrid)
        );
    }

    #[test]
    fn a_menu_reads_its_hint_from_the_table() {
        let save = hint(Action::Save).expect("save is bound");
        assert!(
            save.ends_with("+S"),
            "{save} does not look like the save chord"
        );
        assert_eq!(hint(Action::ToggleGrid).as_deref(), Some("G"));
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
