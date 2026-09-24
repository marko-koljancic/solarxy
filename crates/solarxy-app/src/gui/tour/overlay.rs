//! Drawing a tour: the scrim, the spotlight, and the card.
//!
//! **The scrim is four rectangles around the subject rather than one with a
//! hole in it.** The browser's comment says why and it holds here for the
//! same reason: the hole must not take pointer events, so what is being
//! described stays live underneath and the bright patch reads as a hole
//! rather than as a highlight painted on top.
//!
//! The card is placed by [`super::placement`], which decides which side it
//! sits on. Everything else here is drawing.

use super::placement::{CARD_HEIGHT, CARD_WIDTH, place_coachmark};
use super::steps::{TourAnchor, TourDef, TourStep, tour_by_id};
use crate::gui::theme::Theme;

/// How dark the scrim is. Enough to push the rest of the interface back
/// without hiding it, since half the point is seeing what is described in
/// its place.
const SCRIM_ALPHA: u8 = 150;

/// The rectangles a tour can point at, as the last frame drew them.
///
/// `None` means the surface is not on screen, which on this shell is
/// ordinary rather than exceptional: the default arrangement mounts three
/// panels and every other one is a toggle away. A step whose surface is
/// absent is dropped when the tour starts, which is what the browser's own
/// filtering does with a selector that matches nothing.
#[derive(Default, Clone, Copy)]
pub(in crate::gui) struct TourAnchors {
    pub viewport: Option<egui::Rect>,
    pub tool_column: Option<egui::Rect>,
    pub pane_controls: Option<egui::Rect>,
    pub node_canvas: Option<egui::Rect>,
    pub properties_body: Option<egui::Rect>,
    pub review_panel: Option<egui::Rect>,
    pub attr_column: Option<egui::Rect>,
    pub menu_bar: Option<egui::Rect>,
}

impl TourAnchors {
    pub(in crate::gui) fn get(&self, anchor: TourAnchor) -> Option<egui::Rect> {
        match anchor {
            TourAnchor::Viewport => self.viewport,
            TourAnchor::ToolColumn => self.tool_column,
            TourAnchor::PaneControls => self.pane_controls,
            TourAnchor::NodeCanvas => self.node_canvas,
            TourAnchor::PropertiesBody => self.properties_body,
            TourAnchor::ReviewPanel => self.review_panel,
            TourAnchor::AttrColumn => self.attr_column,
            TourAnchor::MenuBar => self.menu_bar,
        }
    }
}

/// A tour in progress, or none.
///
/// Held on the renderer like every modal and drained through a `take_*`
/// accessor, which is this shell's shape for a single-value handle.
#[derive(Default)]
pub(in crate::gui) struct TourState {
    tour: Option<&'static TourDef>,
    /// Indices into the tour's steps, filtered to those whose surface was
    /// on screen when it started. Fixed at start rather than per frame, so
    /// the count under the card does not change while it is read.
    run: Vec<usize>,
    at: usize,
    /// The id of a tour that just finished, for the state layer to record.
    completed: Option<&'static str>,
}

impl TourState {
    pub(in crate::gui) fn running(&self) -> bool {
        self.tour.is_some()
    }

    /// Start a tour, dropping any step whose surface is not on screen.
    ///
    /// A tour with nothing left to show does not start at all, rather than
    /// opening on an empty card.
    pub(in crate::gui) fn start(&mut self, id: &str, anchors: &TourAnchors) {
        let tour = tour_by_id(id);
        let run: Vec<usize> = tour
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| anchors.get(step.anchor).is_some())
            .map(|(i, _)| i)
            .collect();
        if run.is_empty() {
            return;
        }
        self.tour = Some(tour);
        self.run = run;
        self.at = 0;
        self.completed = None;
    }

    fn step(&self) -> Option<&'static TourStep> {
        let tour = self.tour?;
        tour.steps.get(*self.run.get(self.at)?)
    }

    fn finish(&mut self) {
        self.completed = self.tour.map(|t| t.id);
        self.tour = None;
        self.run.clear();
        self.at = 0;
    }

    /// The tour that finished since this was last asked, if any.
    pub(in crate::gui) fn take_completed(&mut self) -> Option<&'static str> {
        self.completed.take()
    }
}

/// Draw the running tour, if one is running.
pub(in crate::gui) fn draw_tour(
    ctx: &egui::Context,
    state: &mut TourState,
    anchors: &TourAnchors,
    theme: Theme,
) {
    let Some(step) = state.step() else {
        return;
    };
    let Some(anchor) = anchors.get(step.anchor) else {
        // The surface went away mid-tour, which a closed panel can do.
        // Ending is kinder than pointing at nothing.
        state.finish();
        return;
    };

    // Claimed before anything else can read them, because a tour is modal
    // in intent even though the interface underneath stays live.
    let (escape, forward, back) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
        )
    });

    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("solarxy_tour_scrim"),
    ));
    let scrim = egui::Color32::from_black_alpha(SCRIM_ALPHA);
    // Above, below, and the two beside, so the subject itself is untouched.
    for rect in [
        egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, anchor.min.y)),
        egui::Rect::from_min_max(egui::pos2(screen.min.x, anchor.max.y), screen.max),
        egui::Rect::from_min_max(
            egui::pos2(screen.min.x, anchor.min.y),
            egui::pos2(anchor.min.x, anchor.max.y),
        ),
        egui::Rect::from_min_max(
            egui::pos2(anchor.max.x, anchor.min.y),
            egui::pos2(screen.max.x, anchor.max.y),
        ),
    ] {
        if rect.is_positive() {
            painter.rect_filled(rect, 0.0, scrim);
        }
    }
    painter.rect_stroke(
        anchor,
        4.0,
        egui::Stroke::new(2.0_f32, theme.accent),
        egui::StrokeKind::Inside,
    );

    let card = egui::vec2(CARD_WIDTH, CARD_HEIGHT);
    let (at, _side) = place_coachmark(anchor, card, screen.size(), step.side);
    let total = state.run.len();
    let index = state.at;

    let mut action = None;
    // Keyed on the step, so two steps never share widget identity: egui
    // would otherwise carry one card's interaction state into the next.
    egui::Area::new(egui::Id::new(("solarxy_tour_card", step.id)))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme.bg_elevated)
                .show(ui, |ui| {
                    ui.set_width(CARD_WIDTH - 16.0);
                    ui.label(egui::RichText::new(step.title).strong());
                    ui.add_space(4.0);
                    ui.label(step.body);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{} of {total}", index + 1))
                                .small()
                                .weak(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let last = index + 1 >= total;
                            if ui.button(if last { "Done" } else { "Next" }).clicked() {
                                action = Some(if last { Act::Finish } else { Act::Next });
                            }
                            if index > 0 && ui.button("Back").clicked() {
                                action = Some(Act::Back);
                            }
                            if ui.button("Skip").clicked() {
                                action = Some(Act::Finish);
                            }
                        });
                    });
                });
        });

    let last = index + 1 >= total;
    let action = action.or(if escape {
        Some(Act::Finish)
    } else if forward {
        Some(if last { Act::Finish } else { Act::Next })
    } else if back && index > 0 {
        Some(Act::Back)
    } else {
        None
    });

    match action {
        Some(Act::Next) => state.at += 1,
        Some(Act::Back) => state.at = state.at.saturating_sub(1),
        Some(Act::Finish) => state.finish(),
        None => {}
    }
}

enum Act {
    Next,
    Back,
    Finish,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_anchor() -> TourAnchors {
        let r = Some(egui::Rect::from_min_size(
            egui::pos2(10.0, 10.0),
            egui::vec2(100.0, 50.0),
        ));
        TourAnchors {
            viewport: r,
            tool_column: r,
            pane_controls: r,
            node_canvas: r,
            properties_body: r,
            review_panel: r,
            attr_column: r,
            menu_bar: r,
        }
    }

    #[test]
    fn a_tour_runs_every_step_when_every_surface_is_up() {
        let mut state = TourState::default();
        state.start("overview", &every_anchor());
        assert!(state.running());
        assert_eq!(state.run.len(), 7);
    }

    /// The desktop mounts three panels by default, so a step pointing at a
    /// closed one is the ordinary case rather than the exception.
    #[test]
    fn steps_whose_surface_is_absent_are_dropped_at_the_start() {
        let mut anchors = every_anchor();
        anchors.review_panel = None;
        anchors.attr_column = None;
        let mut state = TourState::default();
        state.start("overview", &anchors);
        // The overview's review step is the only one of its seven that
        // points at a panel now absent.
        assert_eq!(state.run.len(), 6);
        assert!(
            state
                .tour
                .expect("running")
                .steps
                .iter()
                .enumerate()
                .filter(|(i, _)| state.run.contains(i))
                .all(|(_, s)| s.anchor != TourAnchor::ReviewPanel)
        );
    }

    #[test]
    fn a_tour_with_nothing_to_show_does_not_start() {
        let mut state = TourState::default();
        state.start("overview", &TourAnchors::default());
        assert!(!state.running());
    }

    /// Only the tour that finished is reported, and only once.
    #[test]
    fn completion_is_reported_once() {
        let mut state = TourState::default();
        state.start("modeling", &every_anchor());
        state.finish();
        assert!(!state.running());
        assert_eq!(state.take_completed(), Some("modeling"));
        assert_eq!(state.take_completed(), None);
    }

    /// Starting a tour clears a completion nobody drained, so a replay
    /// cannot be credited with the run before it.
    #[test]
    fn starting_again_clears_an_undrained_completion() {
        let mut state = TourState::default();
        state.start("overview", &every_anchor());
        state.finish();
        state.start("review", &every_anchor());
        assert_eq!(state.take_completed(), None);
    }
}
