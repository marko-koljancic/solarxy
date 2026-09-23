//! The typed intent queue: what a panel asks the shell to do, and the order
//! it is done in.
//!
//! A panel draws against borrowed, read-only views of the document and of the
//! shell's settings, and never against a mutable engine or a mutable `State`.
//! That is what keeps the engine the single writer, and it means everything a
//! panel wants has to wait until the interface pass has finished. This module
//! is the vocabulary of that wait: a panel raises an [`Intent`], and
//! `State::drain_intents` applies the whole queue afterwards.
//!
//! ## The queue accumulates across passes, and that is load-bearing
//!
//! `egui::Context::run` re-invokes its closure when something requests a
//! discard, up to `max_passes`, which defaults to two. `egui::Grid` requests
//! one the first time it appears, and the Properties panel and two modals
//! draw grids, so a twice-run frame is ordinary rather than exotic. On the repeat pass the raw
//! input has been taken, so no widget reports a click and no key is consumed.
//!
//! Two rules follow, and breaking either is silent.
//!
//! - **The queue is never cleared inside the closure.** An intent raised on
//!   the first pass has to survive into the drain, and clearing at the top of
//!   each pass would throw it away on every frame that runs twice.
//! - **An intent is raised only from a widget response or a consumed key**,
//!   never from a condition that is merely true while a panel is open. A
//!   click cannot repeat on the second pass, because the events are gone by
//!   then; a state-driven raise repeats on every pass and lands in the queue
//!   twice.
//!
//! The one raise that does repeat is the split divider's, because a drag is a
//! gesture in progress rather than an event and the pointer stays down across
//! both passes. That is safe only because the intent it raises sets a ratio to
//! a value it computes from the same pointer position, so raising it twice
//! lands the same number twice. A repeating raise that accumulated, or that
//! opened a dialog, would not be.
//!
//! The flag structs this replaces were immune to both by accident: they were
//! built once outside the closure and every write was idempotent.

use solarxy_core::preferences::{
    BackgroundMode, GizmoOrientation, IblMode, InspectionMode, LineWeight, MaterialOverride,
    NormalsMode, PaneMode, ProjectionMode, UvMapBackground, ViewMode,
};
use solarxy_core::view_config::PaneLook;

use crate::state::view_state::{BoundsMode, ViewLayout};

use super::dock::SolarxyTab;
use super::panels::tree::TreeAction;
use super::panels::nodes::CanvasAction;
use super::chrome::pane_toolbar::{LookThroughChange, PaneView};
use super::chrome::viewport_context_menu::ViewportAction;

/// One thing a panel asked for during an interface pass.
///
/// Grouped by the area a panel belongs to rather than flattened, so a new
/// panel adds a variant to the enum that is already about its area instead of
/// widening one list that every panel shares.
#[derive(Debug, Clone)]
pub(crate) enum Intent {
    /// A pane's projection, picked from that pane's own toolbar.
    PaneProjection { pane: usize, mode: ProjectionMode },
    /// A pane was bound to a scene camera, or released back to a free view.
    LookThrough {
        pane: usize,
        change: LookThroughChange,
    },
    /// Author a `camera` node at a pane's current pose and bind that pane to
    /// it, from that pane's Camera menu.
    CreateCameraFromView { pane: usize },
    /// The viewport's right-click menu, acting on what the pointer landed on.
    Viewport(ViewportAction),
    /// The transform tools: which one is armed, and which frame the
    /// handles are in. Raised by the tool column, the context menu, the
    /// viewport menu and the keys, and every one of them lands in the same
    /// arm so the four cannot disagree.
    Tool(ToolIntent),
    /// The attribute strip's whole state, replaced: the toggles, the picked
    /// lane and the settings travel together, as the browser sends them.
    AttrViz(solarxy_host::attr_viz::AttrVizState),
    /// The scene clock, from the playbar and the playback keys.
    Transport(TransportIntent),
    /// A pane's own framing, from that pane's Views menu or the viewport's
    /// View menu. It writes the pane it names and nothing else.
    PaneView { pane: usize, view: PaneView },
    /// One per-pane display setting, on the pane a widget was drawn for.
    ///
    /// One variant for every pane and not two, which is the point: the active
    /// pane and the others were written through different paths before this,
    /// with a fifteen-field borrow bundle and two constructors to choose
    /// between them.
    Pane { pane: usize, change: PaneChange },
    /// One scene-global display setting.
    Display(DisplayChange),
    /// The per-pane look editor: opening one, and what it writes.
    PaneLook(PaneLookIntent),
    /// The image-based lighting mode.
    Ibl(IblMode),
    /// The File menu.
    File(FileIntent),
    /// The Edit menu.
    Edit(EditIntent),
    /// A capture of the viewport, from its own View menu and its key.
    Capture(CaptureIntent),
    /// The Review menu and the escape chain.
    Review(ReviewIntent),
    /// The Desks menu, a panel's own bar, and the split divider.
    Layout(LayoutIntent),
    /// The Help menu.
    Help(HelpIntent),
    /// A panel asked for something the shell does on its behalf.
    Panel(PanelIntent),
    /// The header strip: cook mode and the explicit cook.
    Cook(CookIntent),
}

/// One of a pane's own display settings.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PaneChange {
    PaneMode(PaneMode),
    ViewMode(ViewMode),
    InspectionMode(InspectionMode),
    MaterialOverride(MaterialOverride),
    BackgroundMode(BackgroundMode),
    NormalsMode(NormalsMode),
    BoundsMode(BoundsMode),
    LineWeight(LineWeight),
    ShowGrid(bool),
    ShowAxisGizmo(bool),
    /// The screen-constant marker at every light, per pane because its
    /// size depends on the pane's camera.
    ShowLightMarkers(bool),
    ShowValidation(bool),
    UvBackground(UvMapBackground),
    ShowUvOverlap(bool),
    /// Per-pane turntable spin. The *speed* is deliberately not here: it is
    /// scene-global on both shells, so it travels as a [`DisplayChange`].
    TurntableActive(bool),
}

/// One scene-global display setting.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DisplayChange {
    /// The turntable's speed. The *toggle* is per pane and travels as a
    /// [`PaneChange`]; the speed is scene-global on both shells.
    TurntableRpm(f32),
    HdriRotation(f32),
    HdriIntensity(f32),
}

/// The per-pane look editor.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PaneLookIntent {
    /// Show the editor for this pane, from that pane's Display menu.
    Open(usize),
    /// The whole look, replaced. The editor sends the value it drew rather
    /// than the field that moved, so two fields dragged in one frame cannot
    /// have the second overwrite the first from a stale copy.
    Set { pane: usize, look: PaneLook },
}

/// The File menu.
#[derive(Debug, Clone)]
pub(crate) enum FileIntent {
    /// Replace the document with an empty one.
    NewScene,
    OpenModel,
    /// Show the Environment dialog: the HDRI, its lighting mode, rotation
    /// and intensity.
    OpenEnvironment,
    /// Write the document to its own path, or ask for one.
    Save,
    /// Ask for a path, then write there.
    SaveAs,
    /// One of the bundled sample scenes, by its index in the menu.
    OpenSample(usize),
    /// One entry from the recent list. Routed through the file router
    /// rather than the model loader, because the one list holds scenes and
    /// models and the routing on extension exists once.
    OpenRecent(String),
    /// Ask for one or more model files and import each into the network
    /// being looked at, which is what dropping them onto the window does.
    ImportModel,
    Quit,
}

/// The Edit menu.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EditIntent {
    Undo,
    Redo,
    Copy,
    Paste,
    Duplicate,
    /// Flip the selection's bypass, every node to the opposite of what the
    /// first one is, as one undo step.
    ToggleBypass,
    /// Make the first selected node the one its network shows.
    SetDisplayFlag,
    /// Remove the selection, as one undo step.
    DeleteSelection,
    OpenPreferences,
}

/// What the viewport produces a file from. A still is not here: it is
/// started from the render node's own action, as it is in the browser.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CaptureIntent {
    Screenshot,
    /// Open the turntable export dialog. The export itself starts from the
    /// dialog, which is where the folder and the frame count are chosen.
    Turntable,
}

/// The scene clock's controls. The first five are session state and never
/// undo; the range, the rate and the loop are document state and do.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TransportIntent {
    Play,
    Pause,
    /// Stop and rewind to the range start.
    Stop,
    /// Step by a signed number of frames.
    Step(i64),
    /// Seek, from the frame field or a scrub; no undo step.
    SetFrame(i64),
    SetRange {
        start: i64,
        end: i64,
    },
    SetFps(f64),
    SetLoop(solarxy_graph::runtime::LoopMode),
    /// Show or hide the playbar, a saved preference.
    ToggleBar,
}

/// The transform tools and the frame their handles align to.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ToolIntent {
    /// Arm a tool. Arming the one already armed changes nothing; arming
    /// another mid-drag rolls the drag back first.
    Set(solarxy_host::gizmo::ToolMode),
    /// Abandon the drag in flight: the first rung of the escape ladder,
    /// which runs inside the interface pass where the rest of the ladder
    /// lives, while the drag itself is the state's.
    CancelDrag,
    /// Which frame the Move and Rotate handles align to, from the viewport
    /// menu or the preferences dialog.
    SetOrientation(GizmoOrientation),
    /// Flip between the two frames, from the key.
    ToggleOrientation,
}

/// Review mode and its notes.
///
/// The notes are the document's, so a change to one is an engine command:
/// the three variants that carry an id or a draft are drained into one
/// command each, and each is one undo step. Placing and re-anchoring do not
/// pass through here, because both start from a click the state layer owns.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ReviewIntent {
    ToggleMode,
    ToggleMarkers,
    /// Write the notes to the sidecar an earlier release reads.
    ExportNotes,
    /// Read a sidecar's notes into the document, as one undo step.
    ImportNotes,
    /// The popup's Save: the open draft becomes an add or an edit.
    CommitDraft,
    /// The panel's Complete checkbox.
    Resolve {
        id: solarxy_graph::review::AnnotationId,
        resolved: bool,
    },
    /// The confirmation's Delete; the engine cascades to the replies.
    Delete {
        id: solarxy_graph::review::AnnotationId,
    },
    /// The mode was left through the escape chain or the status badge. The
    /// state that ends it is already written; this asks only for the toast,
    /// which is the shell's to give rather than a panel's.
    Exited,
    /// A pending re-anchor was cancelled, likewise already written.
    ReanchorCancelled,
}

/// Everything about the arrangement: which panels are up, how the viewport
/// is split, and the saved dock layout.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LayoutIntent {
    /// Show or hide one dock panel. A menu's panel rows are this variant,
    /// and so is the review panel's own close button, which is why adding a
    /// panel is a row in one table rather than a field on a shared struct.
    ToggleTab(SolarxyTab),
    /// Apply a named arrangement: the panel layout, three canvas
    /// preferences and the pane split, and never the document.
    ApplyArrangement(super::ArrangementId),
    /// Ask for a name to save the current arrangement under. The name
    /// itself comes back through the dialog, not through an intent.
    OpenArrangementSave,
    /// Delete one of the user's arrangements, by its place in the list.
    DeleteArrangement(usize),
    /// Maximize the leaf this panel sits in, or restore when anything is
    /// maximized. Raised by every panel bar's last entry.
    ToggleMaximize(SolarxyTab),
    /// Show or hide the floating parameter panel: the second host of the
    /// same panel, with a pin of its own.
    ToggleFloatingProps,
    SetLayout(ViewLayout),
    SetSplitRatio(f32),
}

/// The Help menu. Every entry opens something, so a variant names what.
#[derive(Debug, Clone, Copy)]
pub(crate) enum HelpIntent {
    Wiki,
    Shortcuts,
    About,
}

/// Cook mode and the explicit cook, raised from the header strip. The engine
/// owns both; the shell only asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CookIntent {
    SetMode(solarxy_graph::engine::CookMode),
    /// Cook what is stale now, in manual cook mode.
    CookNow,
}

/// What a panel asked for.
#[derive(Debug, Clone)]
pub(crate) enum PanelIntent {
    /// A validation row in the parameter panel was clicked: frame the
    /// issue it names, on the object the node's network belongs to.
    FlyToIssue {
        ctx: solarxy_graph::document::GraphContext,
        node: solarxy_graph::document::NodeId,
        index: usize,
    },
    /// An action parameter's button in the Properties panel. The key is the
    /// parameter's, and the drain decides what the press does.
    InvokeAction {
        ctx: solarxy_graph::document::GraphContext,
        node: solarxy_graph::document::NodeId,
        key: String,
    },
    /// A file-reference parameter asked for a file chooser. The dialog is
    /// the shell's, not the panel's: a panel that opened one would block
    /// inside an interface pass.
    ChooseAsset {
        ctx: solarxy_graph::document::GraphContext,
        node: solarxy_graph::document::NodeId,
        key: String,
    },
    /// The Environment dialog's Clear button.
    ClearHdri,
    /// The Environment dialog's Load button.
    LoadHdri,
    /// The Tree.
    Tree(TreeAction),
    /// An Assets tile was double-clicked: preview it.
    PreviewAsset { hash: String, name: String },
    /// An orbit or a dolly on the model preview.
    Preview(crate::state::preview::PreviewGesture),
    /// The node canvas.
    Canvas(CanvasAction),
    /// A parameter reset, in one command and therefore one undo step. The
    /// keys are a tab's whole group, hidden rows included; `None` is every
    /// parameter the node has, which is the engine command's own shape.
    ResetParams(
        solarxy_graph::document::GraphContext,
        solarxy_graph::document::NodeId,
        Option<Vec<String>>,
    ),
    /// Open the node info card on a node, from a surface other than the
    /// canvas it is drawn over.
    OpenNodeInfo(solarxy_graph::document::NodeId),
}

impl Intent {
    /// Where this intent sits in the drain's order.
    ///
    /// The drain applies by category rather than by whichever thing a user
    /// happened to click first, because that is what the shell did before this
    /// queue existed: a fixed sequence of blocks after the interface pass. A
    /// stable sort on this key keeps the raise order within a category and
    /// reproduces that sequence.
    ///
    /// The values are contiguous and carry no meaning beyond their order, so
    /// inserting a category means renumbering the ones after it and updating
    /// the test that pins the sequence.
    pub(crate) fn order(&self) -> u8 {
        match self {
            // Both write one pane's camera and both release that pane's
            // look-through binding on the way, so they share a key: only one
            // of them can be raised in a frame, since each is one click.
            Self::PaneProjection { .. } | Self::PaneView { .. } => 0,
            // Both end by writing a pane's binding, and both must land after
            // any framing raised in the same frame rather than before it.
            Self::CreateCameraFromView { .. } | Self::LookThrough { .. } => 1,
            // Everything a menu raises sat in one block before the queue
            // existed, in the field order of the struct it wrote. Only one
            // of them can be raised in a frame, because they are one click
            // each, so the order within this run is a reading order rather
            // than a behaviour.
            // Every setting shares one key. They wrote one struct that was
            // applied in field order, and no two of them can be raised in a
            // frame anyway, so a key each would invent an order rather than
            // preserve one.
            Self::Pane { .. } | Self::Display(_) | Self::PaneLook(_) | Self::Ibl(_) => 2,
            Self::File(_) => 3,
            Self::Edit(_) => 4,
            Self::Capture(_) => 5,
            Self::Review(_) => 6,
            Self::Layout(_) => 8,
            Self::Help(_) => 9,
            Self::Panel(PanelIntent::FlyToIssue { .. }) => 10,
            Self::Panel(PanelIntent::ClearHdri) => 11,
            Self::Panel(PanelIntent::LoadHdri) => 12,
            // The tools share the context menu's key: a tool is armed from
            // the menu as readily as from the column, and only one of the
            // two can be clicked in a frame.
            Self::Viewport(_) | Self::Tool(_) | Self::AttrViz(_) | Self::Transport(_) => 13,
            // The two graph surfaces share a key: they raise the same
            // kind of change to the same document, and only one of them
            // can be under the pointer in a frame.
            Self::Panel(
                PanelIntent::Tree(_)
                | PanelIntent::Canvas(_)
                | PanelIntent::ResetParams(..)
                | PanelIntent::OpenNodeInfo(_),
            ) => 14,
            Self::Cook(_) => 15,
            Self::Panel(
                PanelIntent::InvokeAction { .. }
                | PanelIntent::ChooseAsset { .. }
                | PanelIntent::PreviewAsset { .. }
                | PanelIntent::Preview(_),
            ) => 16,
        }
    }
}

/// The queue a panel raises into, and the drain takes from.
#[derive(Debug, Default)]
pub(crate) struct Intents(Vec<Intent>);

impl Intents {
    /// Ask for something. Called from inside the interface pass, under the
    /// two rules in this module's documentation.
    pub(crate) fn raise(&mut self, intent: Intent) {
        self.0.push(intent);
    }

    /// Ask for something a panel wants. Shorthand for the common case, which
    /// is two constructors deep at every call site without it.
    pub(crate) fn panel(&mut self, intent: PanelIntent) {
        self.raise(Intent::Panel(intent));
    }

    /// Ask for one of a pane's display settings. The pane is the index the
    /// caller stamps on, which is what lets one toolbar function serve all
    /// four.
    pub(crate) fn pane(&mut self, pane: usize, change: PaneChange) {
        self.raise(Intent::Pane { pane, change });
    }

    /// Take everything raised, ordered for application, leaving the queue
    /// empty.
    pub(crate) fn take_ordered(&mut self) -> Vec<Intent> {
        let mut queued = std::mem::take(&mut self.0);
        // Stable, which is the half of the contract that keeps two intents in
        // one category in the order the user raised them.
        queued.sort_by_key(Intent::order);
        queued
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::document::{GraphContext, NodeId};

    fn keys(intents: &mut Intents) -> Vec<u8> {
        intents.take_ordered().iter().map(Intent::order).collect()
    }

    /// The sequence is the one the shell applied before the queue existed,
    /// and the raise order does not get a vote in it.
    #[test]
    fn the_drain_order_is_the_documented_sequence() {
        let mut intents = Intents::default();
        intents.panel(PanelIntent::Tree(TreeAction::Select(
            GraphContext::Root,
            NodeId(1),
        )));
        intents.panel(PanelIntent::Canvas(CanvasAction::MoveNodes(
            GraphContext::Root,
            vec![(NodeId(1), [1.0, 1.0])],
        )));
        intents.raise(Intent::Viewport(ViewportAction::ToggleVisible(
            solarxy_core::scene::SceneObjectId(1),
        )));
        intents.panel(PanelIntent::LoadHdri);
        intents.panel(PanelIntent::ClearHdri);
        intents.panel(PanelIntent::FlyToIssue {
            ctx: GraphContext::Root,
            node: NodeId(1),
            index: 3,
        });
        intents.raise(Intent::LookThrough {
            pane: 0,
            change: LookThroughChange::Free,
        });
        intents.raise(Intent::Layout(LayoutIntent::OpenArrangementSave));
        intents.raise(Intent::File(FileIntent::Quit));
        intents.raise(Intent::Display(DisplayChange::TurntableRpm(6.0)));
        intents.raise(Intent::PaneProjection {
            pane: 0,
            mode: ProjectionMode::Orthographic,
        });
        intents.raise(Intent::Cook(CookIntent::CookNow));

        // Two fourteens: the tree and the canvas are one category,
        // because they raise the same kind of change to the same
        // document.
        assert_eq!(
            keys(&mut intents),
            vec![0, 1, 2, 3, 8, 10, 11, 12, 13, 14, 14, 15]
        );
    }

    /// Two intents in one category keep the order they were raised in, which
    /// is what makes a click sequence inside one panel mean what it looks
    /// like.
    #[test]
    fn two_intents_in_one_category_keep_their_raise_order() {
        use solarxy_core::scene::SceneObjectId;

        let mut intents = Intents::default();
        for object in [7_u64, 2, 5] {
            intents.raise(Intent::Viewport(ViewportAction::ToggleVisible(
                SceneObjectId(object),
            )));
        }

        let order: Vec<u64> = intents
            .take_ordered()
            .into_iter()
            .map(|i| match i {
                Intent::Viewport(ViewportAction::ToggleVisible(id)) => id.0,
                other => panic!("unexpected intent {other:?}"),
            })
            .collect();
        assert_eq!(order, vec![7, 2, 5]);
    }

    /// Taking the queue empties it, so a frame that raises nothing drains
    /// nothing rather than replaying the frame before it.
    #[test]
    fn taking_the_queue_empties_it() {
        let mut intents = Intents::default();
        intents.panel(PanelIntent::ClearHdri);
        assert_eq!(intents.take_ordered().len(), 1);
        assert!(intents.take_ordered().is_empty());
    }
}
