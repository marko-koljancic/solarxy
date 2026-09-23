//! Named arrangements: a layout for a kind of work, chosen by name.
//!
//! An arrangement is **interface state and nothing else**: which panels are
//! up and where, how the viewport is split, and three of the canvas's
//! reading preferences. It never describes a document, which is what makes
//! switching one free. One that captured document state would make choosing
//! a layout destructive.
//!
//! The built-ins are **data, not construction code**. Each is a [`Recipe`],
//! written in the same facts the browser's presets are, and
//! [`Recipe::build`] is the one place a recipe becomes a dock tree. Adding
//! an arrangement is a row in [`BUILT_IN`].
//!
//! The table is held against the browser's own
//! (`web/src/store/desks.ts`) by a test that reads that file, so the two
//! shells cannot come to mean different things by the same name.

use egui_dock::{DockState, NodeIndex};
use solarxy_core::preferences::{CanvasPrefs, DockPrefs, UserArrangement};
use solarxy_core::view_config::ViewLayout;

use super::dock::SolarxyTab;

/// Where the parameter panel sits relative to the node canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertiesDock {
    /// Under the canvas, sharing its column.
    Bottom,
    /// A full-height column of its own, on the far side from the viewport.
    Right,
}

/// A layout described by what it is for rather than by its tree.
///
/// A recipe survives a hand edit and a docking-library upgrade, which a
/// serialized tree does not, and that is why the built-ins are written this
/// way. The first six fields are the browser's, by name and by meaning.
///
/// The browser's recipe has two more, the side the viewport takes and the
/// scene tree tabbed behind the canvas. None of its presets uses either, so
/// neither is here: the viewport is on the left and the tree is a toggle.
/// The drift test compares both as constants, so it fails on the day a
/// preset starts using one, which is the day to add it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Recipe {
    pub properties_dock: PropertiesDock,
    /// The viewport's share of the width, in percent.
    pub split_pct: u8,
    /// The review panel, tabbed behind the parameter panel.
    pub review: bool,
    /// The attributes table, docked under the node canvas.
    pub attributes: bool,
    /// The attributes table's share of the dock's height, in percent.
    pub attributes_pct: u8,
    /// The texture viewer, tabbed behind the parameter panel.
    pub texture: bool,
}

const SPLIT_MIN_PCT: u8 = 20;
const SPLIT_MAX_PCT: u8 = 80;
const ATTRIBUTES_MIN_PCT: u8 = 15;
const ATTRIBUTES_MAX_PCT: u8 = 50;

/// The most of the canvas column the attributes table may take, so a
/// recipe that asks for a tall table under a half-height canvas still
/// leaves the canvas visible.
const ATTRIBUTES_MAX_OF_COLUMN: f32 = 0.8;

impl Recipe {
    /// The recipe every built-in is written against.
    const BASE: Self = Self {
        properties_dock: PropertiesDock::Bottom,
        split_pct: 55,
        review: false,
        attributes: false,
        attributes_pct: 30,
        texture: false,
    };

    /// The same recipe with its two percentages inside the ranges the
    /// browser clamps to, so a recipe means one thing in both shells.
    #[must_use]
    pub(crate) fn sanitized(self) -> Self {
        Self {
            split_pct: self.split_pct.clamp(SPLIT_MIN_PCT, SPLIT_MAX_PCT),
            attributes_pct: self
                .attributes_pct
                .clamp(ATTRIBUTES_MIN_PCT, ATTRIBUTES_MAX_PCT),
            ..self
        }
    }

    /// The dock tree this recipe describes.
    ///
    /// The viewport is always first, so it owns the root, and it is on the
    /// left. The node canvas takes the rest of the width. The parameter
    /// panel goes under the canvas or into a column of its own on the far
    /// side, which halves what the canvas had, as the browser's docking
    /// does.
    #[must_use]
    pub(crate) fn build(self) -> DockState<SolarxyTab> {
        let recipe = self.sanitized();
        let viewport_share = f32::from(recipe.split_pct) / 100.0;

        let mut properties_tabs = vec![SolarxyTab::Properties];
        if recipe.review {
            properties_tabs.push(SolarxyTab::ReviewPanel);
        }
        if recipe.texture {
            properties_tabs.push(SolarxyTab::Texture);
        }

        let mut state = DockState::new(vec![SolarxyTab::Viewport]);
        let surface = state.main_surface_mut();

        // A split's fraction is the share of the left or upper half.
        let [_, rest] =
            surface.split_right(NodeIndex::root(), viewport_share, vec![SolarxyTab::Nodes]);
        let [canvas, _] = match recipe.properties_dock {
            PropertiesDock::Bottom => surface.split_below(rest, 0.5, properties_tabs),
            PropertiesDock::Right => surface.split_right(rest, 0.5, properties_tabs),
        };

        if recipe.attributes {
            surface.split_below(
                canvas,
                1.0 - recipe.attributes_share_of_canvas_column(),
                vec![SolarxyTab::Attributes],
            );
        }

        state
    }

    /// The attributes table's share of the canvas's own column.
    ///
    /// The recipe states it as a share of the whole dock's height. With the
    /// parameter panel under the canvas, the canvas column is half of that
    /// height, so the same table takes twice the share of it.
    fn attributes_share_of_canvas_column(self) -> f32 {
        let of_dock = f32::from(self.attributes_pct) / 100.0;
        let column = match self.properties_dock {
            PropertiesDock::Bottom => 0.5,
            PropertiesDock::Right => 1.0,
        };
        (of_dock / column).min(ATTRIBUTES_MAX_OF_COLUMN)
    }
}

/// One named arrangement: a layout, three canvas reading preferences and
/// how the viewport is split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Arrangement {
    pub name: &'static str,
    pub recipe: Recipe,
    pub grid: bool,
    pub minimap: bool,
    pub controls: bool,
    pub view_layout: ViewLayout,
}

impl Arrangement {
    /// Write this arrangement's canvas preferences. Snapping and the wire
    /// routing are not an arrangement's to set, in either shell.
    pub(crate) fn apply_chrome(&self, canvas: &mut CanvasPrefs) {
        canvas.grid = self.grid;
        canvas.minimap = self.minimap;
        canvas.controls = self.controls;
    }
}

/// The built-in arrangements, in the order both shells list them.
pub(crate) const BUILT_IN: &[Arrangement] = &[
    Arrangement {
        name: "Default",
        recipe: Recipe::BASE,
        grid: true,
        minimap: false,
        controls: true,
        view_layout: ViewLayout::Single,
    },
    Arrangement {
        name: "Modeling",
        recipe: Recipe {
            properties_dock: PropertiesDock::Right,
            split_pct: 50,
            ..Recipe::BASE
        },
        grid: true,
        minimap: false,
        controls: true,
        view_layout: ViewLayout::Single,
    },
    Arrangement {
        name: "Review",
        recipe: Recipe {
            split_pct: 70,
            review: true,
            ..Recipe::BASE
        },
        grid: false,
        minimap: false,
        controls: false,
        view_layout: ViewLayout::Quad,
    },
    // A small viewport, a dominant node canvas with the attributes table
    // under it, and the parameter panel as a full-height column.
    Arrangement {
        name: "Technical",
        recipe: Recipe {
            properties_dock: PropertiesDock::Right,
            split_pct: 35,
            attributes: true,
            attributes_pct: 30,
            ..Recipe::BASE
        },
        grid: true,
        minimap: true,
        controls: true,
        view_layout: ViewLayout::Single,
    },
    // The viewport carries the layout, the texture viewer tabs with the
    // parameter panel, and the canvas chrome stays out of the way.
    Arrangement {
        name: "LookDev",
        recipe: Recipe {
            properties_dock: PropertiesDock::Right,
            split_pct: 70,
            texture: true,
            ..Recipe::BASE
        },
        grid: false,
        minimap: false,
        controls: false,
        view_layout: ViewLayout::Single,
    },
    // A split viewport with the texture viewer at hand. An arrangement
    // splits the panes; which pane shows the image layout is the pane's own
    // display setting.
    Arrangement {
        name: "UV / Texturing",
        recipe: Recipe {
            split_pct: 60,
            texture: true,
            ..Recipe::BASE
        },
        grid: false,
        minimap: false,
        controls: true,
        view_layout: ViewLayout::SplitVertical,
    },
];

/// Which arrangement an intent names. An index rather than a name, so the
/// intent stays `Copy` and a name typed by a user never travels in one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrangementId {
    BuiltIn(usize),
    /// An index into the user's saved arrangements, in stored order.
    User(usize),
}

/// What an id means once the user's own arrangements are consulted.
#[derive(Debug, PartialEq)]
pub(crate) enum Resolved<'a> {
    BuiltIn(&'static Arrangement),
    User(&'a UserArrangement),
}

impl ArrangementId {
    /// **A user's arrangement takes the place of a built-in of the same
    /// name**, as it does in the browser: someone who names theirs after a
    /// built-in meant to replace it, so the built-in's own row applies
    /// theirs too.
    pub(crate) fn resolve(self, users: &[UserArrangement]) -> Option<Resolved<'_>> {
        match self {
            Self::BuiltIn(index) => {
                let built_in = BUILT_IN.get(index)?;
                Some(
                    users
                        .iter()
                        .find(|user| user.name == built_in.name)
                        .map_or(Resolved::BuiltIn(built_in), Resolved::User),
                )
            }
            Self::User(index) => users.get(index).map(Resolved::User),
        }
    }
}

/// The name an arrangement is saved under, or `None` when what was typed
/// names nothing. Trimmed, as the browser trims it, so a name cannot differ
/// from another only by a space nobody can see.
pub(crate) fn saved_name(typed: &str) -> Option<String> {
    let name = typed.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Save `arrangement`, replacing one of the same name. The replaced entry
/// leaves its place and the new one goes last, which is the browser's
/// order and keeps the newest save at the end of the list.
pub(crate) fn upsert(users: &mut Vec<UserArrangement>, arrangement: UserArrangement) {
    users.retain(|existing| existing.name != arrangement.name);
    users.push(arrangement);
}

/// Write a saved arrangement's canvas preferences, the same three a
/// built-in writes.
pub(crate) fn apply_user_chrome(user: &UserArrangement, canvas: &mut CanvasPrefs) {
    canvas.grid = user.grid;
    canvas.minimap = user.minimap;
    canvas.controls = user.controls;
}

/// The name a layout kept through the withdrawn Save Layout entry arrives
/// under.
pub(crate) const CARRIED_LAYOUT_NAME: &str = "Saved Layout";

/// Carry a layout kept through the withdrawn layout menu into a user
/// arrangement, once. Answers whether the preferences changed, which is
/// the caller's cue to write them.
///
/// That menu wrote one slot and read it back, and with the menu gone the
/// slot would sit in the file with nothing able to reach it. A saved
/// arrangement is what took the menu's job, so that is where it goes. It
/// rides with the canvas preferences and the pane split the caller names,
/// because the slot never recorded either.
///
/// Emptying the slot is what makes this happen once: an arrangement the
/// user later deletes would otherwise come back on the next launch. So the
/// slot is emptied only when the layout was carried, and two cases leave it
/// exactly as it was, so nothing the file held is lost. A layout that can no
/// longer be restored is skipped without a word, since an arrangement that
/// cannot be applied is worse than none. And an arrangement the user already
/// saved under this name is theirs, and is not replaced.
pub(crate) fn carry_saved_layout(
    dock: &mut DockPrefs,
    canvas: CanvasPrefs,
    view_layout: ViewLayout,
) -> bool {
    let restorable = dock
        .saved_layout_json
        .as_deref()
        .is_some_and(|json| super::dock::restore(json).is_ok());
    let name_taken = dock
        .arrangements
        .iter()
        .any(|existing| existing.name == CARRIED_LAYOUT_NAME);
    if !restorable || name_taken {
        return false;
    }
    let Some(layout_json) = dock.saved_layout_json.take() else {
        return false;
    };
    dock.arrangements.push(UserArrangement {
        name: CARRIED_LAYOUT_NAME.to_string(),
        layout_json,
        grid: canvas.grid,
        minimap: canvas.minimap,
        controls: canvas.controls,
        view_layout,
    });
    true
}

/// The arrangement a new installation opens in, and the one a layout that
/// cannot be restored falls back to.
pub(crate) fn default_arrangement() -> &'static Arrangement {
    &BUILT_IN[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn membership(dock: &DockState<SolarxyTab>) -> HashSet<SolarxyTab> {
        dock.iter_all_tabs().map(|(_, tab)| *tab).collect()
    }

    fn named(name: &str) -> &'static Arrangement {
        BUILT_IN
            .iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("no built-in named {name}"))
    }

    /// The leaf a tab sits in, as the tabs it shares that leaf with.
    fn leaf_of(dock: &DockState<SolarxyTab>, tab: SolarxyTab) -> Vec<SolarxyTab> {
        let (surface, node, _) = dock.find_tab(&tab).expect("mounted");
        dock[surface][node].get_leaf().expect("a leaf").tabs.clone()
    }

    #[test]
    fn every_built_in_mounts_exactly_what_its_recipe_names() {
        for arrangement in BUILT_IN {
            let recipe = arrangement.recipe;
            let mut expected = HashSet::from([
                SolarxyTab::Viewport,
                SolarxyTab::Nodes,
                SolarxyTab::Properties,
            ]);
            for (on, tab) in [
                (recipe.review, SolarxyTab::ReviewPanel),
                (recipe.attributes, SolarxyTab::Attributes),
                (recipe.texture, SolarxyTab::Texture),
            ] {
                if on {
                    expected.insert(tab);
                }
            }
            assert_eq!(
                membership(&recipe.build()),
                expected,
                "{} mounts the wrong panels",
                arrangement.name
            );
        }
    }

    /// The auxiliary panels tab behind the parameter panel, and it stays in
    /// front, because it is the tab that names the group.
    #[test]
    fn auxiliary_panels_tab_behind_the_parameter_panel() {
        let review = named("Review").recipe.build();
        assert_eq!(
            leaf_of(&review, SolarxyTab::ReviewPanel),
            [SolarxyTab::Properties, SolarxyTab::ReviewPanel]
        );
        let lookdev = named("LookDev").recipe.build();
        assert_eq!(
            leaf_of(&lookdev, SolarxyTab::Texture),
            [SolarxyTab::Properties, SolarxyTab::Texture]
        );
    }

    /// The viewport's leaf holds the viewport and nothing else in every
    /// built-in: a panel tabbed over it would cover the scene.
    #[test]
    fn nothing_shares_the_viewport_leaf() {
        for arrangement in BUILT_IN {
            let dock = arrangement.recipe.build();
            assert_eq!(
                leaf_of(&dock, SolarxyTab::Viewport),
                [SolarxyTab::Viewport],
                "{}",
                arrangement.name
            );
        }
    }

    #[test]
    fn a_recipe_is_clamped_to_the_ranges_the_browser_clamps_to() {
        let wild = Recipe {
            split_pct: 99,
            attributes_pct: 3,
            ..Recipe::BASE
        }
        .sanitized();
        assert_eq!((wild.split_pct, wild.attributes_pct), (80, 15));
        let narrow = Recipe {
            split_pct: 1,
            attributes_pct: 90,
            ..Recipe::BASE
        }
        .sanitized();
        assert_eq!((narrow.split_pct, narrow.attributes_pct), (20, 50));
    }

    /// A table under a half-height canvas takes twice the share of that
    /// column, and never so much that the canvas disappears.
    #[test]
    fn the_attributes_share_follows_where_the_parameter_panel_sits() {
        let column = Recipe {
            properties_dock: PropertiesDock::Right,
            attributes_pct: 30,
            ..Recipe::BASE
        };
        assert!((column.attributes_share_of_canvas_column() - 0.3).abs() < 1e-6);
        let under = Recipe {
            attributes_pct: 30,
            ..Recipe::BASE
        };
        assert!((under.attributes_share_of_canvas_column() - 0.6).abs() < 1e-6);
        let tall = Recipe {
            attributes_pct: 50,
            ..Recipe::BASE
        };
        assert!((tall.attributes_share_of_canvas_column() - 0.8).abs() < 1e-6);
    }

    /// Applying an arrangement writes three canvas preferences and leaves
    /// the two that are not an arrangement's to set.
    #[test]
    fn chrome_writes_three_preferences_and_no_others() {
        let mut canvas = CanvasPrefs {
            snap: true,
            ..CanvasPrefs::default()
        };
        let routing = canvas.routing;
        named("Review").apply_chrome(&mut canvas);
        assert!(!canvas.grid && !canvas.minimap && !canvas.controls);
        assert!(canvas.snap, "snapping is not an arrangement's to set");
        assert_eq!(canvas.routing, routing);

        named("Technical").apply_chrome(&mut canvas);
        assert!(canvas.grid && canvas.minimap && canvas.controls);
    }

    fn user(name: &str) -> UserArrangement {
        UserArrangement {
            name: name.to_string(),
            layout_json: format!("{{\"for\":\"{name}\"}}"),
            grid: false,
            minimap: true,
            controls: false,
            view_layout: ViewLayout::Quad,
        }
    }

    /// A layout written by 0.8.1, which is what a slot kept by hand holds.
    const KEPT_BY_HAND: &str = include_str!("../../tests/fixtures/dock-layout-0.8.1.json");

    fn kept(json: &str) -> DockPrefs {
        DockPrefs {
            saved_layout_json: Some(json.to_string()),
            ..DockPrefs::default()
        }
    }

    /// Every value away from its default, so one that was written in rather
    /// than read from the caller shows.
    fn carried_canvas() -> CanvasPrefs {
        CanvasPrefs {
            grid: false,
            minimap: true,
            controls: false,
            ..CanvasPrefs::default()
        }
    }

    /// The layout arrives under its name with the bytes it was kept as, and
    /// the slot is emptied, which is what stops it arriving twice: not on
    /// the next launch, and not after the user deletes it.
    #[test]
    fn a_layout_kept_by_hand_arrives_as_an_arrangement_once() {
        let mut dock = kept(KEPT_BY_HAND);
        assert!(carry_saved_layout(
            &mut dock,
            carried_canvas(),
            ViewLayout::Quad
        ));
        assert_eq!(dock.saved_layout_json, None, "the slot is emptied");
        assert_eq!(
            dock.arrangements,
            vec![UserArrangement {
                name: CARRIED_LAYOUT_NAME.to_string(),
                layout_json: KEPT_BY_HAND.to_string(),
                grid: false,
                minimap: true,
                controls: false,
                view_layout: ViewLayout::Quad,
            }]
        );
        assert!(
            crate::gui::dock::restore(&dock.arrangements[0].layout_json).is_ok(),
            "what was carried can be applied"
        );

        assert!(!carry_saved_layout(
            &mut dock,
            carried_canvas(),
            ViewLayout::Quad
        ));
        assert_eq!(dock.arrangements.len(), 1, "a second launch adds nothing");

        dock.arrangements.clear();
        assert!(!carry_saved_layout(
            &mut dock,
            carried_canvas(),
            ViewLayout::Quad
        ));
        assert!(dock.arrangements.is_empty(), "a deleted one stays deleted");
    }

    /// Text that is not a layout, and a layout whose every panel has since
    /// been retired, are both passed over, and the slot keeps what it held.
    #[test]
    fn a_kept_layout_that_cannot_be_restored_is_left_where_it_was() {
        let nothing_left = KEPT_BY_HAND.replace("\"Viewport\"", "\"Bogus\"");
        assert_ne!(nothing_left, KEPT_BY_HAND, "the fixture names Viewport");
        for unusable in ["not a layout", nothing_left.as_str()] {
            let mut dock = kept(unusable);
            assert!(!carry_saved_layout(
                &mut dock,
                carried_canvas(),
                ViewLayout::Single
            ));
            assert!(dock.arrangements.is_empty());
            assert_eq!(dock.saved_layout_json.as_deref(), Some(unusable));
        }
    }

    /// An arrangement the user saved under the same name is theirs. It is
    /// kept as it was, and so is the slot, so neither is lost to the other.
    #[test]
    fn an_arrangement_the_user_named_the_same_is_not_replaced() {
        let mut dock = kept(KEPT_BY_HAND);
        dock.arrangements.push(user(CARRIED_LAYOUT_NAME));
        assert!(!carry_saved_layout(
            &mut dock,
            carried_canvas(),
            ViewLayout::Single
        ));
        assert_eq!(dock.arrangements, vec![user(CARRIED_LAYOUT_NAME)]);
        assert_eq!(dock.saved_layout_json.as_deref(), Some(KEPT_BY_HAND));
    }

    #[test]
    fn an_empty_slot_carries_nothing() {
        let mut dock = DockPrefs::default();
        assert!(!carry_saved_layout(
            &mut dock,
            carried_canvas(),
            ViewLayout::Single
        ));
        assert_eq!(dock, DockPrefs::default());
    }

    #[test]
    fn an_id_out_of_range_resolves_to_nothing() {
        assert_eq!(
            ArrangementId::BuiltIn(0).resolve(&[]),
            Some(Resolved::BuiltIn(&BUILT_IN[0]))
        );
        assert!(
            ArrangementId::BuiltIn(BUILT_IN.len())
                .resolve(&[])
                .is_none()
        );
        assert!(ArrangementId::User(0).resolve(&[]).is_none());
    }

    /// A user arrangement named after a built-in is what that built-in's
    /// own row applies, and every other built-in is untouched by it.
    #[test]
    fn a_user_arrangement_takes_the_place_of_a_built_in_of_its_name() {
        let users = [user("Mine"), user("Review")];
        let review = BUILT_IN
            .iter()
            .position(|a| a.name == "Review")
            .expect("a built-in named Review");
        assert_eq!(
            ArrangementId::BuiltIn(review).resolve(&users),
            Some(Resolved::User(&users[1]))
        );
        assert_eq!(
            ArrangementId::BuiltIn(0).resolve(&users),
            Some(Resolved::BuiltIn(&BUILT_IN[0]))
        );
        assert_eq!(
            ArrangementId::User(0).resolve(&users),
            Some(Resolved::User(&users[0]))
        );
    }

    #[test]
    fn a_name_is_trimmed_and_an_empty_one_is_no_name() {
        assert_eq!(saved_name("  Sculpt  ").as_deref(), Some("Sculpt"));
        assert_eq!(
            saved_name("UV / Texturing").as_deref(),
            Some("UV / Texturing")
        );
        assert_eq!(saved_name(""), None);
        assert_eq!(saved_name("   \t "), None);
    }

    /// Saving under a name already in use replaces that arrangement rather
    /// than listing two, and the replacement goes last.
    #[test]
    fn saving_under_a_used_name_replaces_it() {
        let mut users = vec![user("A"), user("B"), user("C")];
        let mut again = user("A");
        again.layout_json = "newer".to_string();
        upsert(&mut users, again);
        let names: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["B", "C", "A"]);
        assert_eq!(users[2].layout_json, "newer");

        upsert(&mut users, user("D"));
        assert_eq!(users.len(), 4);
    }

    #[test]
    fn a_saved_arrangement_writes_the_same_three_preferences() {
        let mut canvas = CanvasPrefs {
            snap: true,
            ..CanvasPrefs::default()
        };
        apply_user_chrome(&user("Mine"), &mut canvas);
        assert!(!canvas.grid && canvas.minimap && !canvas.controls);
        assert!(canvas.snap, "snapping is not an arrangement's to set");
    }

    // ---- held against the browser's table ----

    /// One preset as `web/src/store/desks.ts` declares it.
    #[derive(Debug, PartialEq, Eq)]
    struct Declared {
        name: String,
        viewport_side: String,
        properties_dock: String,
        split_pct: u8,
        review: bool,
        attributes: bool,
        attributes_pct: u8,
        texture: bool,
        tree: bool,
        grid: bool,
        minimap: bool,
        controls: bool,
        view_layout: String,
    }

    fn text_after<'a>(chunk: &'a str, key: &str) -> Option<&'a str> {
        let at = chunk.find(&format!("{key}: "))? + key.len() + 2;
        let rest = &chunk[at..];
        let end = rest.find([',', '\n', ' ', '}'])?;
        Some(rest[..end].trim_matches('"'))
    }

    fn flag(chunk: &str, key: &str) -> bool {
        text_after(chunk, key) == Some("true")
    }

    /// The browser's presets, read from its source. Comments are dropped
    /// first so prose cannot be mistaken for a field.
    fn browser_presets() -> Vec<Declared> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        let source = std::fs::read_to_string(root.join("web/src/store/desks.ts"))
            .expect("the browser's arrangement store");
        let code: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let start = code
            .find("export const DESK_PRESETS")
            .expect("the preset table");
        let block = &code[start..];
        let block = &block[..block.find("\n];").expect("the table's end")];

        block
            .split("name: \"")
            .skip(1)
            .map(|chunk| {
                let name = chunk[..chunk.find('"').expect("a closed name")].to_string();
                Declared {
                    name,
                    viewport_side: text_after(chunk, "viewportSide")
                        .expect("a side")
                        .to_string(),
                    properties_dock: text_after(chunk, "propertiesDock")
                        .expect("a dock")
                        .to_string(),
                    split_pct: text_after(chunk, "splitPct")
                        .and_then(|v| v.parse().ok())
                        .expect("a split"),
                    review: flag(chunk, "review"),
                    attributes: flag(chunk, "attributes"),
                    attributes_pct: text_after(chunk, "attributesPct")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(Recipe::BASE.attributes_pct),
                    texture: flag(chunk, "texture"),
                    tree: flag(chunk, "tree"),
                    grid: flag(chunk, "showFlowGrid"),
                    minimap: flag(chunk, "showMinimap"),
                    controls: flag(chunk, "showFlowControls"),
                    view_layout: text_after(chunk, "viewLayout")
                        .expect("a pane layout")
                        .to_string(),
                }
            })
            .collect()
    }

    fn as_declared(arrangement: &Arrangement) -> Declared {
        let recipe = arrangement.recipe;
        Declared {
            name: arrangement.name.to_string(),
            // Constants here: see `Recipe` for why neither is a field.
            viewport_side: "left".to_string(),
            properties_dock: match recipe.properties_dock {
                PropertiesDock::Bottom => "bottom",
                PropertiesDock::Right => "right",
            }
            .to_string(),
            split_pct: recipe.split_pct,
            review: recipe.review,
            attributes: recipe.attributes,
            attributes_pct: recipe.attributes_pct,
            texture: recipe.texture,
            tree: false,
            grid: arrangement.grid,
            minimap: arrangement.minimap,
            controls: arrangement.controls,
            view_layout: match arrangement.view_layout {
                ViewLayout::Single => "single",
                ViewLayout::SplitVertical => "splitVertical",
                ViewLayout::SplitHorizontal => "splitHorizontal",
                ViewLayout::Quad => "quad",
                ViewLayout::ThreeLeftBig => "threeLeftBig",
            }
            .to_string(),
        }
    }

    /// Both shells list the same arrangements, in the same order, meaning
    /// the same thing. The Sidebar is this shell's alone and is the one
    /// field not compared; it is held to the first arrangement only, by the
    /// test below.
    #[test]
    fn the_built_ins_are_the_browsers_presets() {
        let browser = browser_presets();
        assert!(
            browser.len() >= 6,
            "read {} presets from the browser, so the reader is broken",
            browser.len()
        );
        let here: Vec<Declared> = BUILT_IN.iter().map(as_declared).collect();
        assert_eq!(here, browser);
    }
}
