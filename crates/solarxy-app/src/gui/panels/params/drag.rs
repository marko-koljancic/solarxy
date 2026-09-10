//! Scrubbing a number: the preview lane, and one undo step per gesture.
//!
//! **A drag previews and commits once.** The engine has a non-committing
//! lane for exactly this: `preview_param` writes no document state, raises
//! no event and adds no undo entry, it only dirty-marks so the next cook
//! resolves the previewed value instead of the stored one. A drag that
//! wrote through `SetParam` on every frame would be correct on screen and
//! wrong everywhere else: sixty undo steps for one gesture, sixty
//! document writes, and a cook queue that never drains because each write
//! invalidates the one before it.
//!
//! **An abandoned drag must clear its own preview.** The preview is only
//! otherwise dropped by the committing write, so a cancelled gesture that
//! forgets leaves the viewport asserting the dragged value while the panel
//! and the document both say something else, with nothing to end the
//! disagreement.
//!
//! ## The precision drag
//!
//! Middle-button, carried from the browser: horizontal travel scrubs at
//! the selected decade, vertical travel picks the decade from a floating
//! ladder. This shell drives it by hand because egui's drag value reads
//! the primary button only, and because the decade ladder is the point.
//!
//! The two rules below are pure and are the browser's, constant for
//! constant, held there by [`tests::the_precision_ladder_matches_the_browsers`]
//! reading the TypeScript. They are **not** in `solarxy-studio` even
//! though they would fit its charter: the browser calls its own copy from
//! a pointer-move handler, and crossing the WebAssembly boundary to
//! multiply three floats it is already holding is not a trade worth
//! making. What is shared instead is the guard.

use egui::Pos2;
use solarxy_graph::document::{GraphContext, NodeId};

/// The decades the ladder offers, coarsest first.
pub(super) const DECADES: [f64; 6] = [1.0, 0.1, 0.01, 0.001, 0.0001, 0.00001];
/// How tall one rung of the ladder is, in points of vertical travel.
pub(super) const ROW_HEIGHT: f32 = 28.0;
/// Travel below this does not select at all, so a horizontal scrub with a
/// little wobble in it stays on one decade.
pub(super) const DEADZONE: f32 = 6.0;
/// Once a rung is selected, this much further travel is needed to leave
/// it, so a pointer resting on a boundary does not flicker between two.
pub(super) const HYSTERESIS: f32 = 12.0;
/// A hundredth, which is where a drag starts.
pub(super) const DEFAULT_DECADE: usize = 2;
/// Points of horizontal travel per decade step.
pub(super) const SENSITIVITY: f64 = 0.5;

/// Which rung the vertical travel selects.
///
/// Returns the rung and the pointer position that chose it, which is what
/// the next hysteresis gate measures against.
#[must_use]
pub(super) fn select_decade(
    delta_y: f32,
    pointer_y: f32,
    current: usize,
    last_change_y: f32,
) -> (usize, f32) {
    if delta_y.abs() < DEADZONE {
        return (current, last_change_y);
    }
    #[allow(clippy::cast_possible_truncation)]
    let row = (delta_y / ROW_HEIGHT).floor() as i32;
    #[allow(clippy::cast_possible_wrap)]
    let candidate = (row + DEFAULT_DECADE as i32).clamp(0, DECADES.len() as i32 - 1);
    #[allow(clippy::cast_sign_loss)]
    let candidate = candidate as usize;
    if candidate != current && (pointer_y - last_change_y).abs() >= HYSTERESIS {
        return (candidate, pointer_y);
    }
    (current, last_change_y)
}

/// Where the horizontal travel puts the value.
///
/// From the value the gesture **started** at rather than from the last
/// frame's, so the scrub is a function of where the pointer is and a
/// dropped frame costs nothing.
#[must_use]
pub(super) fn scrub_value(original: f64, delta_x: f32, decade: f64, int: bool) -> f64 {
    let value = original + f64::from(delta_x) * decade * SENSITIVITY;
    if int { value.round() } else { value }
}

/// How a drag is being driven.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum DragKind {
    /// egui's own drag on the field or the slider. The widget reports the
    /// value; this only records that a gesture is open, so the frames in
    /// between preview instead of writing.
    Widget,
    /// The middle-button precision drag, driven here.
    Precision {
        origin: Pos2,
        last_change_y: f32,
        decade: usize,
    },
}

/// A numeric gesture in flight.
#[derive(Debug, Clone)]
pub(super) struct NumericDrag {
    ctx: GraphContext,
    node: NodeId,
    key: String,
    /// Every component's value when the gesture began.
    ///
    /// The whole parameter rather than the one component being scrubbed,
    /// because the preview lane and the commit both write a whole
    /// `ParamValue`: a vector previewed from one component alone would
    /// zero the other two.
    original: Vec<f64>,
    current: Vec<f64>,
    /// Which component is being scrubbed. Zero for a scalar.
    slot: usize,
    kind: DragKind,
}

impl NumericDrag {
    pub(super) fn begin(
        ctx: GraphContext,
        node: NodeId,
        key: &str,
        values: Vec<f64>,
        slot: usize,
        kind: DragKind,
    ) -> Self {
        Self {
            ctx,
            node,
            key: key.to_string(),
            current: values.clone(),
            original: values,
            slot,
            kind,
        }
    }

    pub(super) fn owns(&self, node: NodeId, key: &str) -> bool {
        self.node == node && self.key == key
    }

    pub(super) fn slot(&self) -> usize {
        self.slot
    }

    pub(super) fn ctx(&self) -> GraphContext {
        self.ctx
    }

    pub(super) fn node(&self) -> NodeId {
        self.node
    }

    pub(super) fn key(&self) -> &str {
        &self.key
    }

    pub(super) fn values(&self) -> &[f64] {
        &self.current
    }

    /// What the gesture would put back if it were cancelled.
    ///
    /// Nothing reads this outside the tests today: a cancel drops the
    /// preview rather than previewing the original back, because dropping
    /// it makes the next cook resolve the stored value, which *is* the
    /// original. The browser previews the original back because its cancel
    /// arrives from a window listener that cannot know a cook is coming.
    #[cfg(test)]
    pub(super) fn original(&self) -> &[f64] {
        &self.original
    }

    /// Record a component's live value.
    pub(super) fn set(&mut self, slot: usize, value: f64) {
        if let Some(slot) = self.current.get_mut(slot) {
            *slot = value;
        }
    }

    /// Advance a precision drag to a pointer position, answering the live
    /// value for the scrubbed component.
    pub(super) fn advance(&mut self, pointer: Pos2, int: bool) -> f64 {
        let DragKind::Precision {
            origin,
            last_change_y,
            decade,
        } = self.kind
        else {
            return self.current.get(self.slot).copied().unwrap_or_default();
        };
        let (next, change_y) =
            select_decade(pointer.y - origin.y, pointer.y, decade, last_change_y);
        self.kind = DragKind::Precision {
            origin,
            last_change_y: change_y,
            decade: next,
        };
        let base = self.original.get(self.slot).copied().unwrap_or_default();
        let value = scrub_value(base, pointer.x - origin.x, DECADES[next], int);
        self.set(self.slot, value);
        value
    }

    /// Which rung a precision drag is on, for the ladder that draws it.
    pub(super) fn decade(&self) -> Option<usize> {
        match self.kind {
            DragKind::Precision { decade, .. } => Some(decade),
            DragKind::Widget => None,
        }
    }

    /// Whether the gesture moved the value at all.
    ///
    /// A press and release that scrubbed nothing writes nothing, for the
    /// same reason an untouched text field does: an undo step that
    /// restores the value it already had is worse than no step.
    pub(super) fn moved(&self) -> bool {
        self.current
            .iter()
            .zip(&self.original)
            .any(|(now, then)| (now - then).abs() > f64::EPSILON)
    }
}

/// What a numeric row shows: the live value of a gesture on it, else what
/// is stored.
///
/// A drag previews rather than writes, so the document still holds the
/// value the gesture started from. A row reading storage would sit
/// perfectly still while the viewport moved.
#[must_use]
pub(super) fn shown(
    drag: Option<&NumericDrag>,
    node: NodeId,
    key: &str,
    slot: usize,
    stored: f64,
) -> f64 {
    match drag {
        Some(drag) if drag.owns(node, key) => drag.values().get(slot).copied().unwrap_or(stored),
        _ => stored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTX: GraphContext = GraphContext::Root;
    const NODE: NodeId = NodeId(1);
    const OTHER: NodeId = NodeId(2);

    fn precision(origin_y: f32) -> DragKind {
        DragKind::Precision {
            origin: Pos2::new(0.0, origin_y),
            last_change_y: origin_y,
            decade: DEFAULT_DECADE,
        }
    }

    /// A horizontal scrub with a little wobble in it stays on one decade.
    ///
    /// Upward, deliberately: a downward wobble of the same size floors to
    /// the row it started on and would hold with or without the deadzone,
    /// so a test written that way passes whether the rule exists or not.
    /// Five points up floors to the row above.
    #[test]
    fn a_wobble_inside_the_deadzone_keeps_the_decade() {
        const _: () = assert!(
            DEADZONE < ROW_HEIGHT,
            "a deadzone wider than a row selects nothing, which is a different rule"
        );
        let (index, change) = select_decade(-(DEADZONE - 1.0), 100.0, DEFAULT_DECADE, 0.0);
        assert_eq!(index, DEFAULT_DECADE);
        assert!((change - 0.0).abs() < f32::EPSILON);
        // The same travel one point further really does move, which is
        // what makes the assertion above about the deadzone rather than
        // about the row arithmetic.
        let (moved, _) = select_decade(-(DEADZONE + 1.0), 100.0, DEFAULT_DECADE, 0.0);
        assert_eq!(moved, DEFAULT_DECADE - 1);
    }

    #[test]
    fn travelling_a_row_moves_a_rung_and_the_ladder_has_ends() {
        // One row up from the origin is one rung coarser, and the change is
        // recorded at the pointer that caused it.
        let (index, change) = select_decade(-ROW_HEIGHT, 60.0, DEFAULT_DECADE, 0.0);
        assert_eq!(index, DEFAULT_DECADE - 1);
        assert!((change - 60.0).abs() < f32::EPSILON);

        // And the ladder does not run off either end.
        let (top, _) = select_decade(-ROW_HEIGHT * 40.0, 900.0, DEFAULT_DECADE, 0.0);
        assert_eq!(top, 0);
        let (bottom, _) = select_decade(ROW_HEIGHT * 40.0, 900.0, DEFAULT_DECADE, 0.0);
        assert_eq!(bottom, DECADES.len() - 1);
    }

    /// A pointer sitting on a boundary must not flicker between two rungs.
    #[test]
    fn a_rung_holds_until_the_pointer_has_really_left_it() {
        // Far enough down to name a new rung, but not far from the last
        // change, so the rung stands.
        let (index, change) = select_decade(ROW_HEIGHT, 100.0, DEFAULT_DECADE, 95.0);
        assert_eq!(index, DEFAULT_DECADE, "within the hysteresis distance");
        assert!((change - 95.0).abs() < f32::EPSILON);

        // The same travel, from a change that is far enough behind, moves.
        let (index, _) = select_decade(ROW_HEIGHT, 100.0, DEFAULT_DECADE, 100.0 - HYSTERESIS);
        assert_eq!(index, DEFAULT_DECADE + 1);
    }

    #[test]
    fn a_scrub_is_measured_from_where_the_gesture_began() {
        // Two hundred points at a hundredth, half a point of travel each.
        let moved = scrub_value(1.0, 200.0, DECADES[DEFAULT_DECADE], false);
        assert!((moved - 2.0).abs() < 1e-9, "{moved}");
        // Backwards, and from the same origin rather than from the result
        // above: a dropped frame must cost nothing.
        let back = scrub_value(1.0, -200.0, DECADES[DEFAULT_DECADE], false);
        assert!((back - 0.0).abs() < 1e-9, "{back}");
        // An integer lands on an integer.
        let snapped = scrub_value(1.0, 3.0, DECADES[0], true);
        assert!((snapped - 3.0).abs() < f64::EPSILON, "{snapped}");
    }

    #[test]
    fn a_row_with_no_gesture_on_it_shows_what_is_stored() {
        assert!((shown(None, NODE, "size", 0, 4.0) - 4.0).abs() < f64::EPSILON);
        let drag = NumericDrag::begin(CTX, NODE, "size", vec![1.0], 0, DragKind::Widget);
        // Another node, and another parameter, both read storage.
        assert!((shown(Some(&drag), OTHER, "size", 0, 4.0) - 4.0).abs() < f64::EPSILON);
        assert!((shown(Some(&drag), NODE, "scale", 0, 4.0) - 4.0).abs() < f64::EPSILON);
    }

    /// The document is not written during a drag, so a row that read it
    /// would sit still while the viewport moved.
    #[test]
    fn a_dragged_row_shows_the_gesture_rather_than_the_document() {
        let mut drag = NumericDrag::begin(CTX, NODE, "size", vec![1.0], 0, DragKind::Widget);
        drag.set(0, 7.5);
        assert!((shown(Some(&drag), NODE, "size", 0, 1.0) - 7.5).abs() < f64::EPSILON);
    }

    /// A vector drag carries every component, because the preview and the
    /// commit both write a whole value.
    #[test]
    fn a_vector_drag_carries_the_components_it_is_not_scrubbing() {
        let mut drag = NumericDrag::begin(
            CTX,
            NODE,
            "translate",
            vec![1.0, 2.0, 3.0],
            1,
            DragKind::Widget,
        );
        drag.set(1, 9.0);
        assert_eq!(drag.values(), &[1.0, 9.0, 3.0]);
        assert_eq!(
            drag.original(),
            &[1.0, 2.0, 3.0],
            "a cancel puts the whole parameter back, not one third of it"
        );
    }

    /// A press and release that scrubbed nothing writes nothing.
    #[test]
    fn a_gesture_that_did_not_move_the_value_is_not_an_edit() {
        let mut drag = NumericDrag::begin(CTX, NODE, "size", vec![1.0], 0, DragKind::Widget);
        assert!(!drag.moved());
        drag.set(0, 1.0);
        assert!(!drag.moved(), "the same value is not a move");
        drag.set(0, 1.5);
        assert!(drag.moved());
    }

    /// The precision drag scrubs horizontally and picks its decade
    /// vertically, in one gesture.
    #[test]
    fn the_precision_drag_scrubs_across_and_selects_down() {
        let mut drag = NumericDrag::begin(CTX, NODE, "size", vec![1.0], 0, precision(0.0));
        assert_eq!(drag.decade(), Some(DEFAULT_DECADE));

        // Purely horizontal: the decade stands and the value scrubs.
        let value = drag.advance(Pos2::new(200.0, 0.0), false);
        assert!((value - 2.0).abs() < 1e-9, "{value}");
        assert_eq!(drag.decade(), Some(DEFAULT_DECADE));

        // Now down two rows, far enough to clear the hysteresis: a finer
        // decade, and the same horizontal travel moves the value less.
        let value = drag.advance(Pos2::new(200.0, ROW_HEIGHT * 2.0), false);
        assert_eq!(drag.decade(), Some(DEFAULT_DECADE + 2));
        assert!((value - 1.01).abs() < 1e-9, "{value}");
    }

    /// A widget drag has no rung, so nothing draws a ladder over it.
    #[test]
    fn only_the_precision_drag_has_a_ladder() {
        let drag = NumericDrag::begin(CTX, NODE, "size", vec![1.0], 0, DragKind::Widget);
        assert_eq!(drag.decade(), None);
        assert_eq!(drag.ctx(), CTX);
        assert_eq!(drag.node(), NODE);
        assert_eq!(drag.key(), "size");
        assert_eq!(drag.slot(), 0);
    }

    /// The ladder is the browser's, constant for constant.
    ///
    /// A source scan for the same reason its neighbours are one: the two
    /// copies are in different languages and nothing else compares them,
    /// and a drag that felt different on the two shells would read as one
    /// of them being broken rather than as a number having drifted.
    #[test]
    fn the_precision_ladder_matches_the_browsers() {
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/hooks/usePrecisionDrag.ts"
        ))
        .expect("the browser's precision drag");

        let number_after = |needle: &str| -> f64 {
            let tail = source
                .split_once(needle)
                .unwrap_or_else(|| panic!("`{needle}` is gone from the browser's copy"))
                .1;
            let value: String = tail
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();
            value
                .parse()
                .unwrap_or_else(|_| panic!("`{needle}` no longer reads as a number"))
        };

        #[allow(clippy::cast_possible_truncation)]
        let f32_after = |needle: &str| number_after(needle) as f32;
        assert!((f32_after("export const ROW_HEIGHT =") - ROW_HEIGHT).abs() < f32::EPSILON);
        assert!((f32_after("export const DEADZONE =") - DEADZONE).abs() < f32::EPSILON);
        assert!((f32_after("export const HYSTERESIS =") - HYSTERESIS).abs() < f32::EPSILON);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let default_index = number_after("const DEFAULT_INDEX =") as usize;
        assert_eq!(default_index, DEFAULT_DECADE);
        assert!((number_after("const SENSITIVITY =") - SENSITIVITY).abs() < f64::EPSILON);

        let decades: Vec<f64> = source
            .split_once("export const PRECISION_DECADES = [")
            .expect("the browser's decade list")
            .1
            .split_once(']')
            .expect("an unterminated list")
            .0
            .split(',')
            .map(|part| part.trim().parse().expect("a decade"))
            .collect();
        assert_eq!(
            decades,
            DECADES.to_vec(),
            "the two shells scrub at different precisions"
        );
    }
}
