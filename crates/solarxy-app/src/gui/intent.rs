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
//! one the first time it appears, and three panels here draw a grid, so a
//! twice-run frame is ordinary rather than exotic. On the repeat pass the raw
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
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ProjectionMode, ToneMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_core::view_config::PostStrengths;

use crate::state::view_state::{BoundsMode, ViewLayout};

use super::dock::SolarxyTab;
use super::panels::node_tree::NodeTreeAction;
use super::panels::outliner::OutlinerAction;
use super::chrome::pane_toolbar::LookThroughChange;

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
    /// One per-pane display setting, on the pane a widget was drawn for.
    ///
    /// One variant for every pane and not two, which is the point: the active
    /// pane and the others were written through different paths before this,
    /// with a fifteen-field borrow bundle and two constructors to choose
    /// between them.
    Pane { pane: usize, change: PaneChange },
    /// One scene-global display setting.
    Display(DisplayChange),
    /// One post-processing setting.
    Post(PostChange),
    /// The image-based lighting mode.
    Ibl(IblMode),
    /// Whether the panes' cameras move together.
    LinkCameras(bool),
    /// The View menu's projection, which follows the camera link rather than
    /// naming a pane.
    Projection(ProjectionMode),
    /// The File menu.
    File(FileIntent),
    /// The Edit menu.
    Edit(EditIntent),
    /// The Render menu's two image-producing actions.
    Capture(CaptureIntent),
    /// The Review menu, the status bar's review badge, and the escape chain.
    Review(ReviewIntent),
    /// The Layout and Window menus, and the split divider.
    Layout(LayoutIntent),
    /// The Help menu.
    Help(HelpIntent),
    /// A panel asked for something the shell does on its behalf.
    Panel(PanelIntent),
    /// The header strip and the Render menu: cook mode and the explicit cook.
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
    UvMode(UvMode),
    BoundsMode(BoundsMode),
    LineWeight(LineWeight),
    ShowGrid(bool),
    ShowAxisGizmo(bool),
    ShowLocalAxes(bool),
    ShowValidation(bool),
    UvBackground(UvMapBackground),
    ShowUvOverlap(bool),
}

/// One scene-global display setting.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DisplayChange {
    TurntableActive(bool),
    TurntableRpm(f32),
    LightsLocked(bool),
    RoughnessScale(f32),
    MetallicScale(f32),
    HdriRotation(f32),
    HdriIntensity(f32),
}

/// One post-processing setting.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PostChange {
    Bloom(bool),
    Ssao(bool),
    /// The three intensities together, because they reach the renderer
    /// through one setter that clamps them and pushes both passes.
    Strengths(PostStrengths),
    ToneMode(ToneMode),
    Exposure(f32),
}

/// The File menu.
#[derive(Debug, Clone)]
pub(crate) enum FileIntent {
    OpenModel,
    OpenHdri,
    /// One entry from the recent list. Routed through the file router
    /// rather than the model loader, because the one list holds scenes and
    /// models and the routing on extension exists once.
    OpenRecent(String),
    Close,
    Quit,
}

/// The Edit menu.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EditIntent {
    OpenPreferences,
    /// Persist the current display, rendering and lighting settings.
    SaveViewDefaults,
}

/// The two things the Render menu produces a file from.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CaptureIntent {
    Screenshot,
    Still,
}

/// Review mode and its notes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ReviewIntent {
    ToggleMode,
    ToggleMarkers,
    SaveNotes,
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
    /// Show or hide one dock panel. The Window menu's rows are this variant,
    /// and so is the review panel's own close button, which is why adding a
    /// panel is a row in one table rather than a field on a shared struct.
    ToggleTab(SolarxyTab),
    ToggleMenuBar,
    ToggleStatusBar,
    SetLayout(ViewLayout),
    SetSplitRatio(f32),
    SaveDock,
    RestoreDock,
    ResetDock,
}

/// The Help menu.
#[derive(Debug, Clone, Copy)]
pub(crate) enum HelpIntent {
    OpenWiki,
    OpenShortcuts,
    CheckForUpdates,
    OpenAbout,
}

/// Cook mode and the explicit cook, raised from the header strip and the
/// Render menu. The engine owns both; the shell only asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CookIntent {
    SetMode(solarxy_graph::engine::CookMode),
    /// Cook what is stale now, in manual cook mode.
    CookNow,
}

/// What a panel asked for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PanelIntent {
    /// A validation row was clicked: frame the issue it names.
    FlyToIssue(usize),
    /// The Properties panel's Clear HDRI button.
    ClearHdri,
    /// The Properties panel's Load HDRI button, shown when none is loaded.
    LoadHdri,
    /// The Outliner, or the viewport context menu, which raises the same
    /// actions on purpose because they are the same actions.
    Outliner(OutlinerAction),
    /// The Node Tree.
    NodeTree(NodeTreeAction),
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
            Self::PaneProjection { .. } => 0,
            Self::LookThrough { .. } => 1,
            // Everything a menu raises sat in one block before the queue
            // existed, in the field order of the struct it wrote. Only one
            // of them can be raised in a frame, because they are one click
            // each, so the order within this run is a reading order rather
            // than a behaviour.
            // Every setting shares one key. They wrote one struct that was
            // applied in field order, and no two of them can be raised in a
            // frame anyway, so a key each would invent an order rather than
            // preserve one.
            Self::Pane { .. }
            | Self::Display(_)
            | Self::Post(_)
            | Self::Ibl(_)
            | Self::LinkCameras(_) => 2,
            Self::File(_) => 3,
            Self::Edit(_) => 4,
            Self::Capture(_) => 5,
            Self::Review(_) => 6,
            Self::Projection(_) => 7,
            Self::Layout(_) => 8,
            Self::Help(_) => 9,
            Self::Panel(PanelIntent::FlyToIssue(_)) => 10,
            Self::Panel(PanelIntent::ClearHdri) => 11,
            Self::Panel(PanelIntent::LoadHdri) => 12,
            Self::Panel(PanelIntent::Outliner(_)) => 13,
            Self::Panel(PanelIntent::NodeTree(_)) => 14,
            Self::Cook(_) => 15,
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
        intents.panel(PanelIntent::NodeTree(NodeTreeAction::Select(
            GraphContext::Root,
            NodeId(1),
        )));
        intents.panel(PanelIntent::Outliner(OutlinerAction::ToggleObject(
            solarxy_core::scene::SceneObjectId(1),
        )));
        intents.panel(PanelIntent::LoadHdri);
        intents.panel(PanelIntent::ClearHdri);
        intents.panel(PanelIntent::FlyToIssue(3));
        intents.raise(Intent::LookThrough {
            pane: 0,
            change: LookThroughChange::Free,
        });
        intents.raise(Intent::Layout(LayoutIntent::ResetDock));
        intents.raise(Intent::File(FileIntent::Quit));
        intents.raise(Intent::Post(PostChange::Bloom(true)));
        intents.raise(Intent::PaneProjection {
            pane: 0,
            mode: ProjectionMode::Orthographic,
        });
        intents.raise(Intent::Cook(CookIntent::CookNow));

        assert_eq!(
            keys(&mut intents),
            vec![0, 1, 2, 3, 8, 10, 11, 12, 13, 14, 15]
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
            intents.panel(PanelIntent::Outliner(OutlinerAction::ToggleObject(
                SceneObjectId(object),
            )));
        }

        let order: Vec<u64> = intents
            .take_ordered()
            .into_iter()
            .map(|i| match i {
                Intent::Panel(PanelIntent::Outliner(OutlinerAction::ToggleObject(id))) => id.0,
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
