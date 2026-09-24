//! The guided tour's catalogue: three tours, eighteen steps, as data.
//!
//! The browser keeps this as data too, and that is why it ports rather than
//! being rewritten: the words a user reads should not depend on which shell
//! they opened. `the_catalogue_is_the_browsers` reads
//! `web/src/components/tour/steps.ts` and holds every id, title, body, side
//! and order against it, so a step reworded there and not here fails the
//! build rather than shipping two tours with the same name.
//!
//! **What does not port is how a step finds what it points at.** The
//! browser writes a CSS selector and asks the document; there is no
//! document here, so a step names one of the eight surfaces the browser's
//! selectors resolve to and the overlay is handed a rectangle for it. The
//! set is closed, which is what lets the test compare the two: each variant
//! knows the selector it stands for.

/// Which surface a step points at.
///
/// Eight, because the browser's eighteen steps resolve to eight distinct
/// selectors. Several steps share one: the node canvas is pointed at four
/// times, by two tours, saying something different each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gui) enum TourAnchor {
    Viewport,
    ToolColumn,
    PaneControls,
    NodeCanvas,
    PropertiesBody,
    ReviewPanel,
    AttrColumn,
    MenuBar,
}

impl TourAnchor {
    /// The browser selector this surface is.
    ///
    /// Exists for the comparison and nothing else: this shell resolves an
    /// anchor to a rectangle and never needs to know what a selector is,
    /// so it is compiled only where it is read.
    #[cfg(test)]
    pub(in crate::gui) const fn selector(self) -> &'static str {
        match self {
            Self::Viewport => ".viewport-pane",
            Self::ToolColumn => ".tool-column",
            Self::PaneControls => ".pane-controls",
            Self::NodeCanvas => ".node-canvas-host",
            Self::PropertiesBody => ".properties-panel-body",
            Self::ReviewPanel => ".review-panel",
            Self::AttrColumn => ".attr-column",
            Self::MenuBar => ".menu-bar",
        }
    }
}

/// Which side of its anchor a card would rather sit on.
///
/// A preference, not an instruction: the placement rule moves the card when
/// the preferred side has no room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gui) enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    /// The browser's spelling, for the comparison. Nothing draws with it.
    #[cfg(test)]
    pub(in crate::gui) const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

pub(in crate::gui) struct TourStep {
    /// Unique within its tour, not across them: three ids repeat, because
    /// the same surface is worth saying two different things about.
    pub id: &'static str,
    pub anchor: TourAnchor,
    pub title: &'static str,
    /// One or two sentences. Prose, not a feature list.
    pub body: &'static str,
    pub side: Side,
}

pub(in crate::gui) struct TourDef {
    pub id: &'static str,
    /// The label its row carries in the Help submenu.
    pub title: &'static str,
    /// Bumped when the steps change enough that someone who has seen an
    /// older one should be offered it again. Only the overview's version
    /// gates the first-run offer.
    pub version: u32,
    pub steps: &'static [TourStep],
}

const OVERVIEW: TourDef = TourDef {
    id: "overview",
    title: "Overview",
    version: 1,
    steps: &[
        TourStep {
            id: "viewport",
            anchor: TourAnchor::Viewport,
            title: "The viewport",
            body: "Your scene, rendered on the GPU. Orbit with the left mouse button, pan with the middle, zoom with the wheel.",
            side: Side::Right,
        },
        TourStep {
            id: "tools",
            anchor: TourAnchor::ToolColumn,
            title: "Select, move, rotate, scale",
            body: "Q, W, E and R switch between them. Move, rotate and scale drag a gizmo on the selected object.",
            side: Side::Right,
        },
        TourStep {
            id: "pane-menus",
            anchor: TourAnchor::PaneControls,
            title: "Per-pane display",
            body: "Every pane carries its own shading, camera and overlays, so a split view can compare two of them side by side.",
            side: Side::Bottom,
        },
        TourStep {
            id: "canvas",
            anchor: TourAnchor::NodeCanvas,
            title: "The node graph",
            body: "Solarxy is parametric: this graph builds the scene, and nothing you make here is baked. Change a value upstream and everything downstream recooks.",
            side: Side::Left,
        },
        TourStep {
            id: "palette",
            anchor: TourAnchor::NodeCanvas,
            title: "Press Tab to add a node",
            body: "The palette opens at your cursor and the node lands there. Every node carries its own documentation: select one and press I to read it.",
            side: Side::Left,
        },
        TourStep {
            id: "properties",
            anchor: TourAnchor::PropertiesBody,
            title: "Parameters",
            body: "The selected node's parameters. Drag a number to scrub it, or hold Ctrl while dragging to snap.",
            side: Side::Left,
        },
        TourStep {
            id: "review",
            anchor: TourAnchor::ReviewPanel,
            title: "Review",
            body: "Pin annotations directly onto geometry and they travel with the scene file. Useful when someone else has to look at what you made.",
            side: Side::Left,
        },
    ],
};

const MODELING: TourDef = TourDef {
    id: "modeling",
    title: "Modeling Basics",
    version: 1,
    steps: &[
        TourStep {
            id: "canvas",
            anchor: TourAnchor::NodeCanvas,
            title: "Model with nodes",
            body: "Press Tab, add a Sop container, and double-click it to step inside: the graph in there IS the model. Generators make geometry, modifiers reshape it, and the display flag picks what renders.",
            side: Side::Left,
        },
        TourStep {
            id: "properties",
            anchor: TourAnchor::PropertiesBody,
            title: "Parameters drive everything",
            body: "Scrub any number and watch the viewport follow. Nothing is baked: you can come back to any node's values at any time.",
            side: Side::Left,
        },
        TourStep {
            id: "tools",
            anchor: TourAnchor::ToolColumn,
            title: "Gizmos write nodes",
            body: "Dragging an object with W, E or R writes into a transform node in its graph, so even viewport moves stay parametric and undoable.",
            side: Side::Right,
        },
        TourStep {
            id: "pane-menus",
            anchor: TourAnchor::PaneControls,
            title: "Inspect while you build",
            body: "The bracketed pane menus switch shading, wireframe, normals and bounds per pane. The Display menu tucks the detail under submenus.",
            side: Side::Bottom,
        },
        TourStep {
            id: "attr-strip",
            anchor: TourAnchor::AttrColumn,
            title: "See your attributes",
            body: "Pick a point lane and toggle value labels, vector arrows or point numbers. The gear opens scale and color options for the arrows.",
            side: Side::Left,
        },
        TourStep {
            id: "save",
            anchor: TourAnchor::MenuBar,
            title: "Save, and learn from samples",
            body: "Save Scene writes one self-contained .slxy file. File then Sample Scenes opens worked examples whose note nodes explain each workflow in place.",
            side: Side::Bottom,
        },
    ],
};

const REVIEW: TourDef = TourDef {
    id: "review",
    title: "Review Workflow",
    version: 1,
    steps: &[
        TourStep {
            id: "menu",
            anchor: TourAnchor::MenuBar,
            title: "Review lives in the menu",
            body: "Toggle Review Mode from the Review menu, or press Shift+R. The amber dot up here shows when it is on.",
            side: Side::Bottom,
        },
        TourStep {
            id: "pin",
            anchor: TourAnchor::Viewport,
            title: "Pin notes on geometry",
            body: "In review mode, click a surface to drop an annotation right there. Pins anchor to the geometry and survive camera moves and recooks.",
            side: Side::Right,
        },
        TourStep {
            id: "panel",
            anchor: TourAnchor::ReviewPanel,
            title: "Threads and resolution",
            body: "Every annotation lives here too: reply, filter by category, re-anchor a stale pin, and mark threads complete as they resolve.",
            side: Side::Left,
        },
        TourStep {
            id: "validation",
            anchor: TourAnchor::PropertiesBody,
            title: "Validation",
            body: "Wire a validate node after your geometry and the selected node grows a Validation tab; clicking an issue flies the camera to it.",
            side: Side::Left,
        },
        TourStep {
            id: "share",
            anchor: TourAnchor::NodeCanvas,
            title: "Share the file",
            body: "One .slxy carries the scene, its assets and the whole review conversation, so the person opening it sees exactly what you annotated.",
            side: Side::Left,
        },
    ],
};

/// The three tours, in the order the Help submenu lists them.
pub(in crate::gui) const TOURS: &[TourDef] = &[OVERVIEW, MODELING, REVIEW];

/// The tour a replay asks for. An unknown id falls back to the overview,
/// which is what the browser does and for the same reason: a replay that
/// silently does nothing reads as a broken menu entry.
pub(in crate::gui) fn tour_by_id(id: &str) -> &'static TourDef {
    TOURS.iter().find(|t| t.id == id).unwrap_or(&TOURS[0])
}

/// The overview's version, which is what the first-run offer is gated on.
pub(crate) fn overview_version() -> u32 {
    TOURS[0].version
}

/// Whether this installation should be offered the tour unasked.
///
/// Offered when it has never been offered, and again when the overview has
/// changed materially since the version it saw. Someone upgrading from a
/// build that had no tour reads as never offered and is shown it once,
/// which is deliberate and is what the browser does with an existing user.
#[must_use]
pub(crate) fn should_offer(seen: bool, seen_version: u32) -> bool {
    !seen || seen_version < overview_version()
}

/// Whether finishing this tour records that the first run is done.
///
/// Only the overview does. Replaying a topic tour from the Help menu must
/// not consume someone's first run, and finishing one says nothing about
/// whether they have been introduced to the application.
pub(crate) fn completion_writes_onboarding(tour_id: &str) -> bool {
    tour_id == "overview"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser_source() -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        std::fs::read_to_string(root.join("web/src/components/tour/steps.ts"))
            .expect("the browser's tour catalogue")
    }

    /// Every quoted value of one STEP field in the browser's catalogue, in
    /// order.
    ///
    /// Anchored on the indentation, because a tour and its steps both
    /// declare an `id` and one of the step ids is also a tour id: matching
    /// the bare field name reads the tours as steps and then eats the step
    /// that shares a name with one.
    fn field(source: &str, name: &str) -> Vec<String> {
        source
            .split(&format!("\n      {name}: \""))
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect()
    }

    /// The tour ids, taken from the declarations rather than from the
    /// indentation: the type the tours are declared with lists the same
    /// three ids as a union, at the same indent.
    fn tour_ids(source: &str) -> Vec<String> {
        source
            .split(": TourDef = {\n  id: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect()
    }

    /// The words are the browser's, step for step and in its order.
    ///
    /// Each field is read across the whole catalogue rather than per tour,
    /// because the steps run in one sequence there and the concatenation of
    /// this shell's three tours is that same sequence. A step moved between
    /// tours therefore fails too.
    #[test]
    fn the_catalogue_is_the_browsers() {
        let source = browser_source();
        let here: Vec<&TourStep> = TOURS.iter().flat_map(|t| t.steps.iter()).collect();
        assert_eq!(here.len(), 18, "the catalogue is eighteen steps");

        assert_eq!(
            here.iter().map(|s| s.id).collect::<Vec<_>>(),
            field(&source, "id"),
            "the step ids, in order"
        );
        assert_eq!(
            here.iter().map(|s| s.title).collect::<Vec<_>>(),
            field(&source, "title"),
            "the step titles, in order"
        );
        assert_eq!(
            here.iter().map(|s| s.body).collect::<Vec<_>>(),
            field(&source, "body"),
            "the step bodies, word for word"
        );
        assert_eq!(
            here.iter().map(|s| s.side.as_str()).collect::<Vec<_>>(),
            field(&source, "side"),
            "the preferred sides"
        );
        assert_eq!(
            here.iter().map(|s| s.anchor.selector()).collect::<Vec<_>>(),
            field(&source, "target"),
            "what each step points at"
        );
    }

    /// The three tours are the browser's three, named as it names them and
    /// in the order its Help submenu lists them.
    #[test]
    fn the_tours_are_the_browsers_three() {
        let source = browser_source();
        assert_eq!(tour_ids(&source), ["overview", "modeling", "review"]);
        assert_eq!(
            TOURS.iter().map(|t| t.id).collect::<Vec<_>>(),
            ["overview", "modeling", "review"]
        );
        for tour in TOURS {
            assert!(
                source.contains(&format!("title: \"{}\"", tour.title)),
                "the browser no longer calls a tour {}",
                tour.title
            );
        }
    }

    /// The invariants the browser's own catalogue test asserts.
    #[test]
    fn every_tour_holds_the_catalogue_invariants() {
        for tour in TOURS {
            assert!(!tour.id.is_empty() && !tour.title.is_empty());
            assert!(tour.version >= 1);
            assert!(
                (4..=8).contains(&tour.steps.len()),
                "{} has {} steps",
                tour.id,
                tour.steps.len()
            );
            let mut ids: Vec<&str> = tour.steps.iter().map(|s| s.id).collect();
            ids.sort_unstable();
            let before = ids.len();
            ids.dedup();
            assert_eq!(before, ids.len(), "{} repeats a step id", tour.id);
            for step in tour.steps {
                assert!(step.title.len() > 3, "{} has a bare title", step.id);
                assert!(step.body.len() > 40, "{} has a bare body", step.id);
            }
        }
    }

    /// The Help entry is a submenu built from the catalogue on both shells,
    /// so the rows cannot come to differ from the tours they replay.
    ///
    /// This shell walks `TOURS` where the browser maps it; what is checked
    /// is that the browser still builds the rows from the catalogue rather
    /// than listing them by hand, because a hand-written row there is how
    /// the two would drift while every other test still passed.
    #[test]
    fn the_help_submenu_is_built_from_the_catalogue() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        let bar = std::fs::read_to_string(root.join("web/src/components/menu/MenuBar.tsx"))
            .expect("the browser's menu bar");
        let entry = bar
            .split("label: \"Take a Tour\",")
            .nth(1)
            .and_then(|rest| rest.split("},").next())
            .expect("the browser's tour entry");
        assert!(
            entry.contains("submenu: TOURS.map("),
            "the browser no longer builds its tour rows from the catalogue"
        );
        assert!(
            entry.contains("label: t.title"),
            "the browser no longer names each row after its tour"
        );
    }

    /// An unknown id falls back to the overview rather than to nothing.
    #[test]
    fn an_unknown_tour_falls_back_to_the_overview() {
        assert_eq!(tour_by_id("modeling").id, "modeling");
        assert_eq!(tour_by_id("review").id, "review");
        assert_eq!(tour_by_id("no-such-tour").id, "overview");
        assert_eq!(tour_by_id("").id, "overview");
    }

    /// The offer happens once, and again only when the overview moves on.
    #[test]
    fn the_first_run_is_offered_once_and_after_a_version_moves() {
        assert!(should_offer(false, 0), "never offered");
        assert!(!should_offer(true, overview_version()), "already seen it");
        assert!(
            should_offer(true, overview_version() - 1),
            "saw an older overview"
        );
        // An upgrade from a build with no tour at all: the stored version
        // is zero and the flag is false, so it is offered.
        assert!(should_offer(false, 0));
    }

    /// Only the overview's completion records the first run, which is what
    /// keeps a replayed topic tour from consuming it.
    #[test]
    fn only_the_overview_writes_the_first_run() {
        assert!(completion_writes_onboarding("overview"));
        assert!(!completion_writes_onboarding("modeling"));
        assert!(!completion_writes_onboarding("review"));

        // The gate lives beside the component that runs a tour rather than
        // in the catalogue, because it is a rule about the first run.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        let component = std::fs::read_to_string(root.join("web/src/components/tour/Tour.tsx"))
            .expect("the browser's tour component");
        assert!(
            component.contains("tourId === \"overview\""),
            "the browser no longer gates the first run on the overview alone"
        );
    }

    /// Every surface a step points at is one the browser points at too, and
    /// none is spelled twice.
    #[test]
    fn the_anchors_are_the_browsers_eight() {
        let source = browser_source();
        let mut targets: Vec<String> = field(&source, "target");
        targets.sort();
        targets.dedup();
        assert_eq!(targets.len(), 8, "the browser points at eight surfaces");

        let mut ours: Vec<&str> = TOURS
            .iter()
            .flat_map(|t| t.steps.iter())
            .map(|s| s.anchor.selector())
            .collect();
        ours.sort_unstable();
        ours.dedup();
        assert_eq!(ours, targets);
    }
}
