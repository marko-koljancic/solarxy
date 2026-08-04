//! The readability floor a theme has to clear.
//!
//! # Why a theme system needs a floor at all
//!
//! Every other Solarxy shell paints onto a window it created, so it knows what
//! its own background is. This one paints onto whatever the reader's terminal
//! already was. Once themes become files, a user can pair ink and ground that
//! nobody ever looked at together, and the failure mode is not ugliness: it is
//! a tool that renders and cannot be read. A theme system able to do that is a
//! defect rather than a feature, so the floor is not optional and the refusal
//! is loud.
//!
//! # What the floor can and cannot judge
//!
//! WCAG 2.1 relative luminance needs an actual RGB triple. `Color::from_str`
//! also accepts the named ANSI slots and bare palette indices, and those have
//! no triple we can know: what the terminal maps them to is precisely the
//! thing this shell refuses to assume, and the reason the lower tiers use them
//! in the first place.
//!
//! So a pair involving a non-RGB colour is **skipped, not failed**. The floor
//! is a real guarantee for the pairs it can see and is silent about the rest.
//! Saying that plainly is better than implying a promise the mechanism cannot
//! keep.

use ratatui::style::Color;

/// WCAG 2.1 AA for body text.
pub const MINIMUM_RATIO: f32 = 4.5;

/// One slot pair that fell below the floor, named so the refusal can say what
/// to fix rather than only that something is wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastFailure {
    /// The foreground slot's name, as written in the theme file.
    pub ink: &'static str,
    /// The background slot's name.
    pub ground: &'static str,
    pub ratio: f32,
}

impl std::fmt::Display for ContrastFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} on {} is {:.2}:1, below the {:.1}:1 floor",
            self.ink, self.ground, self.ratio, MINIMUM_RATIO
        )
    }
}

/// Check one pair.
///
/// `None` when either colour carries no RGB, which is a skip rather than a
/// pass: see the module docs.
pub fn check(
    ink_name: &'static str,
    ink: Color,
    ground_name: &'static str,
    ground: Color,
) -> Option<ContrastFailure> {
    let ratio = ratio(ink, ground)?;
    (ratio < MINIMUM_RATIO).then_some(ContrastFailure {
        ink: ink_name,
        ground: ground_name,
        ratio,
    })
}

/// The WCAG contrast ratio between two colours, if both are RGB.
pub fn ratio(a: Color, b: Color) -> Option<f32> {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return None;
    };
    let first = relative_luminance(ar, ag, ab);
    let second = relative_luminance(br, bg, bb);
    let (lighter, darker) = if first >= second {
        (first, second)
    } else {
        (second, first)
    };
    Some((lighter + 0.05) / (darker + 0.05))
}

/// WCAG 2.1 relative luminance.
///
/// Note the gamma decode. This is not the same quantity as the linear-light
/// luma the renderer computes with the same three weights: that one is fed
/// values already in linear space, this one has to leave sRGB first. Skipping
/// the decode would overstate the contrast of dark pairs, which is exactly
/// where a terminal theme goes wrong.
fn relative_luminance(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn channel(value: u8) -> f32 {
    let c = f32::from(value) / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Color = Color::Rgb(0, 0, 0);
    const WHITE: Color = Color::Rgb(255, 255, 255);

    /// The two anchors the whole scale is defined against. Getting either
    /// wrong means every judgement in between is wrong by the same factor.
    #[test]
    fn the_extremes_are_the_published_values() {
        let extreme = ratio(WHITE, BLACK).expect("both are rgb");
        assert!(
            (extreme - 21.0).abs() < 0.01,
            "white on black should be 21:1, got {extreme}"
        );
        let same = ratio(WHITE, WHITE).expect("both are rgb");
        assert!((same - 1.0).abs() < 0.001, "a colour on itself is 1:1");
    }

    /// Contrast is symmetric: the ratio describes a pair, not a direction.
    #[test]
    fn the_order_of_the_pair_does_not_matter() {
        let amber = Color::Rgb(230, 180, 80);
        assert_eq!(ratio(amber, BLACK), ratio(BLACK, amber));
    }

    /// The gamma decode is the part most easily left out, and leaving it out
    /// is not a rounding difference. Linear-light weighting reports a much
    /// brighter mid grey, which would wave through dark pairs that a reader
    /// cannot actually separate.
    #[test]
    fn the_gamma_decode_is_not_optional() {
        let mid = relative_luminance(128, 128, 128);
        let undecoded = 128.0 / 255.0;
        assert!(
            mid < 0.25,
            "mid grey's relative luminance should be near 0.216, got {mid}"
        );
        assert!(
            undecoded - mid > 0.25,
            "skipping the decode barely changed the answer, so the test proves nothing"
        );
    }

    #[test]
    fn a_failing_pair_names_both_slots_and_its_ratio() {
        let failure = check("ink", Color::Rgb(90, 90, 90), "ground", BLACK)
            .expect("dark grey on black is unreadable");
        assert_eq!(failure.ink, "ink");
        assert_eq!(failure.ground, "ground");
        assert!(failure.ratio < MINIMUM_RATIO);

        let rendered = failure.to_string();
        assert!(rendered.contains("ink on ground"), "{rendered}");
        assert!(rendered.contains("4.5:1"), "{rendered}");
    }

    #[test]
    fn a_clearing_pair_is_not_a_failure() {
        assert!(check("ink", WHITE, "ground", BLACK).is_none());
        assert!(
            check(
                "ink",
                Color::Rgb(199, 199, 199),
                "ground",
                Color::Rgb(20, 20, 20)
            )
            .is_none()
        );
    }

    /// A named slot or an index is the terminal's to define, so we cannot
    /// judge it. Skipping is the honest answer; failing would refuse themes
    /// that are fine and passing would claim a guarantee we cannot make.
    #[test]
    fn a_pair_we_cannot_see_is_skipped_rather_than_judged() {
        assert_eq!(ratio(Color::Reset, BLACK), None);
        assert_eq!(ratio(WHITE, Color::DarkGray), None);
        assert_eq!(ratio(Color::Indexed(240), BLACK), None);
        assert!(check("ink", Color::Reset, "ground", BLACK).is_none());
    }

    /// The boundary itself, from either side, so the comparison cannot drift
    /// into rejecting a theme that sits exactly on the floor.
    #[test]
    fn the_threshold_admits_the_boundary() {
        // #767676 on white is the canonical 4.54:1 pair used to illustrate
        // the AA floor for body text.
        let ratio = ratio(Color::Rgb(0x76, 0x76, 0x76), WHITE).expect("both are rgb");
        assert!(
            (4.5..4.6).contains(&ratio),
            "expected the canonical boundary pair near 4.54:1, got {ratio}"
        );
        assert!(check("ink", Color::Rgb(0x76, 0x76, 0x76), "ground", WHITE).is_none());
        assert!(check("ink", Color::Rgb(0x80, 0x80, 0x80), "ground", WHITE).is_some());
    }
}
