//! What separates a click from a drag, and a double-click from two.
//!
//! The window delivers presses, moves and releases and nothing else, so the
//! shell decides both. A press that travelled is a drag and picks nothing,
//! which is the browser's rule (`drag.moved` in its viewport); a second
//! click close in time and space to the first is a double-click, which the
//! browser gets from the DOM and this shell has to derive.
//!
//! Pure over positions and instants, so the two rules can be tested without
//! a window. Positions are in whatever unit the caller keeps the cursor in,
//! and the thresholds are handed in scaled to match.

use std::time::{Duration, Instant};

/// How far a press may travel and still release as a click, in logical
/// pixels. The browser flags a drag on the first move event that reports
/// more than one pixel; a small cumulative allowance is the same rule with
/// pointer jitter forgiven.
pub(crate) const CLICK_SLOP_PX: f32 = 2.0;

/// Two clicks within this interval are one double-click.
pub(crate) const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);

/// Two clicks within this distance are one double-click, in logical pixels.
pub(crate) const DOUBLE_CLICK_PX: f32 = 4.0;

/// A press that has not been released yet.
#[derive(Debug, Clone, Copy)]
struct Press {
    at: (f32, f32),
    moved: bool,
}

/// What a release amounted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Click {
    Single,
    /// The second of two clicks close together. The first already released
    /// as a [`Click::Single`], so a caller acts on both, as the browser
    /// does with its click and dblclick events.
    Double,
}

/// The press in flight and the last click, for the two rules above.
#[derive(Debug, Default)]
pub(crate) struct ClickTracker {
    press: Option<Press>,
    last_click: Option<(Instant, (f32, f32))>,
}

impl ClickTracker {
    /// A primary button went down at `at`.
    pub(crate) fn press(&mut self, at: (f32, f32)) {
        self.press = Some(Press { at, moved: false });
    }

    /// The pointer moved to `pos` with the button held. Once the press has
    /// travelled past `slop`, it is a drag for good: coming back does not
    /// make it a click again.
    pub(crate) fn moved_to(&mut self, pos: (f32, f32), slop: f32) {
        if let Some(press) = self.press.as_mut()
            && !press.moved
            && distance(press.at, pos) > slop
        {
            press.moved = true;
        }
    }

    /// The button came up at `at`. A click when the press never travelled,
    /// and a double-click when the previous click was close in time and
    /// space; `None` for a drag or a release with no press seen.
    pub(crate) fn release(
        &mut self,
        at: (f32, f32),
        now: Instant,
        double_within: Duration,
        double_px: f32,
    ) -> Option<Click> {
        let press = self.press.take()?;
        if press.moved {
            // A drag ends the sequence: the click after it starts fresh.
            self.last_click = None;
            return None;
        }
        let double = self.last_click.is_some_and(|(when, from)| {
            now.duration_since(when) <= double_within && distance(from, at) <= double_px
        });
        // A double-click closes its pair, so a third click begins a new one
        // rather than chaining a triple.
        self.last_click = if double { None } else { Some((now, at)) };
        Some(if double { Click::Double } else { Click::Single })
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITHIN: Duration = Duration::from_millis(400);

    #[test]
    fn a_press_released_in_place_is_a_click() {
        let mut t = ClickTracker::default();
        let now = Instant::now();
        t.press((10.0, 10.0));
        assert_eq!(
            t.release((10.0, 10.0), now, WITHIN, 4.0),
            Some(Click::Single)
        );
    }

    #[test]
    fn a_press_that_travelled_is_a_drag_and_picks_nothing() {
        let mut t = ClickTracker::default();
        let now = Instant::now();
        t.press((10.0, 10.0));
        t.moved_to((14.0, 10.0), CLICK_SLOP_PX);
        // Coming back to the press point does not make it a click again.
        t.moved_to((10.0, 10.0), CLICK_SLOP_PX);
        assert_eq!(t.release((10.0, 10.0), now, WITHIN, 4.0), None);
    }

    #[test]
    fn jitter_inside_the_slop_is_still_a_click() {
        let mut t = ClickTracker::default();
        let now = Instant::now();
        t.press((10.0, 10.0));
        t.moved_to((11.0, 10.5), CLICK_SLOP_PX);
        assert_eq!(
            t.release((11.0, 10.5), now, WITHIN, 4.0),
            Some(Click::Single)
        );
    }

    #[test]
    fn two_clicks_close_in_time_and_space_are_a_double() {
        let mut t = ClickTracker::default();
        let first = Instant::now();
        t.press((10.0, 10.0));
        assert_eq!(
            t.release((10.0, 10.0), first, WITHIN, 4.0),
            Some(Click::Single)
        );
        t.press((11.0, 12.0));
        let second = first + Duration::from_millis(200);
        assert_eq!(
            t.release((11.0, 12.0), second, WITHIN, 4.0),
            Some(Click::Double)
        );
    }

    #[test]
    fn a_slow_second_click_is_a_single() {
        let mut t = ClickTracker::default();
        let first = Instant::now();
        t.press((10.0, 10.0));
        t.release((10.0, 10.0), first, WITHIN, 4.0);
        t.press((10.0, 10.0));
        let late = first + Duration::from_millis(401);
        assert_eq!(
            t.release((10.0, 10.0), late, WITHIN, 4.0),
            Some(Click::Single)
        );
    }

    #[test]
    fn a_far_second_click_is_a_single() {
        let mut t = ClickTracker::default();
        let first = Instant::now();
        t.press((10.0, 10.0));
        t.release((10.0, 10.0), first, WITHIN, 4.0);
        t.press((20.0, 10.0));
        let second = first + Duration::from_millis(100);
        assert_eq!(
            t.release((20.0, 10.0), second, WITHIN, 4.0),
            Some(Click::Single)
        );
    }

    #[test]
    fn a_double_click_closes_its_pair_so_a_third_click_starts_over() {
        let mut t = ClickTracker::default();
        let first = Instant::now();
        for (i, expected) in [Click::Single, Click::Double, Click::Single]
            .into_iter()
            .enumerate()
        {
            t.press((10.0, 10.0));
            let at = first + Duration::from_millis(100 * i as u64);
            assert_eq!(t.release((10.0, 10.0), at, WITHIN, 4.0), Some(expected));
        }
    }

    #[test]
    fn a_drag_between_two_clicks_breaks_the_pair() {
        let mut t = ClickTracker::default();
        let first = Instant::now();
        t.press((10.0, 10.0));
        t.release((10.0, 10.0), first, WITHIN, 4.0);
        t.press((10.0, 10.0));
        t.moved_to((30.0, 10.0), CLICK_SLOP_PX);
        assert_eq!(
            t.release((30.0, 10.0), first + Duration::from_millis(50), WITHIN, 4.0),
            None
        );
        t.press((10.0, 10.0));
        assert_eq!(
            t.release(
                (10.0, 10.0),
                first + Duration::from_millis(100),
                WITHIN,
                4.0
            ),
            Some(Click::Single)
        );
    }

    #[test]
    fn a_release_with_no_press_is_nothing() {
        let mut t = ClickTracker::default();
        assert_eq!(t.release((0.0, 0.0), Instant::now(), WITHIN, 4.0), None);
    }
}
