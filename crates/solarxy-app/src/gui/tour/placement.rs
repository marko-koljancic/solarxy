//! Where a tour card sits relative to what it points at.
//!
//! Geometry with no toolkit in it, like the palette's placement rule and
//! for the same reason: both shells draw a card beside a rectangle, and
//! neither should decide where it goes twice. This one is not shared
//! across the boundary because the browser measures a DOM rect and this
//! shell measures an egui one; what is shared is the arithmetic, held here
//! against the browser's own by `the_rule_is_the_browsers`.
//!
//! **The fallback order is not "most room".** When the preferred side has
//! no room the browser sorts every side by the room it has *minus the room
//! it needs*, which is a different ordering whenever the card is not
//! square, and breaks ties in its own key order. A step pointing at the
//! left-edge tool column is the case that forced it: asking for `left`
//! there must not push the card off-screen.

use super::steps::Side;

/// Gap between the anchor and the card, and between the card and the edge.
pub(in crate::gui) const GAP: f32 = 12.0;

/// The card's size before it is measured. The browser carries the same two
/// numbers for the same reason: the first frame places a card that has not
/// been laid out yet.
pub(in crate::gui) const CARD_WIDTH: f32 = 320.0;
pub(in crate::gui) const CARD_HEIGHT: f32 = 168.0;

/// The order the browser's fallback iterates in, which is the order its
/// own record of room is written in. It matters only for ties, and ties
/// happen: two sides of a centred anchor have the same room.
const SIDES: [Side; 4] = [Side::Top, Side::Bottom, Side::Left, Side::Right];

/// Clamp that degrades rather than inverting.
///
/// The upper bound is itself clamped against the lower one, so a viewport
/// too small for the card pins it at `GAP` instead of returning a negative
/// position. The browser's clamp does the same, in the same place.
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi.max(lo))
}

fn room(anchor: egui::Rect, viewport: egui::Vec2, side: Side) -> f32 {
    match side {
        Side::Top => anchor.min.y,
        Side::Bottom => viewport.y - anchor.max.y,
        Side::Left => anchor.min.x,
        Side::Right => viewport.x - anchor.max.x,
    }
}

fn needed(card: egui::Vec2, side: Side) -> f32 {
    match side {
        Side::Top | Side::Bottom => card.y + GAP,
        Side::Left | Side::Right => card.x + GAP,
    }
}

/// Where the card's top-left corner goes, and which side it ended up on.
///
/// The side comes back because the card draws an arrow towards its anchor
/// and has to know which way to point it.
pub(in crate::gui) fn place_coachmark(
    anchor: egui::Rect,
    card: egui::Vec2,
    viewport: egui::Vec2,
    preferred: Side,
) -> (egui::Pos2, Side) {
    let side = if room(anchor, viewport, preferred) >= needed(card, preferred) {
        preferred
    } else {
        // Descending by room-minus-need, ties in the browser's key order,
        // which a stable sort preserves exactly as its own does.
        let mut order = SIDES;
        order.sort_by(|a, b| {
            let slack = |s: Side| room(anchor, viewport, s) - needed(card, s);
            slack(*b)
                .partial_cmp(&slack(*a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        order[0]
    };

    let max_left = viewport.x - card.x - GAP;
    let max_top = viewport.y - card.y - GAP;
    let centre_x = anchor.center().x - card.x / 2.0;
    let centre_y = anchor.center().y - card.y / 2.0;

    let at = match side {
        Side::Top => egui::pos2(
            clamp(centre_x, GAP, max_left),
            clamp(anchor.min.y - card.y - GAP, GAP, max_top),
        ),
        Side::Bottom => egui::pos2(
            clamp(centre_x, GAP, max_left),
            clamp(anchor.max.y + GAP, GAP, max_top),
        ),
        Side::Left => egui::pos2(
            clamp(anchor.min.x - card.x - GAP, GAP, max_left),
            clamp(centre_y, GAP, max_top),
        ),
        Side::Right => egui::pos2(
            clamp(anchor.max.x + GAP, GAP, max_left),
            clamp(centre_y, GAP, max_top),
        ),
    };
    (at, side)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The browser's own test fixture, so the cases below are its cases.
    const VIEWPORT: egui::Vec2 = egui::vec2(1400.0, 800.0);
    const CARD: egui::Vec2 = egui::vec2(320.0, 160.0);

    fn anchor(left: f32, top: f32, width: f32, height: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, height))
    }

    #[test]
    fn honours_the_preferred_side_when_there_is_room() {
        let (at, side) = place_coachmark(
            anchor(600.0, 300.0, 200.0, 100.0),
            CARD,
            VIEWPORT,
            Side::Bottom,
        );
        assert_eq!(side, Side::Bottom);
        assert!((at.y - 412.0).abs() < 0.01, "below the anchor plus the gap");
        assert!((at.x - 540.0).abs() < 0.01, "centred on the anchor");
    }

    #[test]
    fn flips_away_from_an_edge_rather_than_going_off_screen() {
        let (at, side) = place_coachmark(
            anchor(600.0, 740.0, 200.0, 40.0),
            CARD,
            VIEWPORT,
            Side::Bottom,
        );
        assert_ne!(side, Side::Bottom, "there is no room below");
        assert!(at.y >= GAP && at.y + CARD.y <= VIEWPORT.y);
    }

    /// The case the fallback exists for: a step pointing at the tool column
    /// asks for the left side, where there is no room at all.
    #[test]
    fn does_not_honour_a_preferred_side_with_no_room() {
        let (at, side) =
            place_coachmark(anchor(4.0, 300.0, 40.0, 200.0), CARD, VIEWPORT, Side::Left);
        assert_ne!(side, Side::Left);
        assert!(at.x >= GAP, "the card stays on screen");
    }

    /// Anchors stepped across the whole viewport, every preferred side:
    /// the card lands inside every time.
    #[test]
    fn every_anchor_and_side_lands_inside_the_viewport() {
        let mut checked = 0;
        let mut left = 0.0_f32;
        while left < VIEWPORT.x {
            let mut top = 0.0_f32;
            while top < VIEWPORT.y {
                for side in SIDES {
                    let (at, _) =
                        place_coachmark(anchor(left, top, 120.0, 80.0), CARD, VIEWPORT, side);
                    assert!(
                        at.x >= GAP - 0.01 && at.y >= GAP - 0.01,
                        "card at {at:?} left the viewport"
                    );
                    assert!(
                        at.x + CARD.x <= VIEWPORT.x - GAP + 0.01
                            && at.y + CARD.y <= VIEWPORT.y - GAP + 0.01,
                        "card at {at:?} overhangs"
                    );
                    checked += 1;
                }
                top += 71.0;
            }
            left += 97.0;
        }
        assert!(checked > 100, "only {checked} placements swept");
    }

    /// A viewport too small for the card pins it at the gap rather than
    /// resolving to a negative corner.
    #[test]
    fn degrades_to_the_near_edge_when_the_card_cannot_fit() {
        let tiny = egui::vec2(200.0, 100.0);
        let (at, _) = place_coachmark(anchor(10.0, 10.0, 50.0, 50.0), CARD, tiny, Side::Bottom);
        assert!((at.x - GAP).abs() < 0.01 && (at.y - GAP).abs() < 0.01);
    }

    /// The two numbers and the rule are the browser's, read from its own
    /// source rather than copied into a comment here.
    #[test]
    fn the_rule_is_the_browsers() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        let placement = std::fs::read_to_string(root.join("web/src/components/tour/placement.ts"))
            .expect("the browser's placement rule");
        let gap = placement
            .split("export const GAP = ")
            .nth(1)
            .and_then(|rest| rest.split(';').next())
            .and_then(|v| v.trim().parse::<f32>().ok())
            .expect("the browser declares its gap");
        assert!((gap - GAP).abs() < f32::EPSILON, "the gaps differ");

        // The fallback sorts by room minus need, which is the line worth
        // pinning: sorting by room alone is a different rule that passes
        // every square-card case.
        assert!(
            placement.contains("space[b] - space[a] - (needed[b] - needed[a])"),
            "the browser's fallback ordering changed"
        );

        let card = std::fs::read_to_string(root.join("web/src/components/tour/Tour.tsx"))
            .expect("the browser's tour card");
        let declared = card
            .split("const CARD = { width: ")
            .nth(1)
            .and_then(|rest| rest.split(" }").next())
            .expect("the browser declares its card size");
        assert_eq!(
            declared.replace(", height:", "").trim(),
            format!("{} {}", CARD_WIDTH as i32, CARD_HEIGHT as i32),
            "the fallback card sizes differ"
        );
    }
}
