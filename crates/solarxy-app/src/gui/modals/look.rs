//! The per-pane look editor: exposure, the tone mapper, and the lift, gamma
//! and gain grade.
//!
//! A window rather than entries in the pane's Display dropdown, because that
//! dropdown is a list of checkmarks and submenus and these are continuous
//! values. The browser's dialog for the same job is the reference, control for
//! control, and its wording is reused rather than rewritten.
//!
//! **Modeless, and one window per pane.** The browser's is a modal with a
//! backdrop; this one is not, because the whole activity is looking at the
//! picture while turning a knob, and a backdrop is precisely what stops you
//! doing that. One window per pane follows from the same place: the ticket's
//! own verification is two open at once, each editing its own pane, which a
//! single retargeting dialog cannot do. The browser's modality is filed as a
//! gap rather than copied.
//!
//! **Deliberately smaller than the camera node's look.** A pane reaches the
//! scalars only; the two lookup-table slots live on the camera, because a
//! table is a staged document asset and a pane is a viewport rather than a
//! document object. A pane looking through a camera says so and sends you to
//! the node, instead of editing a value the pane is not compositing with.

use solarxy_core::preferences::ToneMode;
use solarxy_core::view_config::PaneLook;

use crate::gui::intent::{Intent, Intents, PaneLookIntent};
use crate::gui::theme::Theme;

/// The tone mappers the editor offers, with the browser's labels.
///
/// The labels differ from `ToneMode`'s own `Display` in one place, `None`,
/// which reads as `None (clip)` here because a dropdown entry called `None`
/// beside three named curves says nothing about what it does. Held against
/// the browser's `TONE_MODES` by a test.
pub(crate) const TONE_MODES: [(ToneMode, &str); 4] = [
    (ToneMode::None, "None (clip)"),
    (ToneMode::Linear, "Linear"),
    (ToneMode::Reinhard, "Reinhard"),
    (ToneMode::AcesFilmic, "ACES Filmic"),
];

/// One grade vector as a row: which one, its label, its hover text, the drag
/// step, and the floor below which it stops meaning anything.
struct GradeRow {
    label: &'static str,
    doc: &'static str,
    step: f64,
    min: f32,
}

/// The three grade rows, in the browser's order and with its steps.
const GRADE_ROWS: [GradeRow; 3] = [
    GradeRow {
        label: "Lift",
        doc: "Raises or lowers the darkest part of the image, per channel, after \
              tone mapping. Positive lifts the blacks towards grey for a faded \
              base; negative crushes them. It is an addition, so it moves shadows \
              far more than highlights.",
        step: 0.01,
        // No floor: a negative lift is a real grade, and the browser passes no
        // minimum for this row either.
        min: f32::MIN,
    },
    GradeRow {
        label: "Gamma",
        doc: "Bends the midtones per channel without moving black or white: above \
              1 brightens, below 1 darkens. The control for an image whose ends \
              are right and whose middle is not. 1 is neutral.",
        step: 0.05,
        min: 0.01,
    },
    GradeRow {
        label: "Gain",
        doc: "Multiplies each channel, which moves the highlights most and leaves \
              black at black. Use it to set the white point, or to warm and cool \
              an image by pushing red and blue apart. 1 is neutral.",
        step: 0.05,
        min: 0.0,
    },
];

/// The note a bound pane carries, verbatim from the browser's dialog.
const THROUGH_CAMERA: &str = "This pane is looking through a camera, so it composites with that \
                              camera's look. Edit it on the camera node, where it also saves with \
                              the scene and carries the two LUT slots. The values below apply when \
                              the pane goes back to a free view.";

/// Why the two table slots are not here, verbatim from the browser's dialog.
const LUT_NOTE: &str = "The two LUT slots live on the camera node: a table is part of the \
                        document, so it travels with the scene rather than with the viewport.";

/// Which panes have an editor open. One flag each, so two can be up at once.
pub(crate) type LookEditors = [bool; 4];

/// Draw every open look editor.
///
/// Takes the looks read-only and raises an intent per change, like every other
/// panel: the shell owns the value and this draws against a borrowed copy.
pub(in crate::gui) fn draw_look_editors(
    ctx: &egui::Context,
    open: &mut LookEditors,
    looks: &[PaneLook; 4],
    bound: [bool; 4],
    intents: &mut Intents,
    theme: &Theme,
) {
    for pane in 0..4 {
        if !open[pane] {
            continue;
        }
        draw_one(
            ctx,
            pane,
            &mut open[pane],
            looks[pane],
            bound[pane],
            intents,
            theme,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_one(
    ctx: &egui::Context,
    pane: usize,
    open: &mut bool,
    look: PaneLook,
    bound: bool,
    intents: &mut Intents,
    theme: &Theme,
) {
    // The value this pass edits. Every control writes into it and one intent
    // is raised at the end if anything moved, so a drag across two fields in
    // one frame cannot have the second overwrite the first from a stale copy.
    let mut next = look;
    let mut changed = false;
    let mut done = false;
    let mut keep = true;

    egui::Window::new(format!("Look: pane {}", pane + 1))
        .id(egui::Id::new(("solarxy_look_editor", pane)))
        .collapsible(false)
        .resizable(false)
        .movable(true)
        .open(&mut keep)
        .show(ctx, |ui| {
            ui.set_min_width(300.0);
            if bound {
                ui.label(
                    egui::RichText::new(THROUGH_CAMERA)
                        .small()
                        .color(theme.muted),
                );
                ui.add_space(6.0);
            }

            ui.label(egui::RichText::new("Exposure and tone").strong());
            egui::Grid::new(("look_scalars", pane))
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Exposure").on_hover_text(
                        "Linear multiplier on the whole image before tone mapping, so 2 is \
                         one stop brighter and 0.5 one stop darker. Reach for this before \
                         changing light intensities: it moves the exposure of the shot \
                         rather than the lighting of the scene.",
                    );
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut next.exposure)
                                .speed(0.1)
                                .range(0.01..=64.0),
                        )
                        .changed();
                    ui.end_row();

                    ui.label("Tone map").on_hover_text(
                        "How high dynamic range is brought down to what a screen can show. \
                         ACES Filmic is the filmic default; Reinhard is gentler and flatter; \
                         Linear and None both clip, and are for judging raw values rather \
                         than for looking at.",
                    );
                    let current = tone_label(next.tone_mode);
                    egui::ComboBox::from_id_salt(("look_tone", pane))
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (mode, label) in TONE_MODES {
                                if ui.selectable_label(next.tone_mode == mode, label).clicked() {
                                    next.tone_mode = mode;
                                    changed = true;
                                }
                            }
                        });
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.label(egui::RichText::new("Grade").strong());
            egui::Grid::new(("look_grade", pane))
                .num_columns(4)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("");
                    for ch in ["R", "G", "B"] {
                        ui.label(egui::RichText::new(ch).small().color(theme.muted));
                    }
                    ui.end_row();
                    for (row_index, row) in GRADE_ROWS.iter().enumerate() {
                        ui.label(row.label).on_hover_text(row.doc);
                        let values = match row_index {
                            0 => &mut next.lift,
                            1 => &mut next.gamma,
                            _ => &mut next.gain,
                        };
                        for value in values.iter_mut() {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(value)
                                        .speed(row.step)
                                        .range(row.min..=f32::MAX),
                                )
                                .changed();
                        }
                        ui.end_row();
                    }
                });
            ui.label(egui::RichText::new(LUT_NOTE).small().color(theme.muted));

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button("Reset")
                    .on_hover_text("Back to the look that changes nothing")
                    .clicked()
                {
                    next = PaneLook::default();
                    changed = true;
                }
                if ui.button("Done").clicked() {
                    done = true;
                }
            });
        });

    if changed {
        intents.raise(Intent::PaneLook(PaneLookIntent::Set { pane, look: next }));
    }
    if done || !keep {
        *open = false;
    }
}

/// The label a tone mapper is shown under.
fn tone_label(mode: ToneMode) -> &'static str {
    TONE_MODES
        .iter()
        .find(|(m, _)| *m == mode)
        .map_or("ACES Filmic", |(_, label)| *label)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser's own dialog, read from disk.
    fn browser_source() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/components/PaneLookModal.tsx");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("the browser dialog is readable at {}: {e}", path.display()))
    }

    /// The tone mappers and their labels are the browser's four, in its order.
    ///
    /// A source read rather than a copied list, because a label typed twice is
    /// how two shells come to call one curve different things.
    #[test]
    fn the_tone_mappers_are_the_browsers_four() {
        let source = browser_source();
        let start = source
            .find("export const TONE_MODES")
            .expect("the reader found the browser's tone list");
        let end = source[start..]
            .find("];")
            .expect("the tone list is terminated")
            + start;
        let block = &source[start..end];
        // Each entry reads `["Stored", "Label"],`.
        let browser: Vec<(String, String)> = block
            .lines()
            .filter_map(|line| {
                let mut quoted = line.split('"').skip(1).step_by(2);
                Some((quoted.next()?.to_owned(), quoted.next()?.to_owned()))
            })
            .collect();
        assert_eq!(browser.len(), 4, "the reader found the browser's four");

        let here: Vec<(String, String)> = TONE_MODES
            .iter()
            .map(|(mode, label)| {
                let stored = serde_json::to_string(mode)
                    .expect("a tone mode serializes")
                    .trim_matches('"')
                    .to_owned();
                (stored, (*label).to_owned())
            })
            .collect();
        assert_eq!(here, browser);
    }

    /// Reset writes the look that changes nothing, and that look is the one
    /// the browser calls neutral.
    // Exact comparison is the point rather than an oversight, which is the
    // same judgement `composite::grade_is_neutral` records: every value here
    // is exactly representable, and neutral has to be bit-identical or the
    // grade stops being a no-op and the goldens move.
    #[allow(clippy::float_cmp)]
    #[test]
    fn reset_writes_the_neutral_look_the_browser_names() {
        let source = browser_source();
        let start = source
            .find("export const NEUTRAL")
            .expect("the reader found the browser's neutral look");
        let end = source[start..].find("};").expect("it is terminated") + start;
        let block = &source[start..end];

        let neutral = PaneLook::default();
        assert!(
            block.contains("exposure: 1"),
            "the browser's neutral exposure is one"
        );
        assert!(
            (neutral.exposure - 1.0).abs() < f32::EPSILON,
            "and so is this one"
        );
        assert!(block.contains("toneMode: \"AcesFilmic\""));
        assert_eq!(neutral.tone_mode, ToneMode::AcesFilmic);
        assert!(block.contains("lift: [0, 0, 0]"));
        assert_eq!(neutral.lift, [0.0; 3]);
        assert!(block.contains("gamma: [1, 1, 1]"));
        assert_eq!(neutral.gamma, [1.0; 3]);
        assert!(block.contains("gain: [1, 1, 1]"));
        assert_eq!(neutral.gain, [1.0; 3]);
    }

    /// The grade rows carry the browser's steps and floors, which is what
    /// makes a drag here move a value the way a drag there does.
    #[allow(clippy::float_cmp)]
    #[test]
    fn the_grade_rows_carry_the_browsers_steps_and_floors() {
        let source = browser_source();
        // Lift: step 0.01 and no floor at all.
        assert!(source.contains("step: 0.01"));
        assert!((GRADE_ROWS[0].step - 0.01).abs() < f64::EPSILON);
        assert_eq!(
            GRADE_ROWS[0].min,
            f32::MIN,
            "a negative lift is a real grade"
        );
        // Gamma and gain both step 0.05; gamma floors at 0.01 and gain at 0.
        assert!(source.contains("step: 0.05"));
        assert!((GRADE_ROWS[1].step - 0.05).abs() < f64::EPSILON);
        assert!((GRADE_ROWS[2].step - 0.05).abs() < f64::EPSILON);
        assert!(source.contains("min: 0.01"));
        assert!((GRADE_ROWS[1].min - 0.01).abs() < f32::EPSILON);
        assert!(source.contains("min: 0,"));
        assert!(GRADE_ROWS[2].min.abs() < f32::EPSILON);
        assert_eq!(
            GRADE_ROWS.map(|r| r.label),
            ["Lift", "Gamma", "Gain"],
            "in the browser's order"
        );
    }

    /// The two notes are the browser's words rather than a retelling.
    #[test]
    fn the_notes_are_the_browsers_words() {
        let source = browser_source();
        // The browser escapes the apostrophe as an HTML entity and wraps its
        // prose across lines, so both sides are compared with whitespace
        // collapsed and that one entity undone.
        let browser = source.replace("&apos;", "'");
        let flat: String = browser.split_whitespace().collect::<Vec<_>>().join(" ");
        for note in [THROUGH_CAMERA, LUT_NOTE] {
            let want: String = note.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                flat.contains(&want),
                "the browser does not carry this note verbatim:\n{want}"
            );
        }
    }
}
