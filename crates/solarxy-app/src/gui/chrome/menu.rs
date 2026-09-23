//! The global menu bar: `File / Edit / Desks / Review / Help`, the browser's
//! five, with the same entries in the same order.
//!
//! There were eight until 0.10.0. Render, View and Layout acted on the
//! viewport, so their commands live on the viewport now: the per-pane menus
//! carry the shading, inspection, projection and display entries, and the
//! viewport's own View menu carries fitting, the pane layouts, the
//! environment and the screenshot. Window became the panel toggles in Desks.
//! What is left here is what belongs to the document and the application.
//!
//! **Each menu is a table the draw walks.** The entries and their order are
//! data, so a test reads the browser's own menu bar and holds this one to the
//! same entries in the same order, with every entry only this shell has
//! named in a list with its reason.
//!
//! **Every hint is read from the binding table** through the shared
//! vocabulary, never typed in. The menus this one replaced advertised eight
//! keys that had been retired, because a hint typed beside a label has
//! nothing holding it to the key it names.
//!
//! Everything that is an action rather than a setting is raised as an
//! [`Intent`] and applied after the pass.

use crate::gui::MOD;
use crate::gui::arrangement::{ArrangementId, BUILT_IN};
use crate::gui::dock::SolarxyTab;
use crate::gui::intent::{
    EditIntent, FileIntent, HelpIntent, Intent, Intents, LayoutIntent, ReviewIntent,
};
use crate::gui::settings::PanelSettings;
use crate::gui::theme::Theme;
use crate::state::keymap::{Action, hint};

use super::menu_items::{check_entry, entry, entry_if, waiting_entry};

/// What the menu bar needs to know about the shell to draw itself, as
/// against what it asks the shell to do, which travels as an [`Intent`].
#[derive(Clone, Copy)]
pub(in crate::gui) struct MenuContext<'a> {
    /// Whether a document is open, which is what saving needs.
    pub has_model: bool,
    pub recent_files: &'a [String],
    /// The names of the arrangements the user saved, in stored order.
    pub arrangements: &'a [String],
    pub review_available: bool,
    pub review_active: bool,
    pub review_markers_hidden: bool,
    pub review_dirty: bool,
    pub theme: Theme,
}

/// A row of one of the bar's menus: an entry, or the line between two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Item(Entry),
    Divider,
}

/// Every entry the global bar has, by what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    // File
    NewScene,
    OpenScene,
    SampleScenes,
    SaveScene,
    SaveSceneAs,
    ImportModel,
    RecentFiles,
    Quit,
    // Edit
    Undo,
    Redo,
    Copy,
    Paste,
    Duplicate,
    ToggleBypass,
    SetDisplayFlag,
    DeleteSelection,
    Preferences,
    // Review
    ReviewMode,
    ReviewPanel,
    ShowMarkers,
    SaveReviewNotes,
    // Help
    Shortcuts,
    Tour,
    Wiki,
    About,
}

impl Entry {
    const fn label(self) -> &'static str {
        match self {
            Self::NewScene => "New Scene",
            Self::OpenScene => "Open Scene\u{2026}",
            Self::SampleScenes => "Sample Scenes",
            Self::SaveScene => "Save Scene",
            Self::SaveSceneAs => "Save Scene As\u{2026}",
            Self::ImportModel => "Import Model\u{2026}",
            Self::RecentFiles => "Recent Files",
            Self::Quit => "Quit",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Duplicate => "Duplicate",
            Self::ToggleBypass => "Toggle Bypass",
            Self::SetDisplayFlag => "Set Display Flag",
            Self::DeleteSelection => "Delete Selection",
            Self::Preferences => "Preferences\u{2026}",
            Self::ReviewMode => "Review Mode",
            Self::ReviewPanel => "Review Panel",
            Self::ShowMarkers => "Show Markers",
            Self::SaveReviewNotes => "Save Review Notes",
            Self::Shortcuts => "Keyboard Shortcuts",
            Self::Tour => "Take a Tour",
            Self::Wiki => "Wiki",
            Self::About => "About Solarxy",
        }
    }

    /// The binding whose key the entry shows, where it has one.
    const fn action(self) -> Option<Action> {
        match self {
            Self::NewScene => Some(Action::NewScene),
            Self::OpenScene => Some(Action::OpenScene),
            Self::SaveScene => Some(Action::Save),
            Self::SaveSceneAs => Some(Action::SaveAs),
            Self::Undo => Some(Action::Undo),
            Self::Redo => Some(Action::Redo),
            Self::Copy => Some(Action::Copy),
            Self::Paste => Some(Action::Paste),
            Self::Duplicate => Some(Action::Duplicate),
            Self::ToggleBypass => Some(Action::Bypass),
            Self::SetDisplayFlag => Some(Action::DisplayFlag),
            Self::Preferences => Some(Action::OpenPreferences),
            Self::ReviewMode => Some(Action::ToggleReviewMode),
            Self::ReviewPanel => Some(Action::ToggleReviewPanel),
            Self::Shortcuts => Some(Action::ShowShortcuts),
            Self::SampleScenes
            | Self::ImportModel
            | Self::RecentFiles
            | Self::Quit
            | Self::DeleteSelection
            | Self::ShowMarkers
            | Self::SaveReviewNotes
            | Self::Tour
            | Self::Wiki
            | Self::About => None,
        }
    }
}

use Row::{Divider, Item};

const FILE_MENU: &[Row] = &[
    Item(Entry::NewScene),
    Item(Entry::OpenScene),
    Item(Entry::SampleScenes),
    Item(Entry::SaveScene),
    Item(Entry::SaveSceneAs),
    Divider,
    Item(Entry::ImportModel),
    Divider,
    Item(Entry::RecentFiles),
    Item(Entry::Quit),
];

const EDIT_MENU: &[Row] = &[
    Item(Entry::Undo),
    Item(Entry::Redo),
    Divider,
    Item(Entry::Copy),
    Item(Entry::Paste),
    Item(Entry::Duplicate),
    Divider,
    Item(Entry::ToggleBypass),
    Item(Entry::SetDisplayFlag),
    Item(Entry::DeleteSelection),
    Divider,
    Item(Entry::Preferences),
];

const REVIEW_MENU: &[Row] = &[
    Item(Entry::ReviewMode),
    Item(Entry::ReviewPanel),
    Item(Entry::ShowMarkers),
    Divider,
    Item(Entry::SaveReviewNotes),
];

const HELP_MENU: &[Row] = &[
    Item(Entry::Shortcuts),
    Item(Entry::Tour),
    Item(Entry::Wiki),
    Divider,
    Item(Entry::About),
];

/// The bar's five menus, by the browser's names and in its order.
const MENU_TITLES: [&str; 5] = ["File", "Edit", "Desks", "Review", "Help"];

const NO_DOCUMENT: &str = "Open or start a scene first";
const NO_SELECTION: &str = "Select a node first";
const NO_REVIEW: &str = "Review is not available on this document yet";

pub(in crate::gui) fn draw_menu_bar(
    ctx: &egui::Context,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
    present: &dyn Fn(SolarxyTab) -> bool,
    cx: MenuContext<'_>,
) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            for (title, rows) in [(MENU_TITLES[0], FILE_MENU), (MENU_TITLES[1], EDIT_MENU)] {
                ui.menu_button(title, |ui| {
                    draw_rows(ui, rows, settings, intents, present, cx);
                });
            }
            draw_desks_menu(ui, intents, present, cx);
            // Review is a viewport mode. The title turns to the accent
            // colour while it is on, so the mode is visible with the menu
            // closed.
            let review: egui::WidgetText = if cx.review_active {
                egui::RichText::new(format!("\u{25CF} {}", MENU_TITLES[3]))
                    .color(cx.theme.accent)
                    .into()
            } else {
                MENU_TITLES[3].into()
            };
            ui.menu_button(review, |ui| {
                draw_rows(ui, REVIEW_MENU, settings, intents, present, cx);
            });
            ui.menu_button(MENU_TITLES[4], |ui| {
                draw_rows(ui, HELP_MENU, settings, intents, present, cx);
            });
            draw_history_strip(ui, settings.history, intents);
            draw_cook_strip(ui, settings.cook, intents);
        });
    });
}

fn draw_rows(
    ui: &mut egui::Ui,
    rows: &[Row],
    settings: PanelSettings<'_>,
    intents: &mut Intents,
    present: &dyn Fn(SolarxyTab) -> bool,
    cx: MenuContext<'_>,
) {
    for row in rows {
        match row {
            Divider => {
                ui.separator();
            }
            Item(item) => draw_entry(ui, *item, settings, intents, present, cx),
        }
    }
}

/// Draw one entry and raise what it does.
#[allow(clippy::too_many_lines)]
fn draw_entry(
    ui: &mut egui::Ui,
    item: Entry,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
    present: &dyn Fn(SolarxyTab) -> bool,
    cx: MenuContext<'_>,
) {
    let (label, action) = (item.label(), item.action());
    let selection = settings.clipboard;
    // What a plain entry raises, and whether it applies. The few entries
    // that are a submenu, a tick or a wait are drawn in their own arms.
    let plain: Option<(bool, &str, Intent)> = match item {
        Entry::NewScene => Some((true, "", Intent::File(FileIntent::NewScene))),
        Entry::OpenScene => Some((true, "", Intent::File(FileIntent::OpenModel))),
        Entry::SaveScene => Some((cx.has_model, NO_DOCUMENT, Intent::File(FileIntent::Save))),
        Entry::SaveSceneAs => Some((cx.has_model, NO_DOCUMENT, Intent::File(FileIntent::SaveAs))),
        Entry::ImportModel => Some((true, "", Intent::File(FileIntent::ImportModel))),
        Entry::Quit => Some((true, "", Intent::File(FileIntent::Quit))),
        Entry::Undo => Some((
            settings.history.can_undo,
            "Nothing to undo",
            Intent::Edit(EditIntent::Undo),
        )),
        Entry::Redo => Some((
            settings.history.can_redo,
            "Nothing to redo",
            Intent::Edit(EditIntent::Redo),
        )),
        Entry::Copy => Some((
            selection.has_selection,
            NO_SELECTION,
            Intent::Edit(EditIntent::Copy),
        )),
        Entry::Paste => Some((
            selection.has_clipboard,
            "Nothing has been copied",
            Intent::Edit(EditIntent::Paste),
        )),
        Entry::Duplicate => Some((
            selection.has_selection,
            NO_SELECTION,
            Intent::Edit(EditIntent::Duplicate),
        )),
        Entry::ToggleBypass => Some((
            selection.has_selection,
            NO_SELECTION,
            Intent::Edit(EditIntent::ToggleBypass),
        )),
        Entry::SetDisplayFlag => Some((
            selection.has_selection && selection.in_container,
            if selection.has_selection {
                "A display flag is set inside a network; at the top level every object shows"
            } else {
                NO_SELECTION
            },
            Intent::Edit(EditIntent::SetDisplayFlag),
        )),
        Entry::DeleteSelection => Some((
            selection.has_selection,
            NO_SELECTION,
            Intent::Edit(EditIntent::DeleteSelection),
        )),
        Entry::Preferences => Some((true, "", Intent::Edit(EditIntent::OpenPreferences))),
        Entry::SaveReviewNotes => Some((
            cx.review_dirty,
            "There are no unsaved review notes",
            Intent::Review(ReviewIntent::SaveNotes),
        )),
        Entry::Shortcuts => Some((true, "", Intent::Help(HelpIntent::Shortcuts))),
        Entry::Wiki => Some((true, "", Intent::Help(HelpIntent::Wiki))),
        Entry::About => Some((true, "", Intent::Help(HelpIntent::About))),
        Entry::SampleScenes
        | Entry::RecentFiles
        | Entry::ReviewMode
        | Entry::ReviewPanel
        | Entry::ShowMarkers
        | Entry::Tour => None,
    };
    if let Some((enabled, why_not, intent)) = plain {
        if entry_if(ui, enabled, label, action, why_not).clicked() {
            intents.raise(intent);
            ui.close();
        }
        return;
    }

    match item {
        Entry::SampleScenes => {
            ui.menu_button(label, |ui| {
                for (index, sample) in crate::state::samples::SAMPLES.iter().enumerate() {
                    if entry(ui, sample.label, None).clicked() {
                        intents.raise(Intent::File(FileIntent::OpenSample(index)));
                        ui.close();
                    }
                }
            });
        }
        Entry::RecentFiles => draw_recent_files(ui, label, cx.recent_files, intents),
        // The mode toggle stays enabled while it is on, so it can always be
        // turned off.
        Entry::ReviewMode => {
            if ui
                .add_enabled(
                    cx.review_available || cx.review_active,
                    egui::Button::new(label)
                        .selected(cx.review_active)
                        .shortcut_text(action.and_then(hint).unwrap_or_default()),
                )
                .on_disabled_hover_text(NO_REVIEW)
                .clicked()
            {
                intents.raise(Intent::Review(ReviewIntent::ToggleMode));
                ui.close();
            }
        }
        Entry::ReviewPanel => {
            if check_entry(ui, present(SolarxyTab::ReviewPanel), label, action).clicked() {
                intents.raise(Intent::Layout(LayoutIntent::ToggleTab(
                    SolarxyTab::ReviewPanel,
                )));
                ui.close();
            }
        }
        Entry::ShowMarkers => {
            if ui
                .add_enabled(
                    cx.review_available,
                    egui::Button::new(label).selected(!cx.review_markers_hidden),
                )
                .on_disabled_hover_text(NO_REVIEW)
                .clicked()
            {
                intents.raise(Intent::Review(ReviewIntent::ToggleMarkers));
                ui.close();
            }
        }
        Entry::Tour => {
            waiting_entry(ui, label, "The guided tour has not come to this shell yet");
        }
        _ => {}
    }
}

/// The documents opened lately, newest first. Absent while there are none,
/// rather than an empty submenu.
fn draw_recent_files(
    ui: &mut egui::Ui,
    label: &str,
    recent_files: &[String],
    intents: &mut Intents,
) {
    ui.add_enabled_ui(!recent_files.is_empty(), |ui| {
        ui.menu_button(label, |ui| {
            for path in recent_files.iter().take(10) {
                let raw = std::path::Path::new(path)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(path);
                let count = raw.chars().count();
                let shown: String = if count > 50 {
                    let tail: String = raw.chars().skip(count - 47).collect();
                    format!("\u{2026}{tail}")
                } else {
                    raw.to_string()
                };
                if ui.button(&shown).on_hover_text(path).clicked() {
                    intents.raise(Intent::File(FileIntent::OpenRecent(path.clone())));
                    ui.close();
                }
            }
        })
        .response
        .on_disabled_hover_text("Nothing has been opened yet");
    });
}

/// The arrangements menu: the named arrangements, then the panel toggles.
///
/// Nothing here saves, restores or resets a layout by hand. A saved
/// arrangement is how a layout is kept, the Default one is the way back from
/// a layout gone wrong, and what was on screen at quit comes back on launch
/// without being asked for.
fn draw_desks_menu(
    ui: &mut egui::Ui,
    intents: &mut Intents,
    present: &dyn Fn(SolarxyTab) -> bool,
    cx: MenuContext<'_>,
) {
    ui.menu_button(MENU_TITLES[2], |ui| {
        draw_arrangement_entries(ui, intents, cx.arrangements);
        ui.separator();
        // Presence in the dock is the open state, so a tick cannot disagree
        // with what is on screen.
        for (tab, label) in PANEL_TOGGLES {
            if check_entry(ui, present(*tab), label, None).clicked() {
                intents.raise(Intent::Layout(LayoutIntent::ToggleTab(*tab)));
                ui.close();
            }
        }
    });
}

/// The cook strip at the right end of the bar: the mode toggle, and in manual
/// mode the stale count and the Cook button. It lives here because the header
/// is where the browser keeps the same three controls, and the status bar that
/// once might have held it was withdrawn in 0.10.0.
///
/// Widgets are added right to left, so the first one added is the rightmost.
fn draw_cook_strip(ui: &mut egui::Ui, cook: crate::gui::CookReadout, intents: &mut Intents) {
    use solarxy_graph::engine::CookMode;

    if !cook.open {
        return;
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let manual = cook.mode == CookMode::Manual;
        if manual
            && ui
                .add_enabled(cook.can_cook(), egui::Button::new("Cook"))
                .on_hover_text(format!(
                    "Cook the stale nodes now ({}+Enter)",
                    crate::gui::MOD
                ))
                .on_disabled_hover_text("Nothing is stale, or a cook is already working")
                .clicked()
        {
            intents.raise(Intent::Cook(crate::gui::CookIntent::CookNow));
        }
        if let Some(label) = cook.status_label() {
            ui.label(egui::RichText::new(label).small());
        }
        let toggle = egui::Button::new(if manual { "Manual" } else { "Auto" }).selected(manual);
        if ui
            .add(toggle)
            .on_hover_text(
                "Cook mode. Manual holds the cook until you ask for it, which is \
                 how a heavy graph stays editable.",
            )
            .clicked()
        {
            let next = if manual {
                CookMode::Auto
            } else {
                CookMode::Manual
            };
            intents.raise(Intent::Cook(crate::gui::CookIntent::SetMode(next)));
        }
    });
}

/// The undo and redo buttons beside the cook strip: the header controls
/// the browser carries, enabled from the engine's depths.
fn draw_history_strip(
    ui: &mut egui::Ui,
    history: crate::gui::HistoryReadout,
    intents: &mut Intents,
) {
    if !history.open {
        return;
    }
    if ui
        .add_enabled(history.can_undo, egui::Button::new("Undo"))
        .on_hover_text(format!("Undo ({MOD}+Z)"))
        .clicked()
    {
        intents.raise(Intent::Edit(EditIntent::Undo));
    }
    if ui
        .add_enabled(history.can_redo, egui::Button::new("Redo"))
        .on_hover_text(format!("Redo ({MOD}+Shift+Z)"))
        .clicked()
    {
        intents.raise(Intent::Edit(EditIntent::Redo));
    }
}

/// The two arrangement entries' labels, as the browser spells them.
const SAVE_CURRENT_AS: &str = "Save Current As\u{2026}";
const DELETE_DESK: &str = "Delete Desk";

/// The named arrangements, in the browser's order: the built-ins, the
/// user's own under a divider when there are any, then saving and deleting.
fn draw_arrangement_entries(ui: &mut egui::Ui, intents: &mut Intents, arrangements: &[String]) {
    for (index, arrangement) in BUILT_IN.iter().enumerate() {
        if ui.button(arrangement.name).clicked() {
            intents.raise(Intent::Layout(LayoutIntent::ApplyArrangement(
                ArrangementId::BuiltIn(index),
            )));
            ui.close();
        }
    }
    if !arrangements.is_empty() {
        ui.separator();
        for (index, name) in arrangements.iter().enumerate() {
            if ui.button(name).clicked() {
                intents.raise(Intent::Layout(LayoutIntent::ApplyArrangement(
                    ArrangementId::User(index),
                )));
                ui.close();
            }
        }
    }
    ui.separator();
    if ui.button(SAVE_CURRENT_AS).clicked() {
        intents.raise(Intent::Layout(LayoutIntent::OpenArrangementSave));
        ui.close();
    }
    ui.add_enabled_ui(!arrangements.is_empty(), |ui| {
        ui.menu_button(DELETE_DESK, |ui| {
            for (index, name) in arrangements.iter().enumerate() {
                if ui.button(name).clicked() {
                    intents.raise(Intent::Layout(LayoutIntent::DeleteArrangement(index)));
                    ui.close();
                }
            }
        })
        .response
        .on_disabled_hover_text("You have saved no arrangement yet");
    });
}

/// The panel toggles, in the browser's order and under its labels.
///
/// **This table is the registration a new panel needs.** Before it, every
/// panel cost a field on a shared visibility struct, a line building that
/// struct, a line in a diff table, and a line applying the diff. Now it costs
/// a row, and the tick is read from the dock rather than mirrored, so a panel
/// closed by its own tab button unticks itself.
///
/// The viewport is not here, as it is not in the browser: it is the one
/// panel that cannot be closed. The asset preview is not either, since it
/// opens from an asset rather than from a menu.
const PANEL_TOGGLES: &[(SolarxyTab, &str)] = &[
    (SolarxyTab::Nodes, "Nodes Panel"),
    (SolarxyTab::Properties, "Properties Panel"),
    (SolarxyTab::Tree, "Tree Panel"),
    (SolarxyTab::Text, "Text Panel"),
    (SolarxyTab::Assets, "Assets Panel"),
    (SolarxyTab::Texture, "Texture Viewer"),
    (SolarxyTab::Attributes, "Attributes Panel"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry this shell has and the browser does not, with the reason.
    /// Checked in reverse as well: a row naming an entry that is no longer
    /// in a menu fails, so a withdrawal has to delete its row.
    const DESKTOP_ONLY: &[(&str, &str)] = &[
        (
            "Save Scene As...",
            "a native document knows its own path, so Save and Save As are two things",
        ),
        (
            "Recent Files",
            "a native application opens files from disk, and its users expect the list",
        ),
        ("Quit", "a native application is quit from its own menu"),
        (
            "Save Review Notes",
            "becomes an export when review is repointed at the document",
        ),
    ];

    /// An entry the browser has and this shell does not, with the reason.
    const BROWSER_ONLY: &[(&str, &str)] = &[(
        "Export web bundle...",
        "the bundle assembles itself from the app's own origin, which a native app does not have",
    )];

    /// An entry both have under different words: here, there, and why.
    const WORDED_DIFFERENTLY: &[(&str, &str, &str)] = &[(
        "About Solarxy",
        "About Solarxy Web",
        "this shell is not the web one",
    )];

    fn browser_source() -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        std::fs::read_to_string(root.join("web/src/components/menu/MenuBar.tsx"))
            .expect("the browser's menu bar")
    }

    /// The top-level quoted labels of one of the browser's menus, in order.
    /// A submenu's rows are built from a table there rather than written as
    /// quoted labels, so they are not read, which is what is wanted.
    fn browser_menu(source: &str, name: &str) -> Vec<String> {
        let from = format!("const {name}: MenuEntry[] = [");
        let start = source
            .find(&from)
            .unwrap_or_else(|| panic!("no menu named {name}"));
        let block = &source[start..];
        let block = &block[..block.find("\n  ];").expect("the menu's end")];
        block
            .split("label: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect()
    }

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                Item(entry) => Some(entry.label().replace('\u{2026}', "...")),
                Divider => None,
            })
            .collect()
    }

    /// What this shell's menu is once the named differences are taken out
    /// and put back in the browser's words.
    fn as_the_browser_would_list(rows: &[Row]) -> Vec<String> {
        labels(rows)
            .into_iter()
            .filter(|label| !DESKTOP_ONLY.iter().any(|(only, _)| only == label))
            .map(|label| {
                WORDED_DIFFERENTLY
                    .iter()
                    .find(|(here, _, _)| *here == label)
                    .map_or(label, |(_, there, _)| (*there).to_string())
            })
            .collect()
    }

    /// Four of the five menus hold the browser's entries in the browser's
    /// order. The fifth, the arrangements, is built from the arrangement
    /// table and the panels, which have their own tests.
    #[test]
    fn the_menus_list_the_browsers_entries_in_its_order() {
        let source = browser_source();
        for (name, rows) in [
            ("file", FILE_MENU),
            ("edit", EDIT_MENU),
            ("review", REVIEW_MENU),
            ("help", HELP_MENU),
        ] {
            let browser: Vec<String> = browser_menu(&source, name)
                .into_iter()
                .filter(|label| !BROWSER_ONLY.iter().any(|(only, _)| only == label))
                .collect();
            assert!(
                browser.len() >= 3,
                "read {} entries for {name}, so the reader is broken",
                browser.len()
            );
            assert_eq!(as_the_browser_would_list(rows), browser, "the {name} menu");
        }
    }

    /// Every row of the three lists still applies: a desktop-only entry is
    /// really in a menu here and really absent there, and the reverse.
    #[test]
    fn every_named_difference_is_still_a_difference() {
        let source = browser_source();
        let here: Vec<String> = [FILE_MENU, EDIT_MENU, REVIEW_MENU, HELP_MENU]
            .into_iter()
            .flat_map(labels)
            .collect();
        let there: Vec<String> = ["file", "edit", "review", "help"]
            .into_iter()
            .flat_map(|name| browser_menu(&source, name))
            .collect();
        for (label, _) in DESKTOP_ONLY {
            assert!(
                here.iter().any(|l| l == label),
                "{label} is not in a menu here"
            );
            assert!(
                !there.iter().any(|l| l == label),
                "{label} is in the browser too"
            );
        }
        for (label, _) in BROWSER_ONLY {
            assert!(
                there.iter().any(|l| l == label),
                "{label} is not in the browser"
            );
            assert!(
                !here.iter().any(|l| l == label),
                "{label} is in a menu here too"
            );
        }
        for (ours, theirs, _) in WORDED_DIFFERENTLY {
            assert!(
                here.iter().any(|l| l == ours),
                "{ours} is not in a menu here"
            );
            assert!(
                there.iter().any(|l| l == theirs),
                "{theirs} is not in the browser"
            );
        }
    }

    /// The two arrangement entries are spelled as the browser spells them,
    /// and sit in its arrangements menu in that order.
    #[test]
    fn the_arrangement_entries_are_the_browsers() {
        let desks = browser_menu(&browser_source(), "desks");
        let ours = [
            SAVE_CURRENT_AS.replace('\u{2026}', "..."),
            DELETE_DESK.to_string(),
        ];
        let found: Vec<&String> = desks.iter().filter(|label| ours.contains(label)).collect();
        assert_eq!(found, ours.iter().collect::<Vec<_>>());
    }

    /// The seven panel toggles are the browser's, by label and in order.
    /// They follow its two arrangement entries in its menu, so they are what
    /// is left of that menu's quoted labels once the presets, which are
    /// built from a table there, and those two entries are passed.
    #[test]
    fn the_panel_toggles_are_the_browsers_seven() {
        let desks = browser_menu(&browser_source(), "desks");
        let after = desks
            .iter()
            .position(|label| label == DELETE_DESK)
            .expect("the browser's menu has the delete entry");
        let browser: Vec<&str> = desks[after + 1..].iter().map(String::as_str).collect();
        assert_eq!(browser.len(), 7, "the reader found the seven toggles");
        let here: Vec<&str> = PANEL_TOGGLES.iter().map(|(_, label)| *label).collect();
        assert_eq!(here, browser);
    }

    /// Every panel a user can close has a way back, and the two that do not
    /// belong in the list are not in it: the viewport, which cannot be
    /// closed, and the asset preview, which opens from an asset.
    #[test]
    fn every_closeable_panel_has_a_toggle_and_the_viewport_has_none() {
        let mut toggled: Vec<SolarxyTab> = PANEL_TOGGLES.iter().map(|(tab, _)| *tab).collect();
        toggled.push(SolarxyTab::ReviewPanel); // in the Review menu
        for tab in [
            SolarxyTab::ReviewPanel,
            SolarxyTab::Properties,
            SolarxyTab::Tree,
            SolarxyTab::Nodes,
            SolarxyTab::Assets,
            SolarxyTab::Texture,
            SolarxyTab::Attributes,
            SolarxyTab::Text,
        ] {
            assert!(
                toggled.contains(&tab),
                "{tab:?} can be closed and not reopened"
            );
        }
        assert!(!toggled.contains(&SolarxyTab::Viewport));
        assert!(!toggled.contains(&SolarxyTab::AssetPreview));
        let mut unique = toggled.clone();
        unique.dedup();
        assert_eq!(unique.len(), toggled.len(), "no panel is toggled twice");
    }

    /// An entry that names a binding shows a key, because the table binds
    /// it. A hint is never typed in, so this is the whole of what keeps a
    /// menu from advertising a key that does something else.
    #[test]
    fn every_entry_that_names_a_binding_shows_the_tables_key() {
        for rows in [FILE_MENU, EDIT_MENU, REVIEW_MENU, HELP_MENU] {
            for row in rows {
                if let Item(entry) = row
                    && let Some(action) = entry.action()
                {
                    assert!(
                        hint(action).is_some(),
                        "{} names a binding the table does not have",
                        entry.label()
                    );
                }
            }
        }
    }

    /// The bar is the browser's five menus, by name and in order.
    #[test]
    fn the_bar_is_the_browsers_five_menus() {
        let source = browser_source();
        let titles: Vec<String> = source
            .split("<MenuItem title=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect();
        assert_eq!(titles, MENU_TITLES);
    }
}
