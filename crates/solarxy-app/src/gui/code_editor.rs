//! The code editor: a line-numbered field with an error line and a
//! completion popup, used by the Text panel and by the parameter panel's
//! snippet control.
//!
//! **What egui has and what it lacks.** The browser hosts a full code
//! editor with syntax awareness; the immediate-mode toolkit has a text
//! field. So the target is what the milestone set: a line-numbered field,
//! the error's line marked from the engine's structured position, and the
//! completion vocabulary the drift test pins. Syntax highlighting is out.
//!
//! **The completion rules are the browser's** (`wrangleComplete.ts`): a
//! word after `@` offers the attribute lanes, the implicit ones first and
//! then what the input carries; a word after `$` offers the variables; any
//! other word offers the language, and `Ctrl+Space` offers it with no word
//! typed. A builtin or a query inserts with its opening bracket. The
//! language names come from the engine's own constants, which is what the
//! drift test on the browser's lists pins; the implicit lanes are the
//! browser's list, held here and pinned by a test that reads it.
//!
//! **The completion state lives in egui's memory**, keyed by the editor's
//! id, so a control that is one row among many does not need a field on
//! the panel for it.

use egui::text::{CCursor, CCursorRange};
use solarxy_graph::expr::ast::Var;
use solarxy_graph::expr::builtins::{BUILTIN_NAMES, LOCAL_TYPE_NAMES, QUERY_NAMES};
use solarxy_studio::expression::ErrorPosition;

use crate::gui::theme::Theme;

/// The lanes every wrangle can read whatever its input carries, in the
/// browser's order.
pub(crate) const IMPLICIT_LANES: &[&str] =
    &["P", "N", "Cd", "uv", "ptnum", "numpt", "primnum", "numprim"];

/// The most rows the popup lists.
const POPUP_ROWS: usize = 12;

/// One offer: what the popup shows and what accepting it inserts in place
/// of the typed prefix (sigil included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Completion {
    pub label: String,
    pub insert: String,
}

/// What the popup remembers between frames.
#[derive(Debug, Clone, Copy, Default)]
struct CompletionState {
    open: bool,
    selected: usize,
}

/// What the editor tells its caller.
pub(crate) struct EditorOutput {
    pub response: egui::Response,
    /// The text changed this frame, by typing or by an accepted completion.
    pub changed: bool,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The offers for the text before the cursor, and the character index the
/// accepted one replaces from. `None` when nothing is on offer: no word,
/// and not asked for explicitly.
pub(crate) fn completions_for(
    before: &str,
    lanes: &[(String, String)],
    explicit: bool,
) -> Option<(usize, Vec<Completion>)> {
    let chars: Vec<char> = before.chars().collect();
    let mut start = chars.len();
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let word: String = chars[start..].iter().collect();
    let sigil = start.checked_sub(1).map(|i| chars[i]);
    let matches = |name: &str| name.to_lowercase().starts_with(&word.to_lowercase());

    match sigil {
        Some('@') => {
            let mut offers: Vec<Completion> = IMPLICIT_LANES
                .iter()
                .map(|name| Completion {
                    label: format!("@{name}"),
                    insert: format!("@{name}"),
                })
                .collect();
            offers.extend(
                lanes
                    .iter()
                    .filter(|(name, _)| !IMPLICIT_LANES.contains(&name.as_str()))
                    .map(|(name, ty)| Completion {
                        label: format!("@{name}  {ty}"),
                        insert: format!("@{name}"),
                    }),
            );
            offers.retain(|c| matches(&c.insert[1..]));
            Some((start - 1, offers))
        }
        Some('$') => {
            let offers: Vec<Completion> = Var::ALL
                .iter()
                .map(|v| v.name())
                .filter(|name| matches(name))
                .map(|name| Completion {
                    label: format!("${name}"),
                    insert: format!("${name}"),
                })
                .collect();
            Some((start - 1, offers))
        }
        _ if !word.is_empty() || explicit => {
            let offers: Vec<Completion> = language()
                .into_iter()
                .filter(|c| matches(&c.label))
                .collect();
            Some((start, offers))
        }
        _ => None,
    }
}

/// The static language offers: builtins and queries with their bracket,
/// then the local types.
fn language() -> Vec<Completion> {
    BUILTIN_NAMES
        .iter()
        .chain(QUERY_NAMES.iter())
        .map(|name| Completion {
            label: (*name).to_string(),
            insert: format!("{name}("),
        })
        .chain(LOCAL_TYPE_NAMES.iter().map(|name| Completion {
            label: (*name).to_string(),
            insert: (*name).to_string(),
        }))
        .collect()
}

/// `text` with the characters `from..to` replaced by `insert`, and the
/// character index after the insertion.
pub(crate) fn accept(text: &mut String, from: usize, to: usize, insert: &str) -> usize {
    let byte = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(b, _)| b)
    };
    let (a, b) = (byte(from), byte(to));
    text.replace_range(a..b, insert);
    from + insert.chars().count()
}

/// Draw the editor over `text`, `rows` tall at least, marking `error`'s
/// line, offering completions when `vocabulary` is given (the input's
/// attribute lanes, which may be empty).
pub(in crate::gui) fn code_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    rows: usize,
    error: Option<&ErrorPosition>,
    vocabulary: Option<&[(String, String)]>,
    theme: Theme,
) -> EditorOutput {
    let edit_id = id.with("edit");
    let mut comp: CompletionState = ui.memory(|m| m.data.get_temp(id).unwrap_or_default());
    let focused = ui.memory(|m| m.has_focus(edit_id));

    // The popup's keys are taken before the field sees them, or Enter
    // would insert a line and Tab a tab under the accepted offer.
    let mut explicit = false;
    let mut accept_now = false;
    if vocabulary.is_some() && focused {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space)) {
            comp.open = true;
            comp.selected = 0;
            explicit = true;
        }
        if comp.open {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                comp.open = false;
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                comp.selected = comp.selected.saturating_add(1);
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                comp.selected = comp.selected.saturating_sub(1);
            }
            if ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
            }) {
                accept_now = true;
            }
        }
    }

    let line_count = text.lines().count().max(1);
    let gutter = rows.max(line_count);
    let mut output = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in 1..=gutter {
                let marked = error.is_some_and(|p| p.line == line);
                ui.label(
                    egui::RichText::new(format!("{line:>3}"))
                        .monospace()
                        .size(10.0)
                        .color(if marked {
                            theme.severity_error
                        } else {
                            theme.muted
                        }),
                );
            }
        });
        output = Some(
            egui::TextEdit::multiline(text)
                .id(edit_id)
                .desired_rows(rows)
                .desired_width(f32::INFINITY)
                .code_editor()
                .show(ui),
        );
    });
    let Some(output) = output else {
        unreachable!("the field is drawn inside the row");
    };
    let mut changed = output.response.changed();

    // Typing reopens the offers, as the browser's editor does; a change
    // that leaves no word closes them.
    if vocabulary.is_some() && changed {
        comp.open = true;
        comp.selected = 0;
    }

    if let (Some(lanes), true) = (vocabulary, comp.open || explicit) {
        let cursor = output
            .cursor_range
            .map_or(text.chars().count(), |r| r.primary.index);
        let before: String = text.chars().take(cursor).collect();
        match completions_for(&before, lanes, explicit) {
            Some((from, offers)) if !offers.is_empty() => {
                comp.open = true;
                comp.selected = comp.selected.min(offers.len() - 1);
                let mut picked: Option<usize> = accept_now.then_some(comp.selected);
                let anchor = output.galley_pos
                    + output
                        .galley
                        .pos_from_cursor(CCursor::new(cursor))
                        .left_bottom()
                        .to_vec2();
                egui::Area::new(id.with("popup"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(anchor)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.set_min_width(160.0);
                            for (i, offer) in offers.iter().enumerate().take(POPUP_ROWS) {
                                if ui
                                    .selectable_label(i == comp.selected, &offer.label)
                                    .clicked()
                                {
                                    picked = Some(i);
                                }
                            }
                        });
                    });
                if let Some(i) = picked
                    && let Some(offer) = offers.get(i)
                {
                    let after = accept(text, from, cursor, &offer.insert);
                    let mut state = output.state;
                    state
                        .cursor
                        .set_char_range(Some(CCursorRange::one(CCursor::new(after))));
                    state.store(ui.ctx(), edit_id);
                    comp.open = false;
                    changed = true;
                }
            }
            _ => comp.open = false,
        }
    } else {
        comp.open = false;
    }
    ui.memory_mut(|m| m.data.insert_temp(id, comp));

    EditorOutput {
        response: output.response,
        changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(offers: &[Completion]) -> Vec<&str> {
        offers.iter().map(|c| c.label.as_str()).collect()
    }

    /// A word after `@` offers the implicit lanes first and then the
    /// input's, without duplicating one that is both, and replaces from the
    /// sigil.
    #[test]
    fn an_at_sign_offers_the_lanes_from_the_sigil() {
        let lanes = vec![
            ("P".to_string(), "vec3".to_string()),
            ("mass".to_string(), "float".to_string()),
        ];
        let (from, offers) = completions_for("x = @", &lanes, false).expect("offers");
        assert_eq!(from, 4, "the replacement starts at the sigil");
        assert_eq!(
            labels(&offers),
            [
                "@P",
                "@N",
                "@Cd",
                "@uv",
                "@ptnum",
                "@numpt",
                "@primnum",
                "@numprim",
                "@mass  float"
            ]
        );
        let (from, offers) = completions_for("@nu", &lanes, false).expect("offers");
        assert_eq!(from, 0);
        assert_eq!(labels(&offers), ["@numpt", "@numprim"]);
        assert_eq!(offers[0].insert, "@numpt");
    }

    /// A word after `$` offers the variables; any other word offers the
    /// language with brackets on the callables; no word offers nothing
    /// unless asked.
    #[test]
    fn dollar_words_and_bare_words_offer_the_variables_and_the_language() {
        let (from, offers) = completions_for("v = $", &[], false).expect("offers");
        assert_eq!(from, 4);
        assert_eq!(labels(&offers), ["$T", "$F", "$FPS", "$PI", "$E"]);

        let (from, offers) = completions_for("y = si", &[], false).expect("offers");
        assert_eq!(from, 4);
        assert!(labels(&offers).contains(&"sin"), "{:?}", labels(&offers));
        let sin = offers.iter().find(|c| c.label == "sin").expect("sin");
        assert_eq!(sin.insert, "sin(", "a callable inserts its bracket");

        let (_, offers) = completions_for("float x = fl", &[], false).expect("offers");
        assert_eq!(labels(&offers), ["floor", "float"], "a type inserts bare");
        assert_eq!(offers[1].insert, "float");

        assert!(
            completions_for("x = ", &[], false).is_none(),
            "no word, not asked"
        );
        let (from, offers) = completions_for("x = ", &[], true).expect("asked for");
        assert_eq!(from, 4);
        assert_eq!(
            offers.len(),
            BUILTIN_NAMES.len() + QUERY_NAMES.len() + LOCAL_TYPE_NAMES.len()
        );
    }

    /// Accepting replaces the typed prefix, sigil included, and lands the
    /// cursor after what was inserted, counted in characters.
    #[test]
    fn accepting_replaces_the_prefix_and_places_the_cursor_after_it() {
        let mut text = "a = @nu + 1".to_string();
        let after = accept(&mut text, 4, 7, "@numpt");
        assert_eq!(text, "a = @numpt + 1");
        assert_eq!(after, 10);
        let mut text = "\u{e9}t\u{e9} = si".to_string();
        let after = accept(&mut text, 6, 8, "sin(");
        assert_eq!(text, "\u{e9}t\u{e9} = sin(");
        assert_eq!(after, 10, "characters before the insertion, not bytes");
        // A lane named outside ASCII: the cursor lands after its characters,
        // which are fewer than its bytes.
        let mut text = "@gr".to_string();
        let after = accept(&mut text, 0, 3, "@gr\u{f6}\u{df}e");
        assert_eq!(text, "@gr\u{f6}\u{df}e");
        assert_eq!(after, 6, "characters inserted, not bytes");
    }

    /// The implicit lanes are the browser's list, in its order, read from
    /// its source rather than restated.
    #[test]
    fn the_implicit_lanes_are_the_browsers() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../web/src/components/inputs/wrangleComplete.ts"),
        )
        .expect("the browser's completion source");
        let start = src.find("const IMPLICIT_LANES").expect("the list");
        let end = src[start..].find("];").expect("the list closes") + start;
        let names: Vec<String> = src[start..end]
            .lines()
            .filter_map(|line| {
                line.split("name: \"")
                    .nth(1)?
                    .split('"')
                    .next()
                    .map(str::to_string)
            })
            .collect();
        assert_eq!(names, IMPLICIT_LANES);
    }
}
