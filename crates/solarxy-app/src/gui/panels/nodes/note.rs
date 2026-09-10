//! The note: the one node on the canvas that is not an operation.
//!
//! **Its size is a parameter, not a role.** Every other role occupies the
//! one fixed layout box; a note is as big as its author made it, because
//! what it holds is prose and prose needs room. That is the whole reason
//! it is drawn by hand rather than by the shared art.
//!
//! **Every mutation is an ordinary parameter write.** The colour, the
//! size and the text are all params the registry declares, so a note
//! undoes like any other edit and round-trips through a scene file with
//! nothing special written for it. The resize is the one gesture that
//! writes two params, and it wraps them in a transaction so a drag is one
//! entry in the history rather than two.
//!
//! The desktop keys on the **role** rather than on the type identifier,
//! which is the one place it can do better than the browser: a second
//! kind of annotation added in Rust with the note role draws here with no
//! change to this shell.

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use solarxy_graph::document::NodeData;
use solarxy_graph::params::{ParamSource, ParamValue};

use crate::gui::theme::Theme;

/// The pastel set, first entry matching the registry's own default so a
/// freshly added note is already on the ring rather than beside it.
const COLOURS: [[f32; 3]; 6] = [
    [0.992, 0.902, 0.541],
    [0.655, 0.953, 0.816],
    [0.749, 0.859, 0.996],
    [0.984, 0.812, 0.910],
    [0.929, 0.914, 0.996],
    [0.996, 0.894, 0.902],
];

/// How near two colours must be to count as the same entry on the ring.
/// A note whose colour was set outside this canvas simply starts the
/// cycle from the beginning rather than matching nothing and staying put.
const COLOUR_TOLERANCE: f32 = 0.02;

/// The size a note falls back to, which is the registry's declared
/// default. Read from the params in practice; here for a note whose
/// params are somehow absent.
const DEFAULT_SIZE: Vec2 = vec2(160.0, 80.0);

/// The corner grip's side.
const GRIP: f32 = 12.0;

/// What a gesture on a note asked for.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum NoteAction {
    /// A resize finished. Both params travel together so the drag is one
    /// undo step.
    Resize { width: f32, height: f32 },
    /// The next colour on the ring.
    Colour([f32; 4]),
    /// Open the editor on this note. The text itself is committed by the
    /// editor rather than reported here, because a note's own draw is
    /// replaced by the editor while one is open.
    Edit,
}

/// A note's size, clamped to what the registry allows.
///
/// Clamped here as well as by the engine, because the preview has to
/// agree with what will be committed: a grip dragged past the range would
/// otherwise grow under the pointer and snap back on release.
#[must_use]
pub(super) fn size(data: &NodeData, registry: &solarxy_graph::registry::Registry) -> Vec2 {
    let read = |key: &str, fallback: f32| -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        let value = match data.params.get(key) {
            Some(ParamSource::Literal(ParamValue::Float(v))) => *v as f32,
            #[allow(clippy::cast_precision_loss)]
            Some(ParamSource::Literal(ParamValue::Int(v))) => *v as f32,
            _ => fallback,
        };
        let range = registry
            .get(&data.type_id)
            .and_then(|desc| desc.param(key))
            .and_then(|spec| spec.range.as_ref())
            .map(|r| r.hard);
        match range {
            #[allow(clippy::cast_possible_truncation)]
            Some((low, high)) => value.clamp(low as f32, high as f32),
            None => value,
        }
    };
    vec2(
        read("width", DEFAULT_SIZE.x),
        read("height", DEFAULT_SIZE.y),
    )
}

/// The colour a note is painted, and the alpha it keeps.
#[must_use]
pub(super) fn colour(data: &NodeData) -> [f32; 4] {
    match data.params.get("color") {
        Some(ParamSource::Literal(ParamValue::Color(rgba))) => *rgba,
        _ => [COLOURS[0][0], COLOURS[0][1], COLOURS[0][2], 1.0],
    }
}

/// The next colour on the ring.
///
/// A colour that matches no entry starts the cycle from the beginning
/// rather than doing nothing, so a note is never stuck on a colour this
/// surface cannot move off.
#[must_use]
pub(super) fn next_colour(current: [f32; 4]) -> [f32; 4] {
    let at = COLOURS.iter().position(|entry| {
        (entry[0] - current[0]).abs()
            + (entry[1] - current[1]).abs()
            + (entry[2] - current[2]).abs()
            < COLOUR_TOLERANCE
    });
    let next = COLOURS[at.map_or(0, |i| (i + 1) % COLOURS.len())];
    [next[0], next[1], next[2], current[3]]
}

/// The font a note's text is set in, from its declared size.
#[must_use]
pub(super) fn text_size(data: &NodeData) -> f32 {
    match data.params.get("text_size") {
        Some(ParamSource::Literal(ParamValue::Enum(key))) => match key.as_str() {
            "medium" => 13.0,
            "large" => 17.0,
            _ => 11.0,
        },
        _ => 11.0,
    }
}

/// The text a note holds.
#[must_use]
pub(super) fn text(data: &NodeData) -> String {
    match data.params.get("text") {
        Some(ParamSource::Literal(ParamValue::Text(value))) => value.clone(),
        _ => String::new(),
    }
}

/// Draw a note into the box it was allocated, and answer what a gesture
/// on it asked for.
pub(super) fn draw(
    ui: &Ui,
    box_rect: Rect,
    data: &NodeData,
    registry: &solarxy_graph::registry::Registry,
    editing: bool,
    theme: Theme,
) -> Option<NoteAction> {
    let rgba = colour(data);
    let fill = Color32::from_rgba_unmultiplied(
        channel(rgba[0]),
        channel(rgba[1]),
        channel(rgba[2]),
        // Not opaque, deliberately: a note sits over the graph rather
        // than in it, and a wire passing under one should still read.
        199,
    );
    let painter = ui.painter();
    painter.rect_filled(box_rect, CornerRadius::same(6), fill);
    painter.rect_stroke(
        box_rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Inside,
    );

    if !editing {
        let body = text(data);
        painter.text(
            box_rect.shrink(6.0).left_top(),
            egui::Align2::LEFT_TOP,
            if body.is_empty() {
                "Double-click to edit".to_string()
            } else {
                body
            },
            egui::FontId::proportional(text_size(data)),
            theme.fg,
        );
    }

    let mut action = None;

    // The colour swatch, top right.
    let swatch = Rect::from_min_size(
        pos2(box_rect.right() - GRIP - 4.0, box_rect.top() + 4.0),
        Vec2::splat(GRIP),
    );
    let swatch_response = ui
        .interact(
            swatch,
            ui.id().with(("note-colour", data.id.0)),
            Sense::click(),
        )
        .on_hover_text("Next colour");
    painter.rect_filled(
        swatch,
        CornerRadius::same(3),
        Color32::from_rgb(channel(rgba[0]), channel(rgba[1]), channel(rgba[2])),
    );
    if swatch_response.clicked() {
        action = Some(NoteAction::Colour(next_colour(rgba)));
    }

    // The resize grip, bottom right.
    let grip = Rect::from_min_size(
        pos2(box_rect.right() - GRIP, box_rect.bottom() - GRIP),
        Vec2::splat(GRIP),
    );
    let grip_response = ui.interact(
        grip,
        ui.id().with(("note-grip", data.id.0)),
        Sense::click_and_drag(),
    );
    painter.line_segment(
        [grip.left_bottom(), grip.right_top()],
        Stroke::new(1.0_f32, theme.muted),
    );
    if grip_response.drag_stopped() {
        // Read from where the grip ended rather than from an accumulated
        // delta, so a drag that left the window and came back does not
        // carry the excursion into the committed size.
        let ended = grip_response
            .interact_pointer_pos()
            .unwrap_or(grip.center());
        let size = clamp_to_range(
            data,
            registry,
            vec2(ended.x - box_rect.left(), ended.y - box_rect.top()),
        );
        action = Some(NoteAction::Resize {
            width: size.x,
            height: size.y,
        });
    }

    if action.is_none()
        && !editing
        && ui.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| box_rect.contains(p))
        })
    {
        action = Some(NoteAction::Edit);
    }
    action
}

/// Clamp a proposed size to what the registry allows.
fn clamp_to_range(
    data: &NodeData,
    registry: &solarxy_graph::registry::Registry,
    proposed: Vec2,
) -> Vec2 {
    let bound = |key: &str, value: f32, fallback: (f32, f32)| -> f32 {
        let range = registry
            .get(&data.type_id)
            .and_then(|desc| desc.param(key))
            .and_then(|spec| spec.range.as_ref())
            .map(|r| r.hard);
        #[allow(clippy::cast_possible_truncation)]
        let (low, high) = range.map_or(fallback, |(l, h)| (l as f32, h as f32));
        value.clamp(low, high)
    };
    vec2(
        bound("width", proposed.x, (120.0, 800.0)),
        bound("height", proposed.y, (60.0, 600.0)),
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::document::NodeId;

    fn registry() -> solarxy_graph::registry::Registry {
        solarxy_graph::nodes::builtin_registry().expect("builtin registry")
    }

    fn note() -> NodeData {
        NodeData::new(NodeId(1), "note", 1)
    }

    fn with(key: &str, value: ParamValue) -> NodeData {
        let mut data = note();
        data.params
            .insert(key.to_string(), ParamSource::Literal(value));
        data
    }

    /// The first colour on the ring is the registry's own default, so a
    /// freshly added note is already on the cycle rather than beside it
    /// and the first click moves it rather than resetting it.
    #[test]
    fn the_ring_starts_where_the_registry_default_is() {
        let default = registry()
            .get("note")
            .and_then(|desc| desc.param("color"))
            .map(|spec| spec.default.clone())
            .expect("a note declares a colour");
        let ParamValue::Color(rgba) = default else {
            panic!("a colour param defaults to a colour");
        };
        for channel in 0..3 {
            assert!(
                (rgba[channel] - COLOURS[0][channel]).abs() < COLOUR_TOLERANCE,
                "the ring must start on the registry default"
            );
        }
    }

    /// How far apart two colours are, on the same measure the cycle
    /// matches by. Written once because a test that compared one channel
    /// stood here first and passed three colours as identical: the ring
    /// has three entries whose reds are within a hundredth of each other
    /// and which are not remotely the same colour.
    fn distance(a: [f32; 4], b: [f32; 3]) -> f32 {
        (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs()
    }

    /// No two entries on the ring are near enough to be confused, which
    /// is what makes the cycle land on the next colour rather than
    /// jumping. The property the matching tolerance depends on, and it
    /// depends on it silently.
    #[test]
    fn no_two_colours_on_the_ring_are_within_the_matching_tolerance() {
        for (i, a) in COLOURS.iter().enumerate() {
            for b in COLOURS.iter().skip(i + 1) {
                let apart = distance([a[0], a[1], a[2], 1.0], *b);
                assert!(
                    apart > COLOUR_TOLERANCE,
                    "{a:?} and {b:?} are {apart} apart, inside the tolerance the cycle matches by"
                );
            }
        }
    }

    /// The cycle visits every colour and closes, and it carries the alpha
    /// through rather than resetting it.
    #[test]
    fn the_colour_cycle_closes_and_keeps_the_alpha() {
        let mut at = [COLOURS[0][0], COLOURS[0][1], COLOURS[0][2], 0.5];
        let mut seen = vec![at];
        for _ in 0..COLOURS.len() - 1 {
            at = next_colour(at);
            assert!(
                (at[3] - 0.5).abs() < f32::EPSILON,
                "the alpha is not the ring's to change"
            );
            assert!(
                !seen
                    .iter()
                    .any(|prior| distance(*prior, [at[0], at[1], at[2]]) < COLOUR_TOLERANCE),
                "the cycle repeats before it closes"
            );
            seen.push(at);
        }
        assert_eq!(seen.len(), COLOURS.len(), "every colour is visited once");
        let closed = next_colour(at);
        assert!(
            distance(closed, COLOURS[0]) < COLOUR_TOLERANCE,
            "and it closes"
        );
    }

    /// A colour set from somewhere else starts the cycle rather than
    /// matching nothing and leaving the note stuck.
    #[test]
    fn a_colour_off_the_ring_starts_the_cycle() {
        let off = next_colour([0.1, 0.2, 0.3, 1.0]);
        assert!((off[0] - COLOURS[0][0]).abs() < COLOUR_TOLERANCE);
    }

    /// A size is clamped to the registry's own range, and it is clamped
    /// on the way in as well as on the way out: a preview that grew past
    /// the range would snap back on release, which reads as the gesture
    /// having failed.
    #[test]
    fn a_size_is_clamped_to_the_range_the_registry_declares() {
        let registry = registry();
        let huge = with("width", ParamValue::Float(5_000.0));
        assert!(
            size(&huge, &registry).x <= 800.0,
            "a width past the range must not be drawn"
        );
        let tiny = with("height", ParamValue::Float(1.0));
        assert!(size(&tiny, &registry).y >= 60.0);

        let clamped = clamp_to_range(&note(), &registry, vec2(5_000.0, 1.0));
        assert!(clamped.x <= 800.0 && clamped.y >= 60.0);
    }

    /// A note with no size params falls back to the registry's declared
    /// default rather than to nothing.
    #[test]
    fn a_note_with_no_size_params_is_the_declared_default() {
        let registry = registry();
        let declared = |key: &str| -> f32 {
            match registry
                .get("note")
                .and_then(|desc| desc.param(key))
                .map(|spec| spec.default.clone())
            {
                #[allow(clippy::cast_possible_truncation)]
                Some(ParamValue::Float(v)) => v as f32,
                other => panic!("{key} defaults to a float, not {other:?}"),
            }
        };
        let fallback = size(&note(), &registry);
        assert!((fallback.x - declared("width")).abs() < 0.01);
        assert!((fallback.y - declared("height")).abs() < 0.01);
    }

    /// Three declared sizes, three distinct fonts, and an unknown key
    /// reads as the smallest rather than as nothing.
    #[test]
    fn every_declared_text_size_is_its_own_font() {
        let small = text_size(&with("text_size", ParamValue::Enum("small".into())));
        let medium = text_size(&with("text_size", ParamValue::Enum("medium".into())));
        let large = text_size(&with("text_size", ParamValue::Enum("large".into())));
        assert!(small < medium && medium < large);
        assert!(
            (text_size(&with("text_size", ParamValue::Enum("hologram".into()))) - small).abs()
                < f32::EPSILON
        );
        assert!((text_size(&note()) - small).abs() < f32::EPSILON);
    }
}
